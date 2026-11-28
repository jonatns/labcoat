// The wallet check page: every way the Web apps page describes of signing a
// write, each behind its own buttons, against whatever wallets this browser
// has. scripts/tests/generate-web-wallet-check.sh sets it up and serves it.
//
// Each path connects, sends `increment` on the counter, and waits with
// `onStatus`; "Reject" expects the person to decline in the wallet. Results
// collect at the bottom, and in `window.__walletCheck` for a script.

import { useCallback, useEffect, useState } from "react"
import { createRoot } from "react-dom/client"
import { LaserEyesProvider, useLaserEyes } from "@omnisat/lasereyes"

import { WalletRejectedError, createClient } from "./generated/client"
import type { LabcoatWallet } from "./generated/client"
import { ABIS } from "./generated/abi"
import type { AbiDocument } from "./generated/abi"
import { useLabcoatWallet } from "./recipes/lasereyes"
import { satsConnectWallet } from "./recipes/sats-connect"
import { installSimulatedWallets, simCalls } from "./sim"

const query = new URLSearchParams(location.search)
installSimulatedWallets((query.get("sim") ?? "").split(",").filter(Boolean), query.get("reject") === "1")

const labcoat = createClient({ rpc: "/rpc" })
const abi = Object.values(ABIS as Record<string, AbiDocument>)[0] as AbiDocument
const counter = labcoat.contract(import.meta.env["VITE_CONTRACT"] as string, abi)
const feeRate = Number(query.get("feeRate") ?? import.meta.env["VITE_FEE_RATE"] ?? 2)

async function mine(): Promise<void> {
  await fetch("/rpc", {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({
      jsonrpc: "2.0",
      id: 1,
      method: "generatetoaddress",
      params: [1, "bcrt1qw508d6qejxtdg4y5r3zarvary0c5xw7kygt080"],
    }),
  })
}

interface Result {
  path: string
  step: string
  outcome: "ok" | "rejected" | "failed"
  detail: string
}
const results: Result[] = []
;(window as unknown as { __walletCheck: unknown }).__walletCheck = { results, simCalls }

const text = (value: unknown): string =>
  JSON.stringify(value, (_, item: unknown) => (typeof item === "bigint" ? `${item}` : item))

function describe(error: unknown): string {
  if (error instanceof Error) return `${error.constructor.name}: ${error.message}`
  return text(error)
}

/** One way of signing: its wallet, and the three things to try with it. */
function Path(props: { name: string; wallet: () => Promise<LabcoatWallet> | LabcoatWallet; note?: string }) {
  const [lines, setLines] = useState<string[]>([])
  const run = useCallback(
    async (step: string, expectRejection: boolean, body: (log: (line: string) => void) => Promise<string>) => {
      const log = (line: string) => setLines((before) => [...before, `${new Date().toLocaleTimeString()} ${line}`])
      const record = (outcome: Result["outcome"], detail: string) => {
        results.push({ path: props.name, step, outcome, detail })
        log(`${step}: ${outcome.toUpperCase()} ${detail}`)
      }
      try {
        const detail = await body(log)
        record(expectRejection ? "failed" : "ok", expectRejection ? `expected a rejection, got ${detail}` : detail)
      } catch (error) {
        if (error instanceof WalletRejectedError) record(expectRejection ? "ok" : "rejected", describe(error))
        else record("failed", describe(error))
      }
    },
    [props.name],
  )
  const accounts = () =>
    run("accounts", false, async () => text(await (await props.wallet()).getAccounts()))
  const send = (step: string, expectRejection: boolean) =>
    run(step, expectRejection, async (log) => {
      const wallet = await props.wallet()
      const before = await counter.read["getCount"]?.()
      const tx = await counter.write["increment"]?.({ wallet, feeRate })
      if (tx === undefined) throw new Error("the counter has no increment method")
      log(`broadcast ${tx.txid}, fee ${tx.fee} sats`)
      await tx.wait({ onStatus: (status) => log(`status ${text(status)}`) })
      return `txid ${tx.txid}, count ${String(before)} -> ${String(await counter.read["getCount"]?.())}`
    })
  const twoHalves = () =>
    run("prepare + broadcast", false, async (log) => {
      const wallet = await props.wallet()
      const prepared = await counter.prepare["increment"]?.({ accounts: await wallet.getAccounts(), feeRate })
      if (prepared === undefined) throw new Error("the counter has no increment method")
      log(`prepared, fee ${prepared.fee} sats, ${prepared.inputs.length} input(s)`)
      const signed = await wallet.signPsbt({ psbt: prepared.psbt, inputs: prepared.inputs, summary: prepared.summary })
      const tx = await labcoat.broadcast(prepared, signed, { wallet })
      await tx.wait({ onStatus: (status) => log(`status ${text(status)}`) })
      return `txid ${tx.txid}, fee ${tx.fee} sats`
    })
  return (
    <section data-path={props.name} style={{ borderTop: "1px solid #8884", padding: "0.75rem 0" }}>
      <h2 style={{ fontSize: "1rem", margin: 0 }}>{props.name}</h2>
      {props.note === undefined ? null : <p style={{ margin: "0.25rem 0", opacity: 0.7 }}>{props.note}</p>}
      <p style={{ display: "flex", gap: "0.5rem", flexWrap: "wrap" }}>
        <button onClick={accounts}>Accounts</button>
        <button onClick={() => send("write", false)}>Send increment</button>
        <button onClick={() => send("reject", true)}>Send, then reject in the wallet</button>
        <button onClick={twoHalves}>Prepare, sign, broadcast</button>
      </p>
      <pre style={{ whiteSpace: "pre-wrap", overflowWrap: "anywhere", margin: 0 }}>{lines.join("\n")}</pre>
    </section>
  )
}

const LASEREYES_PROVIDERS = ["unisat", "xverse", "okx", "leather"] as const

function LaserEyesPaths() {
  const eyes = useLaserEyes()
  const wallet = useLabcoatWallet()
  const [error, setError] = useState("")
  return (
    <>
      <p style={{ display: "flex", gap: "0.5rem", flexWrap: "wrap", alignItems: "center" }}>
        LaserEyes:
        {LASEREYES_PROVIDERS.map((provider) => (
          <button
            key={provider}
            onClick={() => {
              setError("")
              eyes.connect(provider).catch((cause: unknown) => setError(describe(cause)))
            }}
          >
            Connect {provider}
          </button>
        ))}
        <button onClick={() => eyes.disconnect()}>Disconnect</button>
        <span data-lasereyes-state>
          {eyes.connected ? `connected: ${String(eyes.provider)} ${eyes.address}` : "not connected"} {error}
        </span>
      </p>
      <Path
        name="LaserEyes recipe"
        note="useLabcoatWallet() from the Web apps page, over useLaserEyes()."
        wallet={() => {
          if (wallet === null) throw new Error("connect a wallet through LaserEyes first")
          return wallet
        }}
      />
    </>
  )
}

const LASEREYES_NETWORKS: Record<string, string> = { regtest: "regtest", signet: "signet", testnet: "testnet", mainnet: "mainnet" }

/** The counter's value, read through the endpoint: proof the page reaches the contract. */
function Count() {
  const [count, setCount] = useState("…")
  useEffect(() => {
    const read = counter.read["getCount"]
    if (read === undefined) return setCount("the contract has no get_count method")
    read().then((value) => setCount(String(value)), (error: unknown) => setCount(describe(error)))
  }, [])
  return <b data-count>{count}</b>
}

function App() {
  const builtIn = labcoat.availableWallets()
  return (
    <main style={{ font: "14px/1.5 system-ui, sans-serif", maxWidth: "60rem", margin: "1rem auto", padding: "0 1rem" }}>
      <h1 style={{ fontSize: "1.25rem" }}>Labcoat wallet check</h1>
      <p>
        Network <b>{labcoat.network}</b>, dialect <b>{labcoat.dialect}</b>, contract <b>{counter.id}</b>, fee rate{" "}
        <b>{feeRate}</b> sat/vB, count <Count />. Wallets with a built-in adapter in this page: <b>{builtIn.join(", ") || "none"}</b>.
      </p>
      <LaserEyesProvider config={{ network: LASEREYES_NETWORKS[labcoat.network] ?? "mainnet" }}>
        <LaserEyesPaths />
      </LaserEyesProvider>
      <Path
        name="Sats Connect recipe"
        note="satsConnectWallet from the Web apps page, over sats-connect's Wallet.request."
        wallet={() => satsConnectWallet}
      />
      {builtIn.map((id) => (
        <Path
          key={id}
          name={`Built-in adapter: ${id}`}
          wallet={() => labcoat.connect({ wallet: id })}
        />
      ))}
      <p style={{ display: "flex", gap: "0.5rem" }}>
        {labcoat.network !== "regtest" ? null : (
          // Only the dev wallet mines after a broadcast; a write signed any other way waits for this.
          <button onClick={() => void mine()}>Mine a block</button>
        )}
        <button onClick={() => navigator.clipboard.writeText(JSON.stringify(results, null, 2))}>Copy results</button>
      </p>
    </main>
  )
}

// No StrictMode: its second mount in development disposes the LaserEyes
// client the first one made, and connecting then does nothing.
createRoot(document.getElementById("root") as HTMLElement).render(<App />)
