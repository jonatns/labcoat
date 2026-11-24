#!/usr/bin/env bash
# Accept a Labcoat executable by running the workflow a developer runs:
#
#   up -> init -> test -> deploy -> call -> trace -> test --e2e -> restart
#
# Every step checks the JSON envelope a command prints, not only its exit
# code, and the Counter's decoded value, not only that a call was sent.
#
#   scripts/release/acceptance.sh <labcoat> [--bundle <dir>] [--boot-only]
#
#   --bundle <dir>  a directory holding alkanes.wasm and its
#                   runtime-manifest.json to install instead of downloading
#                   the runtime (--boot-only needs only alkanes.wasm)
#   --boot-only     boot the network, fund, mine, and stop; needs no Rust
#                   toolchain, so it runs on every release target
#
# LABCOAT_DATA_HOME must name a scratch directory: the run boots, mines on,
# and stops the network of that data home, and must never be pointed at the
# one you develop on. Set LABCOAT_RPC_PORT to keep clear of a network already
# running on the default port:
#
#   LABCOAT_DATA_HOME=$(mktemp -d) LABCOAT_RPC_PORT=18555 \
#     scripts/release/acceptance.sh target/debug/labcoat --bundle ./bundle
set -euo pipefail

usage() {
  sed -n '2,23p' "$0" | sed 's/^# \{0,1\}//'
  exit 2
}

labcoat=""
bundle=""
boot_only=false
while [ $# -gt 0 ]; do
  case "$1" in
    --bundle) bundle="${2:?--bundle needs a directory}"; shift 2 ;;
    --boot-only) boot_only=true; shift ;;
    -h|--help) usage ;;
    -*) echo "unknown option: $1" >&2; usage ;;
    *) labcoat="$1"; shift ;;
  esac
done
[ -n "$labcoat" ] || usage
[ -x "$labcoat" ] || { echo "not an executable: $labcoat" >&2; exit 2; }
labcoat="$(cd "$(dirname "$labcoat")" && pwd)/$(basename "$labcoat")"
if [ -z "${LABCOAT_DATA_HOME:-}" ]; then
  echo "set LABCOAT_DATA_HOME to a scratch directory; this run must not use your own network" >&2
  exit 2
fi
mkdir -p "$LABCOAT_DATA_HOME"
command -v jq >/dev/null || { echo "jq is required" >&2; exit 2; }
command -v curl >/dev/null || { echo "curl is required" >&2; exit 2; }

step() { printf '\n== %s\n' "$*" >&2; }
fail() { printf 'FAILED: %s\n' "$*" >&2; exit 1; }

# Run a command with --json and print its envelope. Fails unless exactly one
# envelope came back and it says ok.
ok() {
  local out
  out=$("$labcoat" "$@" --json) || fail "labcoat $* exited non-zero: $out"
  jq -e '.ok == true' >/dev/null 2>&1 <<<"$out" || fail "labcoat $* did not succeed: $out"
  printf '%s\n' "$out"
}

# Run a command that must fail, and print its error code.
refused() {
  local out
  out=$("$labcoat" "$@" --json) || true
  jq -e '.ok == false' >/dev/null 2>&1 <<<"$out" || fail "labcoat $* should have failed: $out"
  jq -r '.error.code' <<<"$out"
}

rpc() {
  curl -fsS --data-binary "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"$1\",\"params\":$2}" "$endpoint" \
    | jq -r .result
}

count() {
  local out
  out=$(ok simulate counter get_count)
  jq -e '.result.status == "success"' >/dev/null <<<"$out" || fail "get_count did not succeed: $out"
  jq -r '.result.decoded.uint' <<<"$out"
}

version=$("$labcoat" --version | awk '{print $2}')
up=(up --ci)
if [ -n "$bundle" ]; then
  [ -f "$bundle/alkanes.wasm" ] || fail "$bundle has no alkanes.wasm"
  case "$(uname -s)" in
    Darwin) product=Labcoat ;;
    *) product=labcoat ;;
  esac
  runtime="$LABCOAT_DATA_HOME/$product/runtimes/cli-v$version"
  mkdir -p "$runtime"
  cp "$bundle/alkanes.wasm" "$runtime/"
  if [ -f "$bundle/runtime-manifest.json" ]; then
    cp "$bundle/runtime-manifest.json" "$runtime/"
  elif ! $boot_only; then
    # `test --e2e` verifies the runtime against its manifest before it runs.
    fail "$bundle has no runtime-manifest.json; write one with: node scripts/release/build-runtime-manifest.mjs --version $version --directory $bundle"
  fi
  up=(up --no-download --ci)
fi

work=$(mktemp -d)
cleanup() {
  "$labcoat" down --json >/dev/null 2>&1 || true
  rm -rf "$work"
}
trap cleanup EXIT

step "boot labcoat $version"
booted=$(ok "${up[@]}")
endpoint=$(jq -r .result.endpoints.rpc <<<"$booted")
jq -e '.result.status.is_ready == true' >/dev/null <<<"$booted" || fail "up reported a network that is not ready: $booted"
jq -e '.result.wallet.balanceSats >= 100000000' >/dev/null <<<"$booted" || fail "up reported an unfunded wallet: $booted"
address=$(jq -r .result.wallet.address <<<"$booted")
chain=$(rpc getblockcount '[]')
[ "$chain" -ge 101 ] || fail "chain height is $chain, expected at least 101"
[ "$chain" = "$(rpc secondaryheight '["alkanes"]')" ] || fail "the alkanes index is behind the chain"
[ "$chain" = "$(rpc secondaryheight '["esplora"]')" ] || fail "the esplora index is behind the chain"

step "fund and mine"
before=$(ok wallet utxos | jq '.result | length')
ok fund "$address" 1 >/dev/null
ok mine 1 >/dev/null
after=$(ok wallet utxos | jq '.result | length')
[ "$after" -gt "$before" ] || fail "a mined faucet payment did not reach the wallet"

if $boot_only; then
  step "stop"
  ok down >/dev/null
  echo "labcoat $version boots on $(uname -sm)" >&2
  exit 0
fi

step "init"
cd "$work"
ok init counter-app >/dev/null
cd counter-app

step "test"
ok test | jq -e '.result.passed == true' >/dev/null || fail "host tests did not pass"

step "deploy, call, and trace"
ok deploy >/dev/null
[ "$(count)" = 0 ] || fail "a new Counter does not read 0"
call=$(ok call counter increment)
jq -e '.result.status == "success"' >/dev/null <<<"$call" || fail "increment did not succeed: $call"
txid=$(jq -r '.result.txid // .result.revealTxid' <<<"$call")
trace=$(ok trace "$txid" --wait)
jq -e '.result.traces | length > 0' >/dev/null <<<"$trace" || fail "the increment has no trace: $trace"
# A decoded trace is a list of typed events: the call into the Counter and
# its return. A revert would be an event of its own.
jq -e '[.result.traces[].trace[].type] | index("call") != null and index("return") != null and index("revert") == null' >/dev/null <<<"$trace" \
  || fail "the increment's trace did not decode to a call that returned: $trace"
[ "$(count)" = 1 ] || fail "the Counter did not increment"

step "run from a subdirectory"
(
  cd contracts/counter
  ok plan >/dev/null
  [ "$(count)" = 1 ] || fail "a subdirectory resolved a different project or network"
)

step "recovery commands survive a broken labcoat.toml"
(
  broken=$(mktemp -d "$work/broken.XXXXXX")
  cd "$broken"
  printf 'network = [not toml\n' > labcoat.toml
  "$labcoat" docs --llm | grep -q "command reference" || fail "docs needs a valid labcoat.toml"
  doctor=$("$labcoat" doctor --json) || true
  jq -e '.result.checks[] | select(.name == "project config" and .status == "fail")' >/dev/null <<<"$doctor" \
    || fail "doctor did not report the broken labcoat.toml: $doctor"
)

step "e2e tests leave the developer's chain and deployments alone"
tip=$(rpc getblockcount '[]')
instance=$(ok status | jq -r .result.instance_id)
lock=$(ok lock show | jq -cS .result)
[ "$(refused test --e2e no_such_e2e_test_zz)" = TEST_FAILED ] || fail "an e2e filter that matches nothing must fail"
e2e=$(ok test --e2e)
jq -e '.result.passed == true and .result.tests >= 1' >/dev/null <<<"$e2e" || fail "e2e tests did not run: $e2e"
[ "$(jq -r .result.network.rpcUrl <<<"$e2e")" != "$endpoint" ] || fail "e2e tests ran on the developer's network"
[ "$(rpc getblockcount '[]')" = "$tip" ] || fail "an e2e run moved the developer's chain tip"
[ "$(ok status | jq -r .result.instance_id)" = "$instance" ] || fail "an e2e run reset the developer's chain"
[ "$(ok lock show | jq -cS .result)" = "$lock" ] || fail "an e2e run replaced the developer's deployment records"
[ "$(count)" = 1 ] || fail "an e2e run changed the developer's Counter"

step "restart"
# A resumed chain must keep its contracts and index the first block after.
before=$(ok wallet utxos | jq '.result | length')
ok down >/dev/null
ok "${up[@]}" >/dev/null
ok fund "$address" 1 >/dev/null
ok mine 1 >/dev/null
after=$(ok wallet utxos | jq '.result | length')
[ "$after" -gt "$before" ] || fail "the resumed chain did not index a new block"
[ "$(count)" = 1 ] || fail "the Counter did not survive a restart"

step "stop"
ok down >/dev/null
echo "labcoat $version passed acceptance on $(uname -sm)" >&2
