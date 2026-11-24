# Wallets and keys

Status: the local wallet and keystore locations are implemented. Everything
under "Public networks" is a proposal.

## Decisions

**Keys do not live in projects.** A gitignored keystore is still one mistaken
commit, archive, or upload from leaking, and any tool working in the project
can read it. The default keystore is one per network in Labcoat's data
directory:

| Platform | Path |
|---|---|
| macOS | `~/Library/Application Support/Labcoat/wallets/<network>.json` |
| Linux | `~/.local/share/labcoat/wallets/<network>.json` |

`LABCOAT_DATA_HOME` moves it. `--wallet-file`, `LABCOAT_WALLET_FILE`, and
`wallet_file` in `labcoat.toml` still select another file, so projects that
set one keep using it.

**Labcoat Network comes with a wallet.** `labcoat up` creates it and tops it
up. It belongs to the machine because the chain does: every project shares
both. `labcoat reset` keeps the key and the next `up` funds it again.

**The built-in wallet is local only.** Its key is random, because well-known
development keys end up holding real money. Its passphrase is public, so
Labcoat refuses to use it on another network, refuses to import a mnemonic
into it, and never shows the one it generated.

**Networks do not share a default keystore.** A signet key is not a mainnet
key unless someone chooses that with `--wallet-file`.

## Public networks (proposed)

Two different things are called a wallet today, and need separate names:

- an **account**: addresses and a fingerprint, safe to read and to commit;
- a **signer**: whatever authorizes that account's transactions.

A deployment should name an account. The machine decides which signer backs
it, so a teammate, CI, and a hardware device can serve the same deployment.

```text
labcoat account list
labcoat account add deployer --network signet     encrypted keystore
labcoat account add deployer --network mainnet --external <descriptor>
labcoat deploy --network signet --account deployer
```

| Network | Signer | Approval |
|---|---|---|
| Labcoat Network | built-in keystore | none |
| Signet, testnet | encrypted keystore; passphrase from the OS keychain | prompt, or `--yes` in CI |
| Mainnet | hardware or external PSBT by default; software keys opt-in | on the device, never `--yes` alone |

Order of work:

1. **Watch-only accounts.** `--signer psbt-file` still loads a keystore and
   its mnemonic to build the PSBT, so a machine cannot yet deploy without
   holding the key. This blocks real hardware and air-gapped use.
2. **Prove the reveal.** A deploy's second transaction spends a taproot
   script path. The PSBT signer declares no script-path support. Establish
   which key signs the reveal, and which devices can.
3. **OS keychain** for the keystore passphrase, instead of an environment
   variable. Accounts by name.
4. **Hardware** through an existing PSBT bridge (for example HWI) rather than
   device drivers in Labcoat.
5. **Mainnet policy**: one approval that covers the whole plan and bounds its
   fees, and an explicit switch before any mainnet signature.

Open questions: whether accounts are per user or per team, whether CI may
deploy to mainnet at all, and how upgrade auth tokens move to a multisig.
