// Compile-time checks for the client `labcoat generate web` writes. Nothing
// here runs: `tsc` passing is the test. Each `@ts-expect-error` line is a
// mistake the generated types must reject, and fails the build if they
// stop rejecting it.

import { createClient, decodeAlkaneId, decodeAlkaneIds, decodeU128 } from "../expected/client"
import type {
  AbiArg,
  AbiValue,
  AlkaneRef,
  ContractHandle,
  ContractRevertError,
  LabcoatWallet,
  ReadMethods,
  ReadResult,
  TransactionReceipt,
  TransactionSummary,
  TypedContractName,
  WriteMethods,
  WriteResult,
} from "../expected/client"
import type { ContractName } from "../expected/manifest"
import { abiOverwriteTestToken } from "../expected/abi/overwrite-test-token"
import type { AbiDocument } from "../expected/abi"

type Equal<X, Y> =
  (<T>() => T extends X ? 1 : 2) extends <T>() => T extends Y ? 1 : 2 ? true : false
type Expect<T extends true> = T

/** An ABI the manifest does not know: what a factory's clones share. */
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
    // camelCase collision: both keep their exact ABI names.
    { name: "get_count", opcode: 30n, params: [], returns: "u128" },
    { name: "getCount", opcode: 31n, params: [], returns: "String" },
    // Types the client does not know.
    { name: "is_alive", opcode: 32n, params: [], returns: "bool" },
    { name: "set_flags", opcode: 33n, params: [{ name: "flags", type: "u64" }], returns: "void" },
  ],
} as const satisfies AbiDocument

export async function typeChecks(wideAbi: AbiDocument, dynamicId: string): Promise<void> {
  const labcoat = createClient({ rpc: "/rpc" })

  /* ----------------------------- manifest names ---------------------------- */

  const token = labcoat.contract("fire")
  const series = labcoat.contract("series")

  const supply: bigint = await token.read.getTotalSupply()
  const name: string = await token.read.getName()
  const bytes: Uint8Array = await series.read.getSeries()
  const nothing: undefined = await token.read.forward()
  void [supply, name, bytes, nothing]

  await token.read.initialize(1, 2n)
  await series.read.initialize("2:1", { block: 2n, tx: 3 }, { block: "2", tx: "4" }, 1n, 2n, 3n, 4)

  // @ts-expect-error a misspelled method
  await token.read.getTotalSuply()
  // @ts-expect-error the ABI's snake_case name is not a second spelling
  await token.read.get_total_supply()
  // @ts-expect-error an argument to a method that takes none
  await token.read.getName(1n)
  // @ts-expect-error a missing argument
  await token.read.initialize(1n)
  // @ts-expect-error a string where a u128 belongs
  await token.read.initialize("1", 2n)
  // @ts-expect-error a number where an AlkaneId belongs
  await series.read.initialize(1, "2:2", "2:3", 1n, 2n, 3n, 4n)
  // @ts-expect-error a bigint return is not a string
  const wrong: string = await token.read.getTotalSupply()
  void wrong

  // @ts-expect-error not a contract in the manifest
  labcoat.contract("nope")
  // @ts-expect-error `legacy` is in the manifest but no ABI was built for it
  labcoat.contract("legacy")
  // ...so it takes one explicitly.
  labcoat.contract("legacy", abiOverwriteTestToken)

  /* ------------------------------ id plus ABI ------------------------------ */

  const clone = labcoat.contract("2:12", abiOrganism)
  labcoat.contract({ block: 2n, tx: 12n }, abiOrganism)
  labcoat.contract(dynamicId, abiOrganism)

  const energy: bigint = await clone.read.getEnergy()
  const parent: AlkaneRef = await clone.read.getParent()
  const child: Uint8Array = await clone.read.childOf(parent, "first")
  void [energy, child]

  // Colliding camelCase names keep their exact ABI names, each with its own types.
  const count: bigint = await clone.read.get_count()
  const label: string = await clone.read.getCount()
  void [count, label]

  // An unknown return type comes back as raw bytes.
  const alive: Uint8Array = await clone.read.isAlive()
  void alive
  // @ts-expect-error a parameter type the client cannot encode
  await clone.read.setFlags(1n)

  // @ts-expect-error an id without an ABI has no typed methods
  labcoat.contract("2:12")

  /* --------------------------- reverts, not thrown -------------------------- */

  const result = await clone.tryRead.getEnergy()
  if (result.status === "success") {
    const value: bigint = result.value
    void value
    // @ts-expect-error a successful read has no reason
    void result.reason
  } else {
    const reason: string = result.reason
    const error: ContractRevertError = result.error
    void [reason, error]
    // @ts-expect-error a reverted read has no value
    void result.value
  }
  // @ts-expect-error tryRead is typed like read
  await clone.tryRead.getEnergy(1n)

  /* ------------------------------ pinned height ----------------------------- */

  const pinned = labcoat.at(120)
  const pinnedSupply: bigint = await pinned.contract("usd").read.getTotalSupply()
  const height: number | null = pinned.height
  void [pinnedSupply, height]
  // @ts-expect-error a height is a number
  labcoat.at("120")

  /* --------------------------- ABIs known at runtime ------------------------ */

  // An ABI that is not a literal (fetched, parsed from JSON) still gives a
  // handle; its methods are indexed by string and loosely typed.
  const loose = labcoat.contract("2:12", wideAbi)
  const value: AbiValue = await loose.read["anything"]?.(1n, "2:1")
  void value

  /* ---------------------------------- writes -------------------------------- */

  const wallet: LabcoatWallet = await labcoat.connect()
  // The method's arguments, then the options.
  const sent: WriteResult<Uint8Array> = await clone.write.childOf("2:1", "seedling", { wallet })
  const receipt: TransactionReceipt<Uint8Array> = await sent.wait()
  const mined: bigint = (await (await clone.write.getEnergy({ wallet, feeRate: 3 })).wait({ timeoutMs: 1 })).value
  const txid: string = sent.txid
  const fee: bigint = sent.fee
  void [receipt, mined, txid, fee]
  await clone.write.getEnergy({
    wallet,
    alkanes: [{ id: "2:5", amount: 300n }],
    pay: [{ address: "bcrt1q...", sats: 10_000 }],
    to: "bcrt1p...",
    simulate: false,
  })
  // The wallet may come from the client instead.
  await clone.write.getEnergy()
  // By transaction id, the return value stays bytes.
  const bare: Uint8Array = (await labcoat.waitForTransaction(txid)).value
  void bare
  // Anything with the three members is a wallet.
  const own: LabcoatWallet = {
    name: "mine",
    getAccounts: async () => [{ address: "bcrt1p...", publicKey: "00" }],
    signPsbt: async ({ psbt }) => ({ psbt }),
  }
  await labcoat.contract("fire").write.forward({ wallet: own })
  // @ts-expect-error write checks arguments like read
  await clone.write.childOf("2:1", { wallet })
  // @ts-expect-error write has no method the ABI lacks
  await clone.write.grow({ wallet })
  // @ts-expect-error an unknown option
  await clone.write.getEnergy({ wallet, gasLimit: 1 })
  // @ts-expect-error a wallet must be able to sign
  await clone.write.getEnergy({ wallet: { name: "half", getAccounts: async () => [] } })
  // @ts-expect-error only the wallets connect knows by name
  await labcoat.connect({ wallet: "metamask" })

  /* ------------------------------ raw simulate ------------------------------ */

  await labcoat.simulate("fire", "get_name")
  await labcoat.simulate("2:12", "26", [1n, 2, "3"], { height: 5 })
  await labcoat.simulate({ block: 2, tx: 12 }, "get_energy", [], { abi: abiOrganism })
}

export type Checks = [
  Expect<Equal<ContractName, "fire" | "legacy" | "series" | "usd">>,
  Expect<Equal<TypedContractName, "fire" | "series" | "usd">>,
  Expect<
    Equal<
      keyof ReadMethods<typeof abiOrganism>,
      "getEnergy" | "getParent" | "childOf" | "get_count" | "getCount" | "isAlive" | "setFlags"
    >
  >,
  Expect<
    Equal<
      keyof ReadMethods<typeof abiOverwriteTestToken>,
      "initialize" | "forward" | "getName" | "getSymbol" | "getTotalSupply"
    >
  >,
  Expect<Equal<keyof WriteMethods<typeof abiOrganism>, keyof ReadMethods<typeof abiOrganism>>>,
  Expect<
    Equal<
      ReadMethods<AbiDocument>,
      { readonly [x: string]: (...args: AbiArg[]) => Promise<AbiValue> }
    >
  >,
  Expect<
    Equal<
      ReturnType<ContractHandle<typeof abiOrganism>["tryRead"]["getParent"]>,
      Promise<ReadResult<AlkaneRef>>
    >
  >,
  // The decode helpers return what reads return: a bigint, and `AlkaneRef`s.
  Expect<Equal<typeof decodeU128, (bytes: Uint8Array, offset?: number) => bigint>>,
  Expect<Equal<typeof decodeAlkaneId, (bytes: Uint8Array, offset?: number) => AlkaneRef>>,
  Expect<Equal<typeof decodeAlkaneIds, (bytes: Uint8Array) => AlkaneRef[]>>,
  // A summary names its contract only when the call went through a manifest name.
  Expect<Equal<TransactionSummary["contractName"], string | undefined>>,
]
