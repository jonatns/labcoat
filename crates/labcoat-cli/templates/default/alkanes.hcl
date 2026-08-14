# Deployment topology for `labcoat plan` / `labcoat apply --broadcast`.
#
# `contract` blocks are managed deployments; `alkane` blocks bind names to
# ids that exist outside this deployment — one id for every network
# (`alkane "usd" { id = [4, 65012] }`) or a per-network map
# (`id = { regtest = [4, 65012], signet = [2, 190213] }`; plan fails when
# the active network is unbound). Expressions support references
# (`contract.<name>.id`, `alkane.<name>.id`, plus `.block`/`.tx`), height
# arithmetic, and "${...}" templates — but no conditionals, loops, or
# functions. Prefer named constructor args (`args = { supply = 100 }`,
# matched to the ABI constructor's parameters) over positional arrays.
#
# Deployment only: the manifest is done when the topology is correct and
# inert. `call` blocks are for configuration that completes a deployment;
# state-changing usage (like incrementing this counter) belongs in
# tests/e2e.rs or your application.

contract "counter" {
  package = "counter"
}
