// Runtime checks for the client `labcoat generate web` writes, against a
// stub `fetch` that speaks the endpoint's wire format: JSON-RPC carrying
// protobuf views. Run by scripts/tests/generate-web-client-test.sh.

import { test } from "node:test"
import { strict as assert } from "node:assert"

import {
  ContractRevertError,
  LabcoatClientError,
  createClient,
  decodeAlkaneId,
  decodeAlkaneIds,
  decodeU128,
} from "../expected/client"
import type { LabcoatWebClientOptions } from "../expected/client"
import { abiOverwriteTestToken } from "../expected/abi/overwrite-test-token"
import type { AbiDocument } from "../expected/abi"

const abiOrganism = {
  contract: "Organism",
  methods: [
    { name: "get_energy", opcode: 26n, params: [], returns: "u128" },
    { name: "get_parent", opcode: 27n, params: [], returns: "AlkaneId" },
    {
      name: "child_of",
      opcode: 28n,
      params: [
        { name: "parent", type: "AlkaneId" },
        { name: "label", type: "String" },
      ],
      returns: "Vec<u8>",
    },
    { name: "get_count", opcode: 30n, params: [], returns: "u128" },
    { name: "getCount", opcode: 31n, params: [], returns: "String" },
  ],
} as const satisfies AbiDocument

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

function varintField(field: number, value: bigint): number[] {
  return [...varint(BigInt(field * 8)), ...varint(value)]
}

function bytesField(field: number, value: ArrayLike<number>): number[] {
  return [...varint(BigInt(field * 8 + 2)), ...varint(BigInt(value.length)), ...Array.from(value)]
}

function hex(bytes: ArrayLike<number>): string {
  return `0x${Array.from(bytes, (byte) => byte.toString(16).padStart(2, "0")).join("")}`
}

function unhex(text: string): number[] {
  return (text.slice(2).match(/../g) ?? []).map((pair) => parseInt(pair, 16))
}

function u128le(value: bigint): number[] {
  return Array.from({ length: 16 }, (_, i) => Number((value >> BigInt(8 * i)) & 0xffn))
}

const utf8 = (text: string): number[] => Array.from(new TextEncoder().encode(text))

/** alkanes.SimulateResponse { execution { data = 3 } = 1, gas_used = 2, error = 3 } */
function simulateResponse(outcome: { data?: number[]; gas?: bigint; error?: string }): string {
  return hex([
    ...bytesField(1, bytesField(3, outcome.data ?? [])),
    ...varintField(2, outcome.gas ?? 0n),
    ...(outcome.error === undefined ? [] : bytesField(3, utf8(outcome.error))),
  ])
}

/** The height and cellpack of a `simulate` view request. */
function parseParcel(payload: string): { height: number; cellpack: bigint[] } {
  const bytes = unhex(payload)
  let offset = 0
  const readVarint = (): bigint => {
    let value = 0n
    for (let shift = 0n; ; shift += 7n) {
      const byte = bytes[offset++] as number
      value |= BigInt(byte & 0x7f) << shift
      if ((byte & 0x80) === 0) return value
    }
  }
  let height = -1
  const cellpack: bigint[] = []
  while (offset < bytes.length) {
    const tag = readVarint()
    if (tag === 32n) {
      height = Number(readVarint())
    } else if (tag === 42n) {
      const length = Number(readVarint())
      const end = offset + length
      while (offset < end) cellpack.push(readVarint())
    } else {
      throw new Error(`unexpected parcel tag ${tag}`)
    }
  }
  return { height, cellpack }
}

interface Call {
  method: string
  params: unknown[]
}

interface Simulated {
  height: number
  cellpack: bigint[]
}

type Outcome = { data?: number[]; gas?: bigint; error?: string } | { rpcError: string }

/** A stub endpoint: answers `secondaryheight` and routes `simulate` to `respond`. */
function stubEndpoint(respond: (call: Simulated) => Outcome, height = 100) {
  const posts: unknown[] = []
  const calls: Call[] = []
  const simulated: Simulated[] = []

  const answer = (request: Call & { id: number }) => {
    calls.push({ method: request.method, params: request.params })
    const reply = (result: unknown) => ({ jsonrpc: "2.0", id: request.id, result })
    if (request.method === "secondaryheight") return reply(height)
    if (request.method === "secondaryview" && request.params[1] === "simulate") {
      const call = parseParcel(request.params[2] as string)
      simulated.push(call)
      const outcome = respond(call)
      if ("rpcError" in outcome) {
        return { jsonrpc: "2.0", id: request.id, error: { code: -1, message: outcome.rpcError } }
      }
      return reply(simulateResponse(outcome))
    }
    return { jsonrpc: "2.0", id: request.id, error: { code: -32601, message: "Method not found" } }
  }

  const fetchFn = (async (_url: unknown, init?: { body?: unknown }) => {
    const body = JSON.parse(String(init?.body))
    posts.push(body)
    // A batch is answered out of order: answers are matched by id.
    const result = Array.isArray(body) ? body.map(answer).reverse() : answer(body)
    return new Response(JSON.stringify(result), { headers: { "content-type": "application/json" } })
  }) as typeof fetch

  const client = (options: LabcoatWebClientOptions = {}) =>
    createClient({ rpc: "http://stub.invalid/rpc", fetchFn, ...options })
  return { posts, calls, simulated, client }
}

const methodsCalled = (calls: Call[]): string[] => calls.map((call) => call.method)

/* ---------------------------------- reads ---------------------------------- */

test("a read sends the manifest id and opcode and decodes the return by its ABI type", async () => {
  const endpoint = stubEndpoint(({ cellpack }) => {
    if (cellpack[2] === 99n) return { data: utf8("Fire") }
    if (cellpack[2] === 101n) return { data: u128le((1n << 100n) + 7n) }
    if (cellpack[2] === 110n) return { data: [1, 2, 3] }
    return {}
  })
  const labcoat = endpoint.client()
  const fire = labcoat.contract("fire")

  assert.equal(fire.id, "4:65011")
  assert.equal(fire.abi, abiOverwriteTestToken)
  assert.equal(await fire.read.getName(), "Fire")
  assert.equal(await fire.read.getTotalSupply(), (1n << 100n) + 7n)
  assert.equal(await fire.read.forward(), undefined)
  assert.deepEqual(await labcoat.contract("series").read.getSeries(), Uint8Array.from([1, 2, 3]))

  assert.deepEqual(
    endpoint.simulated.map((call) => call.cellpack),
    [
      [4n, 65011n, 99n],
      [4n, 65011n, 101n],
      [4n, 65011n, 1n],
      [4n, 65014n, 110n],
    ],
  )
})

test("arguments are encoded per ABI parameter, with either spelling of an Alkane id", async () => {
  const endpoint = stubEndpoint(() => ({}))
  const labcoat = endpoint.client()

  await labcoat
    .contract("series")
    .read.initialize("2:1", { block: 2n, tx: 3 }, { block: "2", tx: "4" }, 10n, 20, 30n, 40n)
  assert.deepEqual(endpoint.simulated[0]?.cellpack, [
    4n, 65014n, 0n, 2n, 1n, 2n, 3n, 2n, 4n, 10n, 20n, 30n, 40n,
  ])

  await labcoat.contract("2:12", abiOrganism).read.childOf("2:5", "hi")
  // "hi" + NUL, little-endian in one u128 word.
  assert.deepEqual(endpoint.simulated[1]?.cellpack, [2n, 12n, 28n, 2n, 5n, 0x6968n])
})

test("a contract outside the manifest is called by id through an ABI it shares", async () => {
  const endpoint = stubEndpoint(({ cellpack }) => {
    if (cellpack[2] === 26n) return { data: u128le(42n) }
    if (cellpack[2] === 27n) return { data: [...u128le(2n), ...u128le(7n)] }
    if (cellpack[2] === 30n) return { data: u128le(3n) }
    return { data: utf8("three") }
  })
  const labcoat = endpoint.client()
  const clone = labcoat.contract("2:12", abiOrganism)

  assert.equal(clone.id, "2:12")
  assert.equal(await clone.read.getEnergy(), 42n)
  assert.deepEqual(await clone.read.getParent(), { block: "2", tx: "7" })
  assert.equal(await labcoat.contract({ block: 2n, tx: 13 }, abiOrganism).read.getEnergy(), 42n)
  assert.deepEqual(endpoint.simulated.at(-1)?.cellpack, [2n, 13n, 26n])

  // Colliding camelCase names keep their exact ABI names.
  assert.deepEqual(Object.keys(clone.read), [
    "getEnergy",
    "getParent",
    "childOf",
    "get_count",
    "getCount",
  ])
  assert.equal(await clone.read.get_count(), 3n)
  assert.equal(await clone.read.getCount(), "three")
})

test("bytes that do not fit the ABI return type are an error, not a wrong value", async () => {
  const endpoint = stubEndpoint(() => ({ data: new Array<number>(17).fill(1) }))
  const clone = endpoint.client().contract("2:12", abiOrganism)
  await assert.rejects(clone.read.getEnergy(), { code: "RESPONSE_INVALID" })
  await assert.rejects(clone.read.getParent(), { code: "RESPONSE_INVALID" })
})

/* --------------------------------- reverts --------------------------------- */

test("a revert throws ContractRevertError carrying the contract's reason", async () => {
  const endpoint = stubEndpoint(() => ({ error: "ALKANES: revert: no such organism", gas: 9n }))
  const clone = endpoint.client().contract("2:12", abiOrganism)

  await assert.rejects(clone.read.getEnergy(), (error: unknown) => {
    assert.ok(error instanceof ContractRevertError)
    assert.ok(error instanceof LabcoatClientError)
    assert.equal(error.code, "CONTRACT_REVERT")
    assert.equal(error.reason, "no such organism")
    assert.equal(error.contract, "2:12")
    assert.equal(error.method, "get_energy")
    assert.equal(error.gasUsed, 9n)
    assert.equal(error.message, "get_energy on 2:12 reverted: no such organism")
    return true
  })
})

test("tryRead returns a revert instead of throwing it", async () => {
  let exists = false
  const endpoint = stubEndpoint(() =>
    exists ? { data: u128le(5n), gas: 3n } : { error: "unexpected end of file" },
  )
  const clone = endpoint.client().contract("2:12", abiOrganism)

  const missing = await clone.tryRead.getEnergy()
  assert.equal(missing.status, "revert")
  assert.ok(missing.status === "revert")
  assert.equal(missing.reason, "unexpected end of file")
  assert.ok(missing.error instanceof ContractRevertError)

  exists = true
  const found = await clone.tryRead.getEnergy()
  assert.deepEqual(found, {
    status: "success",
    value: 5n,
    gasUsed: 3n,
    data: Uint8Array.from(u128le(5n)),
  })
})

test("a transport failure is thrown by tryRead too", async () => {
  const endpoint = stubEndpoint(() => ({ rpcError: "index unavailable" }))
  const clone = endpoint.client().contract("2:12", abiOrganism)
  await assert.rejects(clone.tryRead.getEnergy(), {
    code: "RPC_ERROR",
    message: "secondaryview failed: index unavailable",
  })
})

/* ---------------------------------- height --------------------------------- */

test("a pinned client reads at its height without looking the height up", async () => {
  const endpoint = stubEndpoint(() => ({ data: u128le(1n) }), 100)
  const labcoat = endpoint.client()
  const pinned = labcoat.at(77)

  assert.equal(labcoat.height, null)
  assert.equal(pinned.height, 77)
  await pinned.contract("fire").read.getTotalSupply()
  await pinned.contract("2:12", abiOrganism).read.getEnergy()
  await pinned.simulate("fire", "get_name")

  assert.deepEqual(methodsCalled(endpoint.calls), ["secondaryview", "secondaryview", "secondaryview"])
  assert.deepEqual(
    endpoint.simulated.map((call) => call.height),
    [77, 77, 77],
  )
  // The live height is still one call away.
  assert.equal(await pinned.getIndexedHeight(), 100)
  assert.throws(() => labcoat.at(-1), { code: "ARG_INVALID" })
})

test("an unpinned read runs at the current indexed height", async () => {
  const endpoint = stubEndpoint(() => ({ data: u128le(1n) }), 314)
  await endpoint.client().contract("fire").read.getTotalSupply()
  assert.deepEqual(methodsCalled(endpoint.calls), ["secondaryheight", "secondaryview"])
  assert.equal(endpoint.simulated[0]?.height, 314)
})

test("concurrent reads share one height lookup and one batch request", async () => {
  const endpoint = stubEndpoint(({ cellpack }) => ({ data: u128le(cellpack[2] ?? 0n) }))
  const labcoat = endpoint.client()
  const fire = labcoat.contract("fire")
  const clone = labcoat.contract("2:12", abiOrganism)

  const values = await Promise.all([
    fire.read.getTotalSupply(),
    clone.read.getEnergy(),
    clone.read.get_count(),
  ])

  // Out-of-order batch answers still reach the right caller.
  assert.deepEqual(values, [101n, 26n, 30n])
  assert.equal(endpoint.posts.length, 2)
  assert.equal(Array.isArray(endpoint.posts[0]), false)
  assert.equal((endpoint.posts[1] as unknown[]).length, 3)
  assert.deepEqual(
    endpoint.simulated.map((call) => call.height),
    [100, 100, 100],
  )
})

test("a pinned Promise.all is a single request", async () => {
  const endpoint = stubEndpoint(() => ({ data: u128le(1n) }))
  const fire = endpoint.client().at(50).contract("fire")
  await Promise.all([fire.read.getTotalSupply(), fire.read.getName(), fire.read.getSymbol()])
  assert.equal(endpoint.posts.length, 1)
})

test("one failing call in a batch fails only its own read", async () => {
  const endpoint = stubEndpoint(({ cellpack }) =>
    cellpack[2] === 26n ? { rpcError: "boom" } : { data: u128le(8n) },
  )
  const clone = endpoint.client().at(1).contract("2:12", abiOrganism)
  const [energy, count] = await Promise.allSettled([clone.read.getEnergy(), clone.read.get_count()])
  assert.equal(energy.status, "rejected")
  assert.deepEqual(count, { status: "fulfilled", value: 8n })
})

test("batch: false sends every call on its own", async () => {
  const endpoint = stubEndpoint(() => ({ data: u128le(1n) }))
  const fire = endpoint.client({ batch: false }).at(50).contract("fire")
  await Promise.all([fire.read.getTotalSupply(), fire.read.getName()])
  assert.equal(endpoint.posts.length, 2)
  assert.ok(endpoint.posts.every((post) => !Array.isArray(post)))
})

test("an endpoint that does not answer a batch as a batch says how to turn batching off", async () => {
  const fetchFn = (async () =>
    new Response(JSON.stringify({ error: { code: -32600, message: "Invalid Request" } }))) as typeof fetch
  const fire = createClient({ rpc: "http://stub.invalid", fetchFn }).at(1).contract("fire")
  const [first] = await Promise.allSettled([fire.read.getName(), fire.read.getSymbol()])
  assert.ok(first.status === "rejected")
  assert.match(String(first.reason.message), /batch: false/)
})

/* ------------------------------- raw simulate ------------------------------ */

test("simulate stays the untyped, non-throwing call", async () => {
  const endpoint = stubEndpoint(({ cellpack }) => {
    if (cellpack[2] === 99n) return { data: utf8("Fire"), gas: 11n }
    if (cellpack[2] === 26n) return { data: u128le(6n) }
    return { error: "ALKANES: revert: nope" }
  })
  const labcoat = endpoint.client({ baseUrl: "http://stub.invalid/legacy" })
  assert.equal(labcoat.rpc, "http://stub.invalid/rpc")

  const named = await labcoat.simulate("fire", "get_name")
  assert.deepEqual(
    { status: named.status, value: named.value, gasUsed: named.gasUsed, error: named.error },
    { status: "success", value: "Fire", gasUsed: 11n, error: null },
  )

  // A numeric opcode with raw u128 words: raw bytes back, nothing decoded.
  const raw = await labcoat.simulate("2:12", "26", [1n, 2, "3"], { height: 9 })
  assert.equal(raw.value, null)
  assert.deepEqual(raw.data, Uint8Array.from(u128le(6n)))
  assert.deepEqual(endpoint.simulated.at(-1), { height: 9, cellpack: [2n, 12n, 26n, 1n, 2n, 3n] })

  // A method name on a bare id resolves through options.abi.
  const energy = await labcoat.simulate("2:12", "get_energy", [], { abi: abiOrganism })
  assert.equal(energy.value, 6n)
  await assert.rejects(labcoat.simulate("2:12", "get_energy"), { code: "METHOD_UNKNOWN" })

  const reverted = await labcoat.simulate("fire", "forward")
  assert.deepEqual(
    { status: reverted.status, error: reverted.error, value: reverted.value },
    { status: "revert", error: "ALKANES: revert: nope", value: null },
  )
})

test("baseUrl still names the endpoint when rpc is not given", () => {
  const fetchFn = (async () => new Response("{}")) as typeof fetch
  const legacy = createClient({ baseUrl: "/legacy", fetchFn })
  assert.equal(legacy.rpc, "/legacy")
  assert.equal(legacy.baseUrl, "/legacy")
  assert.equal(createClient({ fetchFn }).rpc, "http://127.0.0.1:18443")
})

/* ----------------------------------- errors -------------------------------- */

test("unknown contracts, missing ABIs, and bad arguments fail before any request", async () => {
  const endpoint = stubEndpoint(() => ({}))
  const labcoat = endpoint.client()
  // What an untyped caller (plain JavaScript, a dynamic name) can still do.
  const untyped = labcoat.contract.bind(labcoat) as unknown as (name: string) => unknown

  assert.throws(() => untyped("nope"), { code: "CONTRACT_UNKNOWN" })
  assert.throws(() => untyped("legacy"), { code: "ABI_UNKNOWN" })
  assert.throws(() => untyped("2:12"), { code: "ABI_UNKNOWN" })
  assert.throws(() => labcoat.contract("2:12:1", abiOrganism), { code: "ARG_INVALID" })

  const clone = labcoat.contract("2:12", abiOrganism)
  await assert.rejects(clone.read.childOf("not-an-id", "x"), { code: "ARG_INVALID" })
  await assert.rejects(
    labcoat.contract("fire").read.initialize(-1, 1n),
    { code: "ARG_INVALID" },
  )
  assert.equal(endpoint.posts.length, 0)
})

/* ------------------------------ decode helpers ----------------------------- */

test("decodeU128, decodeAlkaneId, and decodeAlkaneIds unpack what contracts return as bytes", () => {
  const le16 = (value: bigint) => Array.from({ length: 16 }, (_, i) => Number((value >> BigInt(8 * i)) & 0xffn))
  const id = (block: bigint, tx: bigint) => [...le16(block), ...le16(tx)]
  const max = (1n << 128n) - 1n
  const packed = Uint8Array.from([...id(2n, 7n), ...id(4n, 43793n), ...id(2n, max)])

  assert.equal(decodeU128(packed), 2n)
  assert.equal(decodeU128(packed, 48), 43793n)
  assert.equal(decodeU128(packed, 80), max)
  // The same shape a read returns for an AlkaneId.
  assert.deepEqual(decodeAlkaneId(packed), { block: "2", tx: "7" })
  assert.deepEqual(decodeAlkaneId(packed, 32), { block: "4", tx: "43793" })
  assert.deepEqual(decodeAlkaneIds(packed), [
    { block: "2", tx: "7" },
    { block: "4", tx: "43793" },
    { block: "2", tx: max.toString() },
  ])
  assert.deepEqual(decodeAlkaneIds(new Uint8Array()), [])

  const invalid = (error: unknown) => error instanceof LabcoatClientError && error.code === "RESPONSE_INVALID"
  assert.throws(() => decodeU128(new Uint8Array(15)), invalid)
  assert.throws(() => decodeU128(packed, 81), invalid)
  assert.throws(() => decodeU128(packed, -16), invalid)
  assert.throws(() => decodeU128(packed, 0.5), invalid)
  assert.throws(() => decodeAlkaneId(new Uint8Array(31)), invalid)
  assert.throws(() => decodeAlkaneId(packed, 65), invalid)
  assert.throws(() => decodeAlkaneIds(packed.subarray(0, 33)), invalid)
  assert.throws(() => decodeAlkaneIds(packed.subarray(0, 16)), invalid)
})

/* ---------------------------------- balances ------------------------------- */

test("balances are summed per Alkane across outpoints", async () => {
  const uint128 = (value: bigint) => varintField(1, value)
  const item = (tx: bigint, name: string, symbol: string, balance: bigint) =>
    bytesField(1, [
      ...bytesField(1, [
        ...bytesField(1, [...bytesField(1, uint128(2n)), ...bytesField(2, uint128(tx))]),
        ...bytesField(2, utf8(name)),
        ...bytesField(5, utf8(symbol)),
      ]),
      ...bytesField(2, uint128(balance)),
    ])
  const outpoint = (...items: number[][]) => bytesField(1, bytesField(1, items.flat()))
  const wallet = [
    ...outpoint(item(9n, "Nine", "NIN", 5n), item(3n, "Three", "THR", 1n)),
    ...outpoint(item(9n, "Nine", "NIN", 7n)),
  ]
  const fetchFn = (async (_url: unknown, init?: { body?: unknown }) => {
    const { id, params } = JSON.parse(String(init?.body))
    assert.equal(params[1], "protorunesbyaddress")
    return new Response(JSON.stringify({ jsonrpc: "2.0", id, result: hex(wallet) }))
  }) as typeof fetch

  const balances = await createClient({ rpc: "/rpc", fetchFn }).getAlkanesBalances("bcrt1qexample")
  assert.deepEqual(balances, [
    { id: { block: "2", tx: "3" }, key: "2:3", name: "Three", symbol: "THR", balance: 1n },
    { id: { block: "2", tx: "9" }, key: "2:9", name: "Nine", symbol: "NIN", balance: 12n },
  ])
})
