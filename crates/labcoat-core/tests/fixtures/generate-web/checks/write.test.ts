// Runtime checks for wallet-signed writes in the client `labcoat generate
// web` writes. Run by scripts/tests/generate-web-client-test.sh.
//
// The first test is the one that matters most: for every case in
// write-vectors.json (recorded from the Rust call path by
// crates/labcoat-core/src/web_write_vectors.rs), the transaction the client
// asks the wallet to sign must be byte-for-byte the one `labcoat call`
// builds. The rest run the client against a stub endpoint and stub wallet
// providers.

import { test } from "node:test"
import { strict as assert } from "node:assert"
import { createHash } from "node:crypto"
import { readFileSync } from "node:fs"

import {
  ContractRevertError,
  LabcoatClientError,
  WalletRejectedError,
  approvalPrompt,
  createClient,
} from "../expected/client"
import type {
  LabcoatWallet,
  LabcoatWebClientOptions,
  SignPsbtRequest,
  TransactionStatus,
  TransactionSummary,
  WriteOptions,
} from "../expected/client"
import type { AbiDocument } from "../expected/abi"
import { MANIFEST } from "../expected/manifest"
import {
  addressToScript,
  buildCallTransaction,
  extractTransaction,
  scriptHash,
  sha256,
} from "../expected/transaction"
import type { SpendableOutput } from "../expected/transaction"
import {
  browserWallet,
  installedBrowserWallets,
  leatherWallet,
  subfrostWallet,
  okxWallet,
  unisatWallet,
  xverseWallet,
} from "../expected/wallet"

/* --------------------------------- vectors --------------------------------- */

interface VectorCase {
  name: string
  utxos: { txid: string; vout: number; value: number; alkanes: { id: string; amount: string }[] }[]
  contract: string
  method: { name: string; opcode: string; params: { name: string; type: string }[]; returns: string }
  args: string[]
  feeRate: number
  alkanes: { id: string; amount: string }[]
  to: string | null
  payments: { address: string; sats: number }[]
  expected: { transaction: string; fee: number; inputs: string[]; outputs: number[] }
}

interface Vectors {
  wallet: { address: string; publicKey: string }
  cases: VectorCase[]
}

const vectorsPath = process.env["LABCOAT_WRITE_VECTORS"]
if (vectorsPath === undefined) throw new Error("LABCOAT_WRITE_VECTORS must name write-vectors.json")
const vectors = JSON.parse(readFileSync(vectorsPath, "utf8")) as Vectors

/* ------------------------------- wire helpers ------------------------------ */

function varint(value: bigint): number[] {
  const out: number[] = []
  for (let v = value; ; ) {
    const byte = Number(v & 0x7fn)
    v >>= 7n
    if (v === 0n) return [...out, byte]
    out.push(byte | 0x80)
  }
}

const varintField = (field: number, value: bigint): number[] => [...varint(BigInt(field * 8)), ...varint(value)]

const bytesField = (field: number, value: ArrayLike<number>): number[] => [
  ...varint(BigInt(field * 8 + 2)),
  ...varint(BigInt(value.length)),
  ...Array.from(value),
]

const toHex = (bytes: ArrayLike<number>): string =>
  Array.from(bytes, (byte) => byte.toString(16).padStart(2, "0")).join("")

const fromHex = (text: string): number[] =>
  ((text.startsWith("0x") ? text.slice(2) : text).match(/../g) ?? []).map((pair) => parseInt(pair, 16))

const utf8 = (text: string): number[] => Array.from(new TextEncoder().encode(text))

const uint128 = (value: bigint): number[] => [
  ...varintField(1, value & 0xffff_ffff_ffff_ffffn),
  ...varintField(2, value >> 64n),
]

const u128le = (value: bigint): number[] =>
  Array.from({ length: 16 }, (_, i) => Number((value >> BigInt(8 * i)) & 0xffn))

/** protorune.WalletResponse listing the outpoints that carry Alkanes. */
function walletResponse(utxos: VectorCase["utxos"]): string {
  const out: number[] = []
  for (const utxo of utxos) {
    if (utxo.alkanes.length === 0) continue
    const sheet: number[] = []
    for (const alkane of utxo.alkanes) {
      const [block, tx] = alkane.id.split(":").map(BigInt) as [bigint, bigint]
      const runeId = [...bytesField(1, uint128(block)), ...bytesField(2, uint128(tx))]
      const item = [...bytesField(1, bytesField(1, runeId)), ...bytesField(2, uint128(BigInt(alkane.amount)))]
      sheet.push(...bytesField(1, item))
    }
    const outpoint = [...bytesField(1, fromHex(utxo.txid).reverse()), ...varintField(2, BigInt(utxo.vout))]
    out.push(...bytesField(1, [...bytesField(1, sheet), ...bytesField(2, outpoint)]))
  }
  return `0x${toHex(out)}`
}

/** alkanes.AlkanesTrace of one call frame that returned or reverted. */
function traceResponse(outcome: { reverted: boolean; data: number[]; gas: bigint }): string {
  const exit = [
    ...varintField(1, outcome.reverted ? 1n : 0n),
    ...bytesField(2, bytesField(3, outcome.data)),
    ...varintField(3, outcome.gas),
  ]
  const events = [...bytesField(1, bytesField(1, [])), ...bytesField(1, bytesField(2, exit))]
  return `0x${toHex(events)}`
}

/** Read a Bitcoin compact size at `offset`. */
function compactSize(bytes: number[], offset: number): [value: number, next: number] {
  const first = bytes[offset] as number
  if (first < 0xfd) return [first, offset + 1]
  return [(bytes[offset + 1] as number) | ((bytes[offset + 2] as number) << 8), offset + 3]
}

interface DecodedPsbt {
  /** The unsigned transaction, hex. */
  transaction: string
  /** Each input's fields, key hex to value hex. */
  inputs: Map<string, string>[]
}

/** An independent PSBT reader: the client's own parser is what is under test. */
function decodePsbt(base64: string): DecodedPsbt {
  const bytes = Array.from(Buffer.from(base64, "base64"))
  assert.equal(toHex(bytes.slice(0, 5)), "70736274ff")
  let offset = 5
  const readMap = (): Map<string, string> => {
    const map = new Map<string, string>()
    for (;;) {
      const [keyLength, afterKeyLength] = compactSize(bytes, offset)
      offset = afterKeyLength
      if (keyLength === 0) return map
      const key = toHex(bytes.slice(offset, offset + keyLength))
      offset += keyLength
      const [valueLength, afterValueLength] = compactSize(bytes, offset)
      map.set(key, toHex(bytes.slice(afterValueLength, afterValueLength + valueLength)))
      offset = afterValueLength + valueLength
    }
  }
  const transaction = readMap().get("00") as string
  // version (4), then the input count: every case has fewer than 253 inputs.
  const inputCount = fromHex(transaction)[4] as number
  return { transaction, inputs: Array.from({ length: inputCount }, readMap) }
}

/** The signed form of an unsigned transaction: a 64-byte signature per input. */
function withWitnesses(unsignedHex: string, inputs: number): string {
  const witness = Array.from({ length: inputs }, () => `0140${"ab".repeat(64)}`).join("")
  return `${unsignedHex.slice(0, 8)}0001${unsignedHex.slice(8, -8)}${witness}${unsignedHex.slice(-8)}`
}

/* ------------------------------- stub endpoint ----------------------------- */

interface Call {
  method: string
  params: unknown[]
}

interface EndpointOptions {
  utxos?: VectorCase["utxos"]
  /** Height of both indexes; outputs confirm at 100. */
  height?: number
  simulateError?: string
  devSigner?: boolean
  /** Outpoints `gettxout` reports as spent. */
  spent?: string[]
  trace?: { reverted: boolean; data: number[]; gas: bigint } | null
  /**
   * "alkanes" makes the stub a hosted provider of the standard Alkanes
   * JSON-RPC, as Subfrost's public endpoint answered on 2026-11-27: it
   * refuses the secondary views and the dev signer, answers a batch with a
   * parse error, and returns Esplora results as JSON.
   */
  dialect?: "labcoat" | "alkanes"
  /** Block heights of `utxos`, by position. Defaults to 100 for each. */
  heights?: number[]
}

const WALLET_SCRIPT = addressToScript(vectors.wallet.address, "regtest")
const BROADCAST_TXID = "5e".repeat(32)

function stubEndpoint(options: EndpointOptions = {}) {
  const calls: Call[] = []
  const utxos = options.utxos ?? [{ txid: "c1".repeat(32), vout: 0, value: 100_000, alkanes: [] }]
  const state = {
    trace: options.trace ?? null,
    traceVouts: [] as number[],
    broadcast: [] as string[],
    posts: 0,
    /** The block the broadcast transaction is in, once it is in one. */
    confirmedAt: null as number | null,
  }
  const txStatus = () =>
    state.confirmedAt === null ? { confirmed: false } : { confirmed: true, block_height: state.confirmedAt }

  const answer = (request: Call & { id: number }): unknown => {
    calls.push({ method: request.method, params: request.params })
    const reply = (result: unknown) => ({ jsonrpc: "2.0", id: request.id, result })
    const fail = (code: number, message: string) => ({ jsonrpc: "2.0", id: request.id, error: { code, message } })
    const [first, second, third] = request.params
    const hosted = options.dialect === "alkanes"
    const refused = () => fail(-32601, `method not permitted on the public endpoint: ${request.method}`)
    const listed = () =>
      utxos.map(({ txid, vout, value }, index) => ({
        txid,
        vout,
        value,
        status: { confirmed: true, block_height: options.heights?.[index] ?? 100 },
      }))
    const view = (name: unknown, input: unknown) => {
      const payload = fromHex(input as string)
      if (name === "protorunesbyaddress") return reply(walletResponse(utxos))
      if (name === "simulate") {
        const error = options.simulateError
        return reply(
          `0x${toHex([...bytesField(1, bytesField(3, [])), ...(error === undefined ? [] : bytesField(3, utf8(error)))])}`,
        )
      }
      if (name === "trace") {
        // alkanes.Outpoint { txid = 1, vout = 2 }: the vout follows the 34-byte txid field.
        state.traceVouts.push(payload[35] as number)
        return reply(state.trace === null ? "0x" : traceResponse(state.trace))
      }
      return fail(-1, `unknown view ${String(name)}`)
    }
    switch (request.method) {
      case "secondaryheight":
        if (hosted) return refused()
        return reply(options.height ?? 300)
      case "secondaryview": {
        if (hosted) return refused()
        const payload = fromHex(third as string)
        if (first === "esplora" && second === "utxosbyscripthash") {
          const expected = createHash("sha256").update(WALLET_SCRIPT).digest().reverse().toString("hex")
          return reply(
            `0x${toHex(utf8(JSON.stringify(Buffer.from(payload).toString() === expected ? listed() : [])))}`,
          )
        }
        if (first === "esplora" && second === "txhex") {
          return reply(`0x${toHex(utf8(state.broadcast[0] ?? "tx not found"))}`)
        }
        if (first === "esplora" && second === "txstatus") {
          return reply(`0x${toHex(utf8(JSON.stringify(txStatus())))}`)
        }
        return view(second, third)
      }
      // The standard Alkanes JSON-RPC: the height is a string, a view names
      // its block tag, and Esplora answers in JSON.
      case "metashrew_height":
        if (!hosted) return fail(-32601, "Method not found")
        return reply(String(options.height ?? 300))
      case "metashrew_view":
        if (!hosted) return fail(-32601, "Method not found")
        if (third !== "latest") return fail(-32602, `unexpected block tag ${String(third)}`)
        return view(first, second)
      case "esplora_address::utxo":
        if (!hosted) return fail(-32601, "Method not found")
        // Esplora promises no order: list newest first.
        return reply(first === vectors.wallet.address ? listed().reverse() : [])
      case "esplora_tx::hex":
        if (!hosted) return fail(-32601, "Method not found")
        return reply(state.broadcast[0] ?? "Transaction not found")
      case "esplora_tx::status":
        if (!hosted) return fail(-32601, "Method not found")
        return reply(txStatus())
      case "gettxout":
        return reply(
          (options.spent ?? []).includes(`${String(first)}:${String(second)}`)
            ? null
            : { confirmations: 201, coinbase: false },
        )
      case "labcoat_dev_addresses":
        if (hosted) return refused()
        if (options.devSigner === false) return fail(-32601, "Method not found")
        return reply({
          network: "regtest",
          addresses: [{ address: vectors.wallet.address, type: "p2tr", internalKey: vectors.wallet.publicKey }],
        })
      case "labcoat_dev_sign_psbt": {
        if (hosted) return refused()
        const decoded = decodePsbt(first as string)
        return reply({ complete: true, hex: withWitnesses(decoded.transaction, decoded.inputs.length) })
      }
      case "sendrawtransaction":
        state.broadcast.push(first as string)
        return reply(BROADCAST_TXID)
      case "generatetoaddress":
        if (hosted) return refused()
        return reply(["00".repeat(32)])
      default:
        return fail(-32601, "Method not found")
    }
  }

  const fetchFn = (async (_url: unknown, init?: { body?: unknown }) => {
    const body = JSON.parse(String(init?.body)) as (Call & { id: number }) | (Call & { id: number })[]
    state.posts += 1
    let result: unknown
    if (!Array.isArray(body)) {
      result = answer(body)
    } else if (options.dialect === "alkanes") {
      result = {
        jsonrpc: "2.0",
        id: null,
        error: { code: -32700, message: "Parse error: invalid type: map, expected a string at line 1 column 1" },
      }
    } else {
      result = body.map(answer)
    }
    return new Response(JSON.stringify(result), { headers: { "content-type": "application/json" } })
  }) as typeof fetch

  const client = (extra: LabcoatWebClientOptions = {}) =>
    createClient({
      rpc: "http://stub.invalid/rpc",
      fetchFn,
      ...(options.dialect === undefined ? {} : { dialect: options.dialect }),
      ...extra,
    })
  const called = (method: string): Call[] => calls.filter((call) => call.method === method)
  return { calls, called, client, state, fetchFn }
}

/** A wallet that signs whatever it is asked to, as a browser wallet would after approval. */
function signingWallet() {
  const requests: SignPsbtRequest[] = []
  const wallet: LabcoatWallet = {
    name: "Signer",
    getAccounts: async () => [{ address: vectors.wallet.address, publicKey: vectors.wallet.publicKey }],
    signPsbt: async (request) => {
      requests.push(request)
      const decoded = decodePsbt(request.psbt)
      return { hex: withWitnesses(decoded.transaction, decoded.inputs.length) }
    },
  }
  return { wallet, requests }
}

/** A wallet that records what it is asked to sign and then declines. */
function recordingWallet() {
  const requests: SignPsbtRequest[] = []
  const wallet: LabcoatWallet = {
    name: "Recorder",
    getAccounts: async () => [{ address: vectors.wallet.address, publicKey: vectors.wallet.publicKey }],
    signPsbt: async (request) => {
      requests.push(request)
      throw new WalletRejectedError("Recorder")
    },
  }
  return { wallet, requests }
}

/** Run `body` with the generated manifest naming another Bitcoin network. */
async function onNetwork(network: string, body: () => Promise<void>): Promise<void> {
  const manifest = MANIFEST as { bitcoinNetwork: string }
  const before = manifest.bitcoinNetwork
  manifest.bitcoinNetwork = network
  try {
    await body()
  } finally {
    manifest.bitcoinNetwork = before
  }
}

/** An unsigned transaction (hex) with `sats` taken from, or added to, one output's value. */
function withOutputValue(unsignedHex: string, index: number, change: bigint): string {
  const bytes = fromHex(unsignedHex)
  // version, input count, then 41 bytes per unsigned input, then the output count.
  let offset = 4 + 1 + (bytes[4] as number) * 41 + 1
  for (let output = 0; output < index; output++) offset += 8 + 1 + (bytes[offset + 8] as number)
  let value = 0n
  for (let i = 7; i >= 0; i--) value = (value << 8n) | BigInt(bytes[offset + i] as number)
  value += change
  for (let i = 0; i < 8; i++) bytes[offset + i] = Number((value >> BigInt(8 * i)) & 0xffn)
  return toHex(bytes)
}

const abiCounter = {
  contract: "Counter",
  methods: [
    { name: "increment", opcode: 1n, params: [], returns: "void" },
    { name: "add", opcode: 2n, params: [{ name: "amount", type: "u128" }], returns: "u128" },
    { name: "get_count", opcode: 3n, params: [], returns: "u128" },
  ],
} as const satisfies AbiDocument

const camel = (name: string): string => name.replace(/_(.)/g, (_, letter: string) => letter.toUpperCase())

/* ------------------------ byte-for-byte against Rust ----------------------- */

for (const vector of vectors.cases) {
  test(`builds what labcoat call builds: ${vector.name}`, async () => {
    const endpoint = stubEndpoint({ utxos: vector.utxos })
    const { wallet, requests } = recordingWallet()
    const abi: AbiDocument = {
      contract: "Vector",
      methods: [{ ...vector.method, opcode: BigInt(vector.method.opcode) }],
    }
    const options: WriteOptions = {
      wallet,
      feeRate: vector.feeRate,
      alkanes: vector.alkanes,
      pay: vector.payments,
      ...(vector.to === null ? {} : { to: vector.to }),
    }
    const write = endpoint.client().contract(vector.contract, abi).write[camel(vector.method.name)]
    assert.ok(write)
    await assert.rejects(write(...vector.args, options), WalletRejectedError)

    const [request] = requests
    assert.ok(request)
    const psbt = decodePsbt(request.psbt)
    assert.equal(psbt.transaction, vector.expected.transaction)
    assert.equal(request.summary.fee, BigInt(vector.expected.fee))
    assert.deepEqual(
      request.inputs,
      vector.expected.inputs.map((_, index) => ({ index, address: vectors.wallet.address })),
    )
    // Every input carries what a taproot signer needs.
    const spent = new Map(vector.utxos.map((utxo) => [`${utxo.txid}:${utxo.vout}`, utxo.value]))
    psbt.inputs.forEach((fields, index) => {
      const value = BigInt(spent.get(vector.expected.inputs[index] as string) as number)
      const amount = toHex(Array.from({ length: 8 }, (_, i) => Number((value >> BigInt(8 * i)) & 0xffn)))
      assert.equal(fields.get("01"), `${amount}22${toHex(WALLET_SCRIPT)}`)
      assert.equal(fields.get("17"), vectors.wallet.publicKey)
    })
    assert.equal(endpoint.called("sendrawtransaction").length, 0)
  })
}

/* ---------------------------------- builder -------------------------------- */

test("sha256 and the Esplora script hash match the platform's", () => {
  for (const length of [0, 1, 55, 56, 63, 64, 65, 119, 120, 1000]) {
    const data = Uint8Array.from({ length }, (_, i) => (i * 31 + length) & 0xff)
    assert.equal(toHex(sha256(data)), createHash("sha256").update(data).digest("hex"))
  }
  assert.equal(
    scriptHash(WALLET_SCRIPT),
    createHash("sha256").update(WALLET_SCRIPT).digest().reverse().toString("hex"),
  )
})

test("an address of another network, or a damaged one, is refused", () => {
  const invalid = (address: string, network: "mainnet" | "signet" | "regtest", pattern: RegExp) =>
    assert.throws(
      () => addressToScript(address, network),
      (error: unknown) => error instanceof LabcoatClientError && error.code === "ARG_INVALID" && pattern.test(error.message),
    )
  invalid(vectors.wallet.address, "mainnet", /does not belong to the mainnet network/)
  invalid("bc1qw508d6qejxtdg4y5r3zarvary0c5xw7kv8f3t4", "regtest", /does not belong/)
  invalid("1BvBMSEYstWetqTFn5Au4m4GFg7xJaNVN2", "signet", /does not belong/)
  invalid(`${vectors.wallet.address.slice(0, -1)}q`, "regtest", /bad checksum/)
  invalid("mipcBbFg9gMiCh81Kj8tqqdgoZub1ZJRfm", "regtest", /bad checksum/)
  invalid("not an address", "regtest", /not a bitcoin address/)
  assert.equal(
    toHex(addressToScript("BC1QW508D6QEJXTDG4Y5R3ZARVARY0C5XW7KV8F3T4", "mainnet")),
    "0014751e76e8199196d454941c45d1b3a323f1433bd6",
  )
  assert.equal(
    toHex(addressToScript("1BvBMSEYstWetqTFn5Au4m4GFg7xJaNVN2", "mainnet")),
    "76a91477bff20c60e522dfaa3350c39b030a5d004e839a88ac",
  )
})

function output(index: number, value: bigint, alkanes: SpendableOutput["alkanes"] = null): SpendableOutput {
  return {
    txid: index.toString(16).padStart(64, "0"),
    vout: 0,
    value,
    script: WALLET_SCRIPT,
    internalKey: null,
    alkanes,
  }
}

const baseRequest = {
  cellpack: [2n, 1n, 1n],
  recipient: WALLET_SCRIPT,
  change: WALLET_SCRIPT,
  alkanes: [],
  payments: [],
  feeRate: 2,
}

test("the builder refuses what it cannot build safely", () => {
  const code = (expected: string) => (error: unknown) =>
    error instanceof LabcoatClientError && error.code === expected
  // Not enough bitcoin, and an output carrying Alkanes is not bitcoin.
  assert.throws(() => buildCallTransaction({ ...baseRequest, utxos: [output(1, 900n)] }), code("INSUFFICIENT_FUNDS"))
  assert.throws(
    () =>
      buildCallTransaction({
        ...baseRequest,
        utxos: [output(1, 500_000n, [{ block: 2n, tx: 5n, amount: 1n }])],
      }),
    code("INSUFFICIENT_FUNDS"),
  )
  // Listed by the index with nothing on it: still not spent.
  assert.throws(
    () => buildCallTransaction({ ...baseRequest, utxos: [output(1, 500_000n, [])] }),
    code("INSUFFICIENT_FUNDS"),
  )
  // Fewer Alkanes than the call sends.
  assert.throws(
    () =>
      buildCallTransaction({
        ...baseRequest,
        alkanes: [{ block: 2n, tx: 5n, amount: 11n }],
        utxos: [output(1, 546n, [{ block: 2n, tx: 5n, amount: 10n }]), output(2, 50_000n)],
      }),
    code("INSUFFICIENT_FUNDS"),
  )
  // The output holding the Alkane to send also holds one that is not sent.
  assert.throws(
    () =>
      buildCallTransaction({
        ...baseRequest,
        alkanes: [{ block: 2n, tx: 5n, amount: 10n }],
        utxos: [
          output(1, 546n, [
            { block: 2n, tx: 5n, amount: 10n },
            { block: 2n, tx: 6n, amount: 4n },
          ]),
          output(2, 50_000n),
        ],
      }),
    code("TRANSACTION_INVALID"),
  )
  assert.throws(
    () => buildCallTransaction({ ...baseRequest, utxos: [output(1, 50_000n)], feeRate: 0 }),
    code("ARG_INVALID"),
  )
  assert.throws(
    () =>
      buildCallTransaction({
        ...baseRequest,
        utxos: [output(1, 50_000n)],
        payments: [{ script: WALLET_SCRIPT, sats: 545n }],
      }),
    code("ARG_INVALID"),
  )
})

test("without a change output, Alkanes sent in are still addressed to the call", () => {
  // 700 sats beside the carrier: enough for the fee, with change below dust.
  const carrier = output(1, 546n, [{ block: 2n, tx: 5n, amount: 10n }])
  const request = { ...baseRequest, feeRate: 1, alkanes: [{ block: 2n, tx: 5n, amount: 10n }] }
  const dropped = buildCallTransaction({ ...request, utxos: [carrier, output(2, 700n)] })
  const kept = buildCallTransaction({ ...request, utxos: [carrier, output(2, 50_000n)] })
  assert.equal(dropped.changeOutput, null)
  assert.equal(dropped.outputs.length, 2)
  assert.equal(kept.outputs.length, 3)
  // The call is the second protostone: one past `outputs + 1`.
  assert.equal(dropped.traceVout, 4)
  assert.equal(kept.traceVout, 5)
  // The forwarding edict names the call's virtual output, so the two
  // protostones differ exactly where that number is encoded.
  const script = (built: typeof kept) => toHex(built.outputs[built.outputs.length - 1]?.script ?? [])
  assert.notEqual(script(dropped), script(kept))
  assert.equal(script(dropped).length, script(kept).length)
})

/* ---------------------------------- writes --------------------------------- */

test("a write is simulated, signed by the dev wallet, broadcast, mined, and waited for", async () => {
  const endpoint = stubEndpoint()
  const labcoat = endpoint.client()
  const wallet = await labcoat.connect()
  assert.equal(wallet.name, "Labcoat dev wallet")

  const tx = await labcoat.contract("2:1", abiCounter).write.add(5n, { wallet })
  assert.equal(tx.txid, BROADCAST_TXID)
  assert.ok(tx.fee > 0n)
  const order = endpoint.calls.map((call) => (call.method === "secondaryview" ? String(call.params[1]) : call.method))
  const at = (name: string) => order.indexOf(name)
  assert.ok(at("simulate") < at("labcoat_dev_sign_psbt"))
  assert.ok(at("labcoat_dev_sign_psbt") < at("sendrawtransaction"))
  assert.ok(at("sendrawtransaction") < at("generatetoaddress"))
  assert.deepEqual(endpoint.called("generatetoaddress")[0]?.params, [
    1,
    "bcrt1qw508d6qejxtdg4y5r3zarvary0c5xw7kygt080",
  ])
  // The mining address is the one the CLI mines to: P2WPKH of the generator point.
  assert.equal(
    toHex(addressToScript("bcrt1qw508d6qejxtdg4y5r3zarvary0c5xw7kygt080", "regtest")),
    "0014751e76e8199196d454941c45d1b3a323f1433bd6",
  )

  endpoint.state.trace = { reverted: false, data: u128le(12n), gas: 4_321n }
  const receipt = await tx.wait({ pollMs: 1 })
  assert.deepEqual(
    { ...receipt, data: undefined },
    { txid: BROADCAST_TXID, status: "success", value: 12n, data: undefined, gasUsed: 4_321n },
  )
  // Recipient, change, OP_RETURN: the call is traced at the fourth output.
  assert.deepEqual(endpoint.state.traceVouts, [4])

  // By transaction id alone, the return value stays bytes.
  const bare = await labcoat.waitForTransaction(tx.txid, { pollMs: 1 })
  assert.deepEqual(Array.from(bare.value), u128le(12n))
})

test("a transaction that reverted on chain throws the revert reads throw", async () => {
  const endpoint = stubEndpoint()
  const labcoat = endpoint.client()
  const tx = await labcoat.contract("2:1", abiCounter).write.increment({ wallet: await labcoat.connect() })
  endpoint.state.trace = {
    reverted: true,
    data: [0x08, 0xc3, 0x79, 0xa0, ...utf8("ALKANES: revert: counter is full")],
    gas: 99n,
  }
  await assert.rejects(tx.wait({ pollMs: 1 }), (error: unknown) => {
    assert.ok(error instanceof ContractRevertError)
    assert.equal(error.reason, "counter is full")
    assert.equal(error.contract, "2:1")
    assert.equal(error.method, "increment")
    assert.equal(error.txid, BROADCAST_TXID)
    assert.equal(error.gasUsed, 99n)
    return true
  })
  await assert.rejects(labcoat.waitForTransaction(tx.txid, { pollMs: 1 }), ContractRevertError)
})

test("waiting gives up when the index never has the transaction", async () => {
  const endpoint = stubEndpoint()
  const labcoat = endpoint.client()
  const tx = await labcoat.contract("2:1", abiCounter).write.increment({ wallet: await labcoat.connect() })
  await assert.rejects(
    tx.wait({ timeoutMs: 5, pollMs: 1 }),
    (error: unknown) => error instanceof LabcoatClientError && error.code === "TRANSACTION_TIMEOUT",
  )
})

test("a call that would revert never reaches the wallet, unless simulation is skipped", async () => {
  const endpoint = stubEndpoint({ simulateError: "ALKANES: revert: not the owner" })
  const { wallet, requests } = recordingWallet()
  const counter = endpoint.client().contract("2:1", abiCounter)
  await assert.rejects(counter.write.increment({ wallet }), (error: unknown) => {
    assert.ok(error instanceof ContractRevertError)
    assert.equal(error.reason, "not the owner")
    assert.equal(error.txid, null)
    return true
  })
  assert.equal(requests.length, 0)

  await assert.rejects(counter.write.increment({ wallet, simulate: false }), WalletRejectedError)
  assert.equal(requests.length, 1)
})

test("a rejection in the wallet is a WalletRejectedError and nothing is broadcast", async () => {
  const endpoint = stubEndpoint()
  const labcoat = endpoint.client()
  const summaries: unknown[] = []
  const wallet = await labcoat.connect({
    approve: (summary) => {
      summaries.push(summary)
      return false
    },
  })
  const payee = "bcrt1qw508d6qejxtdg4y5r3zarvary0c5xw7kygt080"
  await assert.rejects(
    labcoat.contract("2:1", abiCounter).write.add(1n, { wallet, pay: [{ address: payee, sats: 2_000 }] }),
    (error: unknown) =>
      error instanceof WalletRejectedError && error.code === "WALLET_REJECTED" && error.wallet === "Labcoat dev wallet",
  )
  assert.equal(endpoint.called("labcoat_dev_sign_psbt").length, 0)
  assert.equal(endpoint.called("sendrawtransaction").length, 0)
  assert.deepEqual(summaries, [
    {
      network: "regtest",
      contract: "2:1",
      method: "add",
      fee: 433n,
      recipient: vectors.wallet.address,
      alkanes: [],
      payments: [{ address: payee, sats: 2_000n }],
    },
  ])

  // An approval lets it through.
  const approving = await labcoat.connect({ approve: () => true })
  const tx = await labcoat.contract("2:1", abiCounter).write.add(1n, { wallet: approving })
  assert.equal(tx.txid, BROADCAST_TXID)
})

test("a call through a manifest name carries the name in its summary", async () => {
  const labcoat = stubEndpoint().client()
  const summaries: TransactionSummary[] = []
  const wallet = await labcoat.connect({
    approve: (summary) => {
      summaries.push(summary)
      return false
    },
  })
  await assert.rejects(labcoat.contract("fire").write.forward({ wallet }), WalletRejectedError)
  // Addressed by the same id, the contract has no name to show.
  await assert.rejects(labcoat.contract("4:65011", abiCounter).write.increment({ wallet }), WalletRejectedError)
  assert.deepEqual(
    summaries.map(({ contract, contractName, method }) => ({ contract, contractName, method })),
    [
      { contract: "4:65011", contractName: "fire", method: "forward" },
      { contract: "4:65011", contractName: undefined, method: "increment" },
    ],
  )
  assert.equal("contractName" in (summaries[1] as TransactionSummary), false)
})

test("the approval prompt shows a named contract as name (id), and an unnamed one as its id", async () => {
  // Just enough of a DOM for the prompt: elements that record their text
  // and children, and a dialog that closes when a button is clicked.
  interface FakeElement {
    tag: string
    textContent: string
    style: Record<string, string>
    children: FakeElement[]
    returnValue: string
    listeners: Record<string, () => void>
    [member: string]: unknown
  }
  const element = (tag: string): FakeElement => {
    const self: FakeElement = {
      tag,
      textContent: "",
      style: {},
      children: [],
      returnValue: "",
      listeners: {},
      setAttribute: () => undefined,
      append: (...children: FakeElement[]) => void self.children.push(...children),
      addEventListener: (event: string, listener: () => void) => void (self.listeners[event] = listener),
      close: (value: string) => {
        self.returnValue = value
        self.listeners["close"]?.()
      },
      remove: () => undefined,
      showModal: () => undefined,
      focus: () => undefined,
    }
    return self
  }
  const body = element("body")
  const scope = globalThis as { document?: unknown }
  scope.document = { createElement: element, body }
  try {
    const shown = async (summary: TransactionSummary, answer: "Approve" | "Reject") => {
      const answered = approvalPrompt()(summary)
      const dialog = body.children.at(-1) as FakeElement
      const [, , list, actions] = dialog.children as [FakeElement, FakeElement, FakeElement, FakeElement]
      const rows = new Map<string, string>()
      for (let i = 0; i < list.children.length; i += 2) {
        rows.set((list.children[i] as FakeElement).textContent, (list.children[i + 1] as FakeElement).textContent)
      }
      ;(actions.children.find((button) => button.textContent === answer) as FakeElement).listeners["click"]?.()
      return { rows, approved: await answered }
    }
    const named = await shown({ ...signing.summary, contract: "4:43793", contractName: "factory" }, "Approve")
    assert.equal(named.rows.get("Contract"), "factory (4:43793)")
    assert.equal(named.approved, true)
    const byId = await shown({ ...signing.summary, contract: "4:43793" }, "Reject")
    assert.equal(byId.rows.get("Contract"), "4:43793")
    assert.equal(byId.approved, false)
  } finally {
    delete scope.document
  }
})

test("the dev wallet does not mine when told not to", async () => {
  const endpoint = stubEndpoint()
  const labcoat = endpoint.client()
  const wallet = await labcoat.connect({ mine: false })
  await labcoat.contract("2:1", abiCounter).write.increment({ wallet })
  assert.equal(endpoint.called("sendrawtransaction").length, 1)
  assert.equal(endpoint.called("generatetoaddress").length, 0)
})

test("a wallet that returns another transaction is refused before broadcast", async () => {
  const endpoint = stubEndpoint()
  const other = buildCallTransaction({ ...baseRequest, utxos: [output(9, 80_000n)] })
  const wallet: LabcoatWallet = {
    name: "Swapper",
    getAccounts: async () => [{ address: vectors.wallet.address, publicKey: vectors.wallet.publicKey }],
    signPsbt: async () => ({ psbt: other.psbt }),
  }
  await assert.rejects(
    endpoint.client().contract("2:1", abiCounter).write.increment({ wallet }),
    (error: unknown) => error instanceof LabcoatClientError && error.code === "WALLET_ERROR",
  )
  assert.equal(endpoint.called("sendrawtransaction").length, 0)
})

test("an output spent in the mempool is skipped and selection runs again", async () => {
  const utxos = [
    { txid: "d1".repeat(32), vout: 0, value: 40_000, alkanes: [] },
    { txid: "d2".repeat(32), vout: 1, value: 60_000, alkanes: [] },
  ]
  const endpoint = stubEndpoint({ utxos, spent: [`${"d1".repeat(32)}:0`] })
  const { wallet, requests } = recordingWallet()
  await assert.rejects(endpoint.client().contract("2:1", abiCounter).write.increment({ wallet }), WalletRejectedError)
  const transaction = decodePsbt(requests[0]?.psbt ?? "").transaction
  assert.ok(transaction.includes(toHex(fromHex("d2".repeat(32)).reverse())))
  assert.ok(!transaction.includes("d1".repeat(32)))
})

test("a write needs a wallet, and takes the client's when none is passed", async () => {
  const endpoint = stubEndpoint()
  const counter = endpoint.client().contract("2:1", abiCounter)
  await assert.rejects(
    counter.write.increment(),
    (error: unknown) => error instanceof LabcoatClientError && error.code === "WALLET_UNAVAILABLE",
  )

  const { wallet, requests } = recordingWallet()
  const withWallet = endpoint.client({ wallet, feeRate: 5 }).contract("2:1", abiCounter)
  await assert.rejects((withWallet.write.add as () => Promise<unknown>)(), /expects 1 parameter/)
  await assert.rejects(withWallet.write.add(3n), WalletRejectedError)
  await assert.rejects(withWallet.write.add(3n, { feeRate: 2 }), WalletRejectedError)
  assert.ok((requests[0]?.summary.fee ?? 0n) > (requests[1]?.summary.fee ?? 0n))
})

/* ----------------------- the standard Alkanes JSON-RPC --------------------- */

test("a write through a hosted Alkanes endpoint uses only the methods it serves", async () => {
  const endpoint = stubEndpoint({ dialect: "alkanes" })
  const labcoat = endpoint.client()
  assert.equal(labcoat.dialect, "alkanes")
  const { wallet } = signingWallet()

  const tx = await labcoat.contract("2:1", abiCounter).write.add(5n, { wallet })
  assert.equal(tx.txid, BROADCAST_TXID)
  endpoint.state.trace = { reverted: false, data: u128le(12n), gas: 4_321n }
  const receipt = await tx.wait({ pollMs: 1 })
  assert.equal(receipt.value, 12n)
  assert.deepEqual(endpoint.state.traceVouts, [4])
  const bare = await labcoat.waitForTransaction(tx.txid, { pollMs: 1 })
  assert.deepEqual(Array.from(bare.value), u128le(12n))

  const methods = new Set(endpoint.calls.map((call) => call.method))
  assert.deepEqual(
    [...methods].sort(),
    [
      "esplora_address::utxo",
      "esplora_tx::hex",
      "gettxout",
      "metashrew_height",
      "metashrew_view",
      "sendrawtransaction",
    ],
  )
  // Nothing was batched, so nothing was refused: one call per request.
  assert.equal(endpoint.state.posts, endpoint.calls.length)
  assert.deepEqual(endpoint.called("esplora_address::utxo")[0]?.params, [vectors.wallet.address])
  assert.ok(endpoint.called("metashrew_view").every((call) => call.params[2] === "latest"))
})

test("both dialects build the same transaction from the same outputs", async () => {
  const utxos = [
    { txid: "d1".repeat(32), vout: 0, value: 600, alkanes: [] },
    { txid: "d2".repeat(32), vout: 1, value: 700, alkanes: [] },
    { txid: "d3".repeat(32), vout: 2, value: 60_000, alkanes: [] },
  ]
  const built = async (dialect: "labcoat" | "alkanes", heights: number[]) => {
    const endpoint = stubEndpoint({ utxos, dialect, heights })
    const { wallet, requests } = recordingWallet()
    await assert.rejects(endpoint.client().contract("2:1", abiCounter).write.add(5n, { wallet }), WalletRejectedError)
    return requests[0]?.psbt
  }
  const local = await built("labcoat", [100, 101, 102])
  assert.ok(local !== undefined)
  // The hosted stub lists newest first; the client puts the oldest first again.
  assert.equal(await built("alkanes", [100, 101, 102]), local)
  // Same block: transaction id, then output index, decides.
  assert.equal(await built("alkanes", [100, 100, 100]), local)
})

test("the dialect a hosted endpoint refuses is reported as the endpoint reports it", async () => {
  const endpoint = stubEndpoint({ dialect: "alkanes" })
  const { wallet } = recordingWallet()
  const counter = endpoint.client({ dialect: "labcoat", batch: false }).contract("2:1", abiCounter)
  await assert.rejects(counter.write.increment({ wallet }), (error: unknown) => {
    assert.ok(error instanceof LabcoatClientError)
    assert.equal(error.code, "RPC_ERROR")
    assert.equal(error.rpcCode, -32601)
    assert.match(error.message, /method not permitted on the public endpoint: secondary/)
    return true
  })
})

test("a hosted endpoint has no dev wallet and is never asked for one", async () => {
  const endpoint = stubEndpoint({ dialect: "alkanes" })
  const unisat = {
    requestAccounts: async () => [vectors.wallet.address],
    getPublicKey: async () => `02${vectors.wallet.publicKey}`,
  }
  await onNetwork("signet", async () => {
    const labcoat = endpoint.client()
    // An installed wallet is not a chosen wallet.
    await assert.rejects(labcoat.connect({ host: { unisat } }), (error: unknown) => {
      assert.ok(error instanceof LabcoatClientError)
      assert.equal(error.code, "WALLET_UNAVAILABLE")
      assert.match(error.message, /does not choose a wallet on signet/)
      assert.match(error.message, /connect\(\{ wallet: "unisat" \}\)/)
      return true
    })
    assert.equal((await labcoat.connect({ wallet: "unisat", host: { unisat } })).name, "UniSat")
    assert.deepEqual(labcoat.availableWallets({ unisat }), ["unisat"])
  })
  assert.deepEqual(endpoint.calls, [])
})

test("waiting outlasts a hosted endpoint's request limit", async () => {
  const endpoint = stubEndpoint({ dialect: "alkanes", trace: { reverted: false, data: u128le(3n), gas: 1n } })
  let limited = 2
  const fetchFn = (async (url: unknown, init?: { body?: unknown }) => {
    const body = JSON.parse(String(init?.body)) as Call
    if (body.method === "metashrew_view" && body.params[0] === "trace" && limited-- > 0) {
      return new Response(JSON.stringify({ error: "Rate limit exceeded (20 req/min).", code: "IP_RATE_LIMIT" }), {
        status: 429,
        headers: { "retry-after": "60" },
      })
    }
    return endpoint.fetchFn(url as string, init as RequestInit)
  }) as typeof fetch
  const labcoat = endpoint.client({ fetchFn })
  const tx = await labcoat.contract("2:1", abiCounter).write.add(3n, { wallet: signingWallet().wallet })
  assert.equal((await tx.wait({ pollMs: 1 })).value, 3n)
  assert.equal(limited, -1)
})

test("a transaction a hosted endpoint does not know is an argument error", async () => {
  const endpoint = stubEndpoint({ dialect: "alkanes" })
  await assert.rejects(
    endpoint.client().waitForTransaction("ab".repeat(32)),
    (error: unknown) => error instanceof LabcoatClientError && error.code === "ARG_INVALID",
  )
})

/* --------------------------------- connecting ------------------------------ */

test("connect returns the dev wallet on Labcoat Network and never picks a browser wallet", async () => {
  const unisat = {
    requestAccounts: async () => [vectors.wallet.address],
    getPublicKey: async () => `02${vectors.wallet.publicKey}`,
  }
  const withSigner = stubEndpoint().client()
  assert.equal((await withSigner.connect({ host: { unisat } })).name, "Labcoat dev wallet")
  assert.deepEqual(withSigner.availableWallets({ unisat }), ["dev", "unisat"])

  const unavailable = (error: unknown) =>
    error instanceof LabcoatClientError && error.code === "WALLET_UNAVAILABLE"
  // Without a dev signer there is no wallet, whatever is installed.
  const without = stubEndpoint({ devSigner: false }).client()
  await assert.rejects(without.connect({ host: { unisat } }), unavailable)
  await assert.rejects(without.connect({ wallet: "dev" }), unavailable)
  // A browser wallet is returned when it is asked for by name.
  assert.equal((await without.connect({ wallet: "unisat", host: { unisat } })).name, "UniSat")
  await assert.rejects(without.connect({ wallet: "leather", host: { unisat } }), unavailable)
})

/* --------------------------- prepare and broadcast ------------------------- */

const ACCOUNTS = [{ address: vectors.wallet.address, publicKey: vectors.wallet.publicKey }]

test("prepare builds what a write asks the wallet to sign, and broadcast sends it signed", async () => {
  const endpoint = stubEndpoint()
  const labcoat = endpoint.client()
  const counter = labcoat.contract("2:1", abiCounter)

  const { wallet, requests } = recordingWallet()
  await assert.rejects(counter.write.add(5n, { wallet }), WalletRejectedError)
  const prepared = await counter.prepare.add(5n, { accounts: ACCOUNTS })
  assert.deepEqual(
    { psbt: prepared.psbt, inputs: prepared.inputs, summary: prepared.summary },
    requests[0],
  )
  assert.equal(prepared.fee, prepared.summary.fee)
  // Preparing asks no wallet and sends nothing.
  assert.equal(requests.length, 1)
  assert.equal(endpoint.called("sendrawtransaction").length, 0)
  // The wallet's addresses serve as well as a list of them.
  assert.equal((await counter.prepare.add(5n, { wallet })).psbt, prepared.psbt)
  await assert.rejects(
    counter.prepare.add(5n),
    (error: unknown) => error instanceof LabcoatClientError && error.code === "WALLET_UNAVAILABLE",
  )

  // Signed somewhere else, for as long as that took.
  const decoded = decodePsbt(prepared.psbt)
  const tx = await labcoat.broadcast(prepared, { hex: withWitnesses(decoded.transaction, decoded.inputs.length) })
  assert.equal(tx.txid, BROADCAST_TXID)
  assert.equal(tx.fee, prepared.fee)
  assert.equal(endpoint.called("sendrawtransaction").length, 1)
  // No wallet was named, so no block is mined for it.
  assert.equal(endpoint.called("generatetoaddress").length, 0)
  endpoint.state.trace = { reverted: false, data: u128le(12n), gas: 7n }
  const receipt = await tx.wait({ pollMs: 1 })
  assert.equal(receipt.value, 12n)
  assert.deepEqual(endpoint.state.traceVouts, [4])

  // Another transaction is not the one prepared.
  const other = buildCallTransaction({ ...baseRequest, utxos: [output(9, 80_000n)] })
  await assert.rejects(labcoat.broadcast(prepared, { psbt: other.psbt }), /the signer returned a different transaction/)
  assert.equal(endpoint.called("sendrawtransaction").length, 1)
})

test("a wallet that raises the fee by lowering the change is accepted only when it says it does", async () => {
  const signer = (raisesFee: boolean, index: number, change: bigint): LabcoatWallet => ({
    name: "Raiser",
    ...(raisesFee ? { raisesFee } : {}),
    getAccounts: async () => ACCOUNTS,
    signPsbt: async (request) => {
      const decoded = decodePsbt(request.psbt)
      return { hex: withWitnesses(withOutputValue(decoded.transaction, index, change), decoded.inputs.length) }
    },
  })
  const endpoint = stubEndpoint()
  const labcoat = endpoint.client()
  const counter = labcoat.contract("2:1", abiCounter)
  const built = await counter.prepare.increment({ accounts: ACCOUNTS })
  // Recipient, change, OP_RETURN.
  assert.equal(built.call.changeOutput, 1)

  await assert.rejects(counter.write.increment({ wallet: signer(false, 1, -150n) }), (error: unknown) => {
    assert.ok(error instanceof LabcoatClientError)
    assert.equal(error.code, "WALLET_ERROR")
    assert.match(error.message, /Raiser raised the fee from \d+ to \d+ sats/)
    return true
  })
  // Only the change may shrink: not another output, and it may not grow.
  await assert.rejects(counter.write.increment({ wallet: signer(true, 0, -150n) }), /a different transaction/)
  await assert.rejects(counter.write.increment({ wallet: signer(true, 1, 150n) }), /a different transaction/)
  assert.equal(endpoint.called("sendrawtransaction").length, 0)

  const tx = await counter.write.increment({ wallet: signer(true, 1, -150n) })
  assert.equal(tx.fee, built.fee + 150n)
  assert.equal(endpoint.called("sendrawtransaction").length, 1)
  // An app that signs for itself says so at broadcast.
  const decoded = decodePsbt(built.psbt)
  const raised = { hex: withWitnesses(withOutputValue(decoded.transaction, 1, -150n), decoded.inputs.length) }
  await assert.rejects(labcoat.broadcast(built, raised), /raised the fee/)
  assert.equal((await labcoat.broadcast(built, raised, { allowFeeChange: true })).fee, built.fee + 150n)
})

/* ---------------------------------- waiting -------------------------------- */

test("a wait reports broadcast, pending, confirmed, and indexed, each once and in order", async () => {
  const endpoint = stubEndpoint()
  const labcoat = endpoint.client()
  const tx = await labcoat
    .contract("2:1", abiCounter)
    .write.add(5n, { wallet: await labcoat.connect({ mine: false }) })
  const seen: TransactionStatus[] = []
  const receipt = await tx.wait({
    pollMs: 1,
    onStatus: (status) => {
      seen.push(status)
      // The chain moves on only after the page has heard each step.
      if (status.status === "pending") endpoint.state.confirmedAt = 301
      if (status.status === "confirmed") endpoint.state.trace = { reverted: false, data: u128le(9n), gas: 1n }
    },
  })
  assert.equal(receipt.value, 9n)
  assert.deepEqual(seen, [
    { status: "broadcast", txid: BROADCAST_TXID },
    { status: "pending", txid: BROADCAST_TXID },
    { status: "confirmed", txid: BROADCAST_TXID, blockHeight: 301 },
    { status: "indexed", txid: BROADCAST_TXID },
  ])

  // By transaction id too; a step nobody saw is skipped, and a callback that throws changes nothing.
  const again: string[] = []
  await labcoat.waitForTransaction(tx.txid, {
    pollMs: 1,
    onStatus: ({ status }) => {
      again.push(status)
      throw new Error("the page fell over")
    },
  })
  assert.deepEqual(again, ["broadcast", "indexed"])
})

test("a reverted call is reported indexed before the wait throws", async () => {
  const endpoint = stubEndpoint({ trace: { reverted: true, data: [0, 0, 0, 0, ...utf8("no")], gas: 1n } })
  const labcoat = endpoint.client()
  const tx = await labcoat.contract("2:1", abiCounter).write.increment({ wallet: signingWallet().wallet, simulate: false })
  const seen: string[] = []
  await assert.rejects(tx.wait({ pollMs: 1, onStatus: ({ status }) => seen.push(status) }), ContractRevertError)
  assert.deepEqual(seen, ["broadcast", "indexed"])
})

test("progress on a hosted endpoint costs one request every third poll, and none unasked", async () => {
  const run = async (onStatus: boolean) => {
    const endpoint = stubEndpoint({ dialect: "alkanes" })
    let traces = 0
    const fetchFn = (async (url: unknown, init?: { body?: unknown }) => {
      const body = JSON.parse(String(init?.body)) as Call
      // The index has the call at the eighth look.
      if (body.method === "metashrew_view" && body.params[0] === "trace" && ++traces === 8) {
        endpoint.state.trace = { reverted: false, data: u128le(3n), gas: 1n }
      }
      return endpoint.fetchFn(url as string, init as RequestInit)
    }) as typeof fetch
    const labcoat = endpoint.client({ fetchFn })
    const tx = await labcoat.contract("2:1", abiCounter).write.add(3n, { wallet: signingWallet().wallet })
    const seen: string[] = []
    await tx.wait({ pollMs: 1, ...(onStatus ? { onStatus: ({ status }) => seen.push(status) } : {}) })
    return { seen, traces, status: endpoint.called("esplora_tx::status").length, hex: endpoint.called("esplora_tx::hex").length }
  }
  // Seven polls find nothing: polls 0, 3, and 6 also ask whether it is in a block.
  assert.deepEqual(await run(true), { seen: ["broadcast", "pending", "indexed"], traces: 8, status: 3, hex: 0 })
  assert.deepEqual(await run(false), { seen: [], traces: 8, status: 0, hex: 0 })
})

/* ------------------------------ browser wallets ---------------------------- */

const PSBT = buildCallTransaction({ ...baseRequest, utxos: [output(7, 90_000n)] })
const signing: SignPsbtRequest = {
  psbt: PSBT.psbt,
  inputs: [{ index: 0, address: vectors.wallet.address }],
  summary: {
    network: "signet",
    contract: "2:1",
    method: "increment",
    fee: PSBT.fee,
    recipient: vectors.wallet.address,
    alkanes: [],
    payments: [],
  },
}
const PSBT_HEX = Buffer.from(PSBT.psbt, "base64").toString("hex")

const rejected = (name: string) => (error: unknown) =>
  error instanceof WalletRejectedError && error.wallet === name

test("UniSat: accounts, signing options, network check, rejection", async () => {
  const seen: unknown[][] = []
  const unisat = {
    requestAccounts: async () => ["tb1pexample"],
    getPublicKey: async () => "03" + "11".repeat(32),
    getChain: async () => ({ enum: "BITCOIN_SIGNET", name: "Bitcoin Signet", network: "testnet" }),
    signPsbt: async (...args: unknown[]) => {
      seen.push(args)
      return "70736274ff00"
    },
  }
  const wallet = unisatWallet("signet", { unisat })
  assert.deepEqual(await wallet.getAccounts(), [{ address: "tb1pexample", publicKey: "03" + "11".repeat(32) }])
  assert.deepEqual(await wallet.signPsbt(signing), { psbt: "70736274ff00" })
  assert.deepEqual(seen, [
    [PSBT_HEX, { autoFinalized: true, toSignInputs: [{ index: 0, address: vectors.wallet.address }] }],
  ])

  // Signet and testnet share an address format; the chain tells them apart.
  await assert.rejects(unisatWallet("testnet", { unisat }).getAccounts(), /UniSat is on BITCOIN_SIGNET; switch it to testnet/)

  const declining = {
    ...unisat,
    requestAccounts: async () => Promise.reject({ code: 4001, message: "User rejected the request." }),
    signPsbt: async () => Promise.reject({ code: 4001, message: "User rejected the request." }),
  }
  await assert.rejects(unisatWallet("signet", { unisat: declining }).getAccounts(), rejected("UniSat"))
  await assert.rejects(unisatWallet("signet", { unisat: declining }).signPsbt(signing), rejected("UniSat"))
  await assert.rejects(
    unisatWallet("signet", {}).getAccounts(),
    (error: unknown) => error instanceof LabcoatClientError && error.code === "WALLET_UNAVAILABLE",
  )
})

test("OKX Wallet: one provider per network", async () => {
  const seen: unknown[][] = []
  const provider = (address: string) => ({
    connect: async () => ({ address, publicKey: "22".repeat(32) }),
    signPsbt: async (...args: unknown[]) => {
      seen.push(args)
      return "70736274ff00"
    },
  })
  const okxwallet = { bitcoin: provider("bc1pmain"), bitcoinSignet: provider("tb1psignet") }
  assert.deepEqual(await okxWallet("mainnet", { okxwallet }).getAccounts(), [
    { address: "bc1pmain", publicKey: "22".repeat(32) },
  ])
  const signet = okxWallet("signet", { okxwallet })
  assert.equal((await signet.getAccounts())[0]?.address, "tb1psignet")
  assert.deepEqual(await signet.signPsbt(signing), { psbt: "70736274ff00" })
  assert.deepEqual(seen[0]?.[1], {
    autoFinalized: true,
    toSignInputs: [{ index: 0, address: vectors.wallet.address }],
  })
  const unavailable = (error: unknown) =>
    error instanceof LabcoatClientError && error.code === "WALLET_UNAVAILABLE"
  await assert.rejects(okxWallet("testnet", { okxwallet }).getAccounts(), unavailable)
  await assert.rejects(okxWallet("regtest", { okxwallet }).getAccounts(), unavailable)
})

test("Xverse: JSON-RPC envelopes, payment and ordinals addresses, rejection", async () => {
  const seen: unknown[][] = []
  let decline = false
  const BitcoinProvider = {
    request: async (method: string, params: unknown) => {
      seen.push([method, params])
      // Xverse resolves with the error; it does not throw.
      if (decline) return { jsonrpc: "2.0", id: 1, error: { code: -32000, message: "User rejected request to sign a psbt" } }
      if (method === "wallet_connect") {
        return {
          jsonrpc: "2.0",
          id: 1,
          result: {
            network: { bitcoin: { name: "Signet" } },
            addresses: [
              { address: "tb1qpay", publicKey: "02" + "33".repeat(32), purpose: "payment", addressType: "p2wpkh" },
              { address: "tb1pord", publicKey: "44".repeat(32), purpose: "ordinals", addressType: "p2tr" },
              { address: "ST1stacks", publicKey: "55", purpose: "stacks" },
            ],
          },
        }
      }
      return { jsonrpc: "2.0", id: 2, result: { psbt: PSBT.psbt } }
    },
  }
  const host = { XverseProviders: { BitcoinProvider } }
  const wallet = xverseWallet("signet", host)
  assert.deepEqual(await wallet.getAccounts(), [
    { address: "tb1qpay", publicKey: "02" + "33".repeat(32), purpose: "payment" },
    { address: "tb1pord", publicKey: "44".repeat(32), purpose: "ordinals" },
  ])
  assert.deepEqual(
    await wallet.signPsbt({
      ...signing,
      inputs: [
        { index: 0, address: "tb1qpay" },
        { index: 1, address: "tb1pord" },
        { index: 2, address: "tb1qpay" },
      ],
    }),
    { psbt: PSBT.psbt },
  )
  assert.deepEqual(seen[1], [
    "signPsbt",
    { psbt: PSBT.psbt, signInputs: { tb1qpay: [0, 2], tb1pord: [1] }, broadcast: false },
  ])
  await assert.rejects(xverseWallet("mainnet", host).getAccounts(), /Xverse is on Signet; switch it to mainnet/)
  decline = true
  await assert.rejects(wallet.getAccounts(), rejected("Xverse"))
  await assert.rejects(wallet.signPsbt(signing), rejected("Xverse"))
})

test("Leather: addresses by type, hex PSBTs, rejection", async () => {
  const seen: unknown[][] = []
  let decline = false
  const LeatherProvider = {
    request: async (method: string, params: unknown) => {
      seen.push([method, params])
      // Leather rejects with the JSON-RPC response.
      if (decline) throw { jsonrpc: "2.0", id: 1, error: { code: 4001, message: "User rejected signing PSBT request" } }
      if (method === "getAddresses") {
        return {
          jsonrpc: "2.0",
          id: 1,
          result: {
            addresses: [
              { symbol: "BTC", type: "p2wpkh", address: "tb1qpay", publicKey: "02" + "66".repeat(32) },
              { symbol: "BTC", type: "p2tr", address: "tb1pord", publicKey: "03" + "77".repeat(32) },
              { symbol: "STX", address: "ST1stacks", publicKey: "88" },
            ],
          },
        }
      }
      return { jsonrpc: "2.0", id: 2, result: { hex: PSBT_HEX } }
    },
  }
  const wallet = leatherWallet("signet", { LeatherProvider })
  assert.deepEqual(await wallet.getAccounts(), [
    { address: "tb1qpay", publicKey: "02" + "66".repeat(32), purpose: "payment" },
    { address: "tb1pord", publicKey: "03" + "77".repeat(32), purpose: "ordinals" },
  ])
  assert.deepEqual(await wallet.signPsbt(signing), { psbt: PSBT_HEX })
  assert.deepEqual(seen, [
    ["getAddresses", { network: "signet" }],
    ["signPsbt", { hex: PSBT_HEX, signAtIndex: [0], broadcast: false, network: "signet" }],
  ])
  decline = true
  await assert.rejects(wallet.signPsbt(signing), rejected("Leather"))
})

test("Subfrost: the selected taproot address, hex PSBTs, network from the address, rejection", async () => {
  const seen: unknown[][] = []
  const signet = "tb1pz9vq0dgghu69wrhv4435ps5d5a3rwl4rr9s539d7avzlzqpg4naq2q2a3k"
  const subfrost = {
    requestAccounts: async () => [signet],
    getPublicKey: async () => "11".repeat(32),
    // The extension answers "mainnet" whatever it is on.
    getNetwork: async () => "mainnet",
    signPsbt: async (...args: unknown[]) => {
      seen.push(args)
      return "70736274ff00"
    },
  }
  const wallet = subfrostWallet("signet", { subfrost })
  assert.equal(wallet.name, "Subfrost")
  assert.deepEqual(await wallet.getAccounts(), [{ address: signet, publicKey: "11".repeat(32) }])
  assert.deepEqual(await wallet.signPsbt(signing), { psbt: "70736274ff00" })
  assert.deepEqual(seen, [[PSBT_HEX]])

  await assert.rejects(
    subfrostWallet("mainnet", { subfrost }).getAccounts(),
    /Subfrost is on another network: tb1p\S+ is not a mainnet address; switch the wallet to mainnet/,
  )
  const segwit = { ...subfrost, requestAccounts: async () => ["tb1qw508d6qejxtdg4y5r3zarvary0c5xw7kxpjzsx"] }
  await assert.rejects(subfrostWallet("signet", { subfrost: segwit }).getAccounts(), /select its taproot address/)

  // A declined prompt has no code of its own.
  const declining = {
    ...subfrost,
    requestAccounts: async () => Promise.reject(new Error("[UNKNOWN_ERROR] El usuario rechazó la solicitud")),
    signPsbt: async () => Promise.reject(new Error("[UNKNOWN_ERROR] El usuario rechazó la solicitud")),
  }
  await assert.rejects(subfrostWallet("signet", { subfrost: declining }).getAccounts(), rejected("Subfrost"))
  await assert.rejects(subfrostWallet("signet", { subfrost: declining }).signPsbt(signing), rejected("Subfrost"))

  const silent = {
    ...subfrost,
    signPsbt: async () => Promise.reject(new Error("[REQUEST_TIMEOUT] subfrost: request timed out")),
  }
  await assert.rejects(subfrostWallet("signet", { subfrost: silent }).signPsbt(signing), (error: unknown) => {
    assert.ok(error instanceof LabcoatClientError && !(error instanceof WalletRejectedError))
    assert.equal(error.code, "WALLET_ERROR")
    assert.match(error.message, /did not answer in time/)
    return true
  })
  await assert.rejects(
    subfrostWallet("signet", {}).getAccounts(),
    (error: unknown) => error instanceof LabcoatClientError && error.code === "WALLET_UNAVAILABLE",
  )
})

test("installed wallets are found by their globals", () => {
  assert.deepEqual(installedBrowserWallets({ subfrost: {} }), ["subfrost"])
  assert.equal(browserWallet("subfrost", "signet", { subfrost: {} }).name, "Subfrost")
  const host = { LeatherProvider: {}, unisat: {}, okxwallet: undefined }
  assert.deepEqual(installedBrowserWallets(host), ["unisat", "leather"])
  assert.equal(browserWallet("xverse", "mainnet", host).name, "Xverse")
})

/* ------------------------- signed PSBTs from wallets ------------------------ */

/** Add one field to the only input of a single-input PSBT. */
function withInputField(psbt: string, key: string, value: string): string {
  const bytes = Array.from(Buffer.from(psbt, "base64"))
  let offset = 5
  const skipMap = (): void => {
    for (;;) {
      const [keyLength, afterKey] = compactSize(bytes, offset)
      offset = afterKey + keyLength
      if (keyLength === 0) return
      const [valueLength, afterValue] = compactSize(bytes, offset)
      offset = afterValue + valueLength
    }
  }
  skipMap()
  const inputStart = offset
  skipMap()
  const terminator = offset - 1
  assert.ok(terminator > inputStart)
  const size = (hexText: string) => [hexText.length / 2]
  const field = [...size(key), ...fromHex(key), ...size(value), ...fromHex(value)]
  return Buffer.from([...bytes.slice(0, terminator), ...field, ...bytes.slice(terminator)]).toString("base64")
}

test("a signed PSBT becomes a transaction, finalized by the wallet or not", () => {
  const unsigned = toHex(PSBT.transaction)
  const signature = "cd".repeat(64)
  const expected = `${unsigned.slice(0, 8)}0001${unsigned.slice(8, -8)}0140${signature}${unsigned.slice(-8)}`
  // UniSat and OKX finalize: PSBT_IN_FINAL_SCRIPTWITNESS.
  assert.equal(extractTransaction(withInputField(PSBT.psbt, "08", `0140${signature}`)), expected)
  // Xverse and Leather leave the taproot key signature: PSBT_IN_TAP_KEY_SIG.
  const keySigned = withInputField(PSBT.psbt, "13", signature)
  assert.equal(extractTransaction(keySigned), expected)
  // Hex, as UniSat, OKX, and Leather return it.
  assert.equal(extractTransaction(Buffer.from(keySigned, "base64").toString("hex")), expected)
  // A P2WPKH input signed but not finalized: PSBT_IN_PARTIAL_SIG.
  const publicKey = `02${"ee".repeat(32)}`
  const der = `30${"ab".repeat(70)}01`
  assert.equal(
    extractTransaction(withInputField(PSBT.psbt, `02${publicKey}`, der)),
    `${unsigned.slice(0, 8)}0001${unsigned.slice(8, -8)}0248${der}21${publicKey}${unsigned.slice(-8)}`,
  )
  assert.throws(
    () => extractTransaction(PSBT.psbt),
    (error: unknown) => error instanceof LabcoatClientError && error.code === "WALLET_ERROR",
  )
})
