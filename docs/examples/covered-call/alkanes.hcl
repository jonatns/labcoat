# Covered-call deployment topology — the declarative half of what
# tests/wallets/covered-call.sh used to do imperatively.
#
# Deployment only: the manifest is done when the topology is correct and
# inert. Value-moving operations (the writer escrowing tFIRE, the buyer
# flow) are protocol usage and live in tests/e2e.rs.
#
# Reserve numbers are fixed: `labcoat test --e2e` resets the chain before
# applying, so no run_id arithmetic is needed.

# tUSD is buyer-deployed in the e2e test at reserve 65012 — it exists
# outside this deployment, so it is an external alkane reference: a
# symbolic name for a fixed id, never deployed here and never a
# dependency edge. This fixture only ever runs on the labcoat network, so
# one binding covers it; a shared deployment would use a per-network map:
#   id = { labcoat = [4, 65012], signet = [2, 190213] }
alkane "usd" {
  id = [4, 65012]
}

# Named args are matched to the ABI constructor's parameter names, so the
# manifest documents each value instead of relying on position.
contract "fire" {
  package = "strata_test_token"
  reserve = 65011
  args = {
    variant = 1   # tFIRE
    supply  = 100
  }
}

contract "writer_claims" {
  package = "strata_test_token"
  reserve = 65013
  args = {
    variant = 3   # WRITER
    supply  = 100
  }
}

contract "series" {
  package = "strata_series"
  reserve = 65014
  args = {
    underlying    = contract.fire.id
    quote         = alkane.usd.id
    writer_claim  = contract.writer_claims.id
    contract_size = 1
    strike        = 75
    expiry        = height + 100
    supply        = 100
  }
}
