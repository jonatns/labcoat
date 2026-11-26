// Compile-time checks for the client `labcoat generate web` writes. Nothing
// here runs: `tsc` passing is the test. Each `@ts-expect-error` line is a
// mistake the generated types must reject, and fails the build if they
// stop rejecting it.

import { createClient } from "../expected/client"
import type {
  AbiArg,
  AbiValue,
  AlkaneRef,
  ContractHandle,
  ContractRevertError,
  ReadMethods,
  ReadResult,
  TypedContractName,
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
]
