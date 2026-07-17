# Security policy

Labcoat is early-stage software for local Alkanes development. Interfaces may change before 1.0, and mainnet deployment controls are not production-ready.

## Supported versions

Security fixes target the latest published `cli-v*` release and the current `main` branch. Older prerelease versions may not receive fixes. Labcoat supports macOS and Linux; Windows is not supported.

## Report a vulnerability

Please use [GitHub private vulnerability reporting](https://github.com/jonatns/labcoat/security/advisories/new) so maintainers can investigate before details are public. If private reporting is unavailable, email `jonatns@gmail.com`. Do not include secrets or exploitable details in a public issue.

Include the affected Labcoat version, operating system, reproduction steps, potential impact, and any suggested mitigation. Avoid accessing data or systems that you do not own while testing.

## Threat boundaries

- Labcoat downloads and orchestrates several local services. A single CLI entry point does not make those services one process or one trust boundary.
- The local regtest environment and its default credentials are development conveniences. Do not expose its ports to untrusted networks.
- Labcoat wallet material is for local development. Do not import production seed phrases or fund generated addresses with real bitcoin.
- The installer requires `sha256sum` or `shasum` and verifies published checksums automatically. Users who need stronger provenance should also verify GitHub artifact attestations.
- Node.js is required by the local gateway. Dependencies and pinned upstream revisions remain part of the runtime supply-chain boundary.
- Production mainnet deployment policy, durable runtime state, hosted operation, and team access controls are outside the supported security model.

See [Stability and releases](https://labcoat.sh/docs/reference/stability/) for compatibility and product-scope details.
