// Stand-ins for wallet extensions on Labcoat Network, where no extension has
// a network to be on. Each answers the API its extension injects, and signs
// with the network's dev signer, so the real wallet libraries and the
// recipes run end to end against a real signer and a real node.
//
// They are not the extensions: what an extension does differently from its
// documentation only shows on a public network, with the extension itself.

type Rpc = (method: string, params: unknown[]) => Promise<unknown>

const rpc: Rpc = async (method, params) => {
  const response = await fetch("/rpc", {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({ jsonrpc: "2.0", id: 1, method, params }),
  })
  const body = (await response.json()) as { result?: unknown; error?: { message?: string } }
  if (body.error != null) throw new Error(body.error.message ?? "the dev signer failed")
  return body.result
}

async function devAccount(): Promise<{ address: string; internalKey: string }> {
  const result = (await rpc("labcoat_dev_addresses", [])) as { addresses: { address: string; internalKey: string }[] }
  const [first] = result.addresses
  if (first === undefined) throw new Error("the dev wallet has no address")
  return first
}

async function devSign(psbtBase64: string, finalize: boolean): Promise<string> {
  const result = (await rpc("labcoat_dev_sign_psbt", [psbtBase64, { finalize }])) as { psbt: string }
  return result.psbt
}

const hexToBase64 = (hex: string): string =>
  btoa(String.fromCharCode(...(hex.match(/../g) ?? []).map((pair) => parseInt(pair, 16))))
const base64ToHex = (text: string): string =>
  Array.from(atob(text), (char) => char.charCodeAt(0).toString(16).padStart(2, "0")).join("")

/** What each stand-in was asked, for the page to show. */
export const simCalls: { wallet: string; method: string; args: unknown }[] = []

export function installSimulatedWallets(which: string[], reject: boolean): void {
  const host = window as unknown as Record<string, unknown>
  if (which.includes("unisat")) {
    // docs.unisat.io wallet API: hex PSBTs, a compressed public key, EIP-1193 style errors.
    const refuse = () => Promise.reject({ code: 4001, message: "User rejected the request." })
    host["unisat"] = {
      requestAccounts: async () => [(await devAccount()).address],
      getAccounts: async () => [(await devAccount()).address],
      getPublicKey: async () => `02${(await devAccount()).internalKey}`,
      // LaserEyes calls getChain while connecting. UniSat has no regtest chain, so none is named.
      getChain: async () => ({ name: "Bitcoin Regtest", network: "regtest" }),
      getNetwork: async () => "regtest",
      // LaserEyes asks for these while connecting.
      switchChain: async () => ({ enum: "BITCOIN_REGTEST", name: "Bitcoin Regtest", network: "regtest" }),
      getBalance: async () => ({ confirmed: 0, unconfirmed: 0, total: 0 }),
      on: () => undefined,
      removeListener: () => undefined,
      signPsbt: async (psbtHex: string, options?: { autoFinalized?: boolean }) => {
        simCalls.push({ wallet: "unisat", method: "signPsbt", args: options })
        if (reject) return refuse()
        return base64ToHex(await devSign(hexToBase64(psbtHex), options?.autoFinalized !== false))
      },
    }
  }
  if (which.includes("xverse")) {
    // Sats Connect JSON-RPC (WBIP001): an envelope either way, base64 PSBTs.
    const request = async (method: string, params: unknown) => {
      simCalls.push({ wallet: "xverse", method, args: params })
      const ok = (result: unknown) => ({ jsonrpc: "2.0", id: "sim", result })
      const fail = (code: number, message: string) => ({ jsonrpc: "2.0", id: "sim", error: { code, message } })
      const whole = method === "wallet_connect" || method === "wallet_getAccount"
      if (whole || method === "getAddresses" || method === "getAccounts") {
        const { address, internalKey } = await devAccount()
        const entry = { address, publicKey: internalKey, addressType: "p2tr", walletType: "software" }
        const addresses = [
          { ...entry, purpose: "ordinals" },
          { ...entry, purpose: "payment" },
        ]
        if (!whole) return ok(method === "getAddresses" ? { addresses } : addresses)
        return ok({
          id: "sim",
          walletType: "software",
          addresses,
          network: { bitcoin: { name: "Regtest" }, stacks: { name: "Mainnet" }, spark: { name: "Mainnet" } },
        })
      }
      if (method === "signPsbt") {
        if (reject) return fail(-32000, "User rejected the request.")
        const { psbt } = params as { psbt: string }
        // Xverse hands the PSBT back signed and not finalized.
        return ok({ psbt: await devSign(psbt, false) })
      }
      if (method === "wallet_getNetwork") return ok({ bitcoin: { name: "Regtest" } })
      return fail(-32601, `${method} is not simulated`)
    }
    host["XverseProviders"] = { BitcoinProvider: { request, addListener: () => () => undefined } }
    host["btc_providers"] = [
      {
        id: "XverseProviders.BitcoinProvider",
        name: "Xverse Wallet (simulated)",
        icon: "data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg'/%3E",
        webUrl: "https://www.xverse.app",
        methods: ["wallet_connect", "signPsbt"],
      },
    ]
  }
}
