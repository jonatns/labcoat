// End-to-end check of wallet-signed writes, from Node, against a running
// Labcoat Network with the counter template deployed. Run by
// scripts/tests/generate-web-write-e2e.sh, which generates `./generated`
// from that project and passes the endpoint in LABCOAT_RPC_URL.

import { test } from "node:test"
import { strict as assert } from "node:assert"

import { ContractRevertError, WalletRejectedError, createClient } from "./generated/client"
import type { AbiDocument } from "./generated/abi"

const rpc = process.env["LABCOAT_RPC_URL"]
if (rpc === undefined) throw new Error("LABCOAT_RPC_URL must name the Labcoat Network endpoint")
const labcoat = createClient({ rpc })
const counter = labcoat.contract("counter")

test("the dev wallet sends a write, and the new value reads back", async () => {
  const wallet = await labcoat.connect()
  assert.equal(wallet.name, "Labcoat dev wallet")
  const before = await counter.read.getCount()

  const tx = await counter.write.increment({ wallet })
  assert.match(tx.txid, /^[0-9a-f]{64}$/)
  const receipt = await tx.wait()
  assert.equal(receipt.status, "success")
  assert.ok(receipt.gasUsed > 0n)
  assert.equal(await counter.read.getCount(), before + 1n)

  // By transaction id alone.
  const bare = await labcoat.waitForTransaction(tx.txid)
  assert.equal(bare.status, "success")

  // A second write spends the change of the first.
  const second = await counter.write.increment({ wallet })
  await second.wait()
  assert.equal(await counter.read.getCount(), before + 2n)
})

test("a payment output rides along with a write", async () => {
  const wallet = await labcoat.connect()
  const [account] = await wallet.getAccounts()
  assert.ok(account)
  const tx = await counter.write.increment({ wallet, pay: [{ address: account.address, sats: 12_345 }] })
  assert.deepEqual(tx.payments, [{ output: 1, address: account.address, sats: 12_345n }])
  await tx.wait()
})

// The counter with a method it does not have: the call reverts.
const abiBroken = {
  contract: "Counter",
  methods: [{ name: "missing", opcode: 4040n, params: [], returns: "void" }],
} as const satisfies AbiDocument

test("a write that reverts is caught before signing, and reported when mined", async () => {
  const wallet = await labcoat.connect()
  const broken = labcoat.contract(counter.id, abiBroken)
  const before = await counter.read.getCount()

  // Simulated first: nothing is signed or broadcast.
  await assert.rejects(broken.write.missing({ wallet }), (error: unknown) => {
    assert.ok(error instanceof ContractRevertError)
    assert.equal(error.txid, null)
    return true
  })

  // Sent anyway: the transaction is mined, and waiting for it throws.
  const tx = await broken.write.missing({ wallet, simulate: false })
  let simulated = ""
  try {
    await broken.read.missing()
  } catch (error) {
    simulated = error instanceof ContractRevertError ? error.reason : ""
  }
  await assert.rejects(tx.wait(), (error: unknown) => {
    assert.ok(error instanceof ContractRevertError)
    assert.equal(error.txid, tx.txid)
    assert.equal(error.contract, counter.id)
    assert.equal(error.method, "missing")
    // The reason is the one a read of the same call reports.
    assert.ok(error.reason.length > 0)
    assert.equal(error.reason, simulated)
    return true
  })
  await assert.rejects(labcoat.waitForTransaction(tx.txid), ContractRevertError)
  assert.equal(await counter.read.getCount(), before)
})

test("rejecting in the dev wallet's approval step sends nothing", async () => {
  const wallet = await labcoat.connect({ approve: () => false })
  const before = await counter.read.getCount()
  await assert.rejects(counter.write.increment({ wallet }), WalletRejectedError)
  assert.equal(await counter.read.getCount(), before)
})

test("without mining, a write stays pending until a block is asked for", async () => {
  const wallet = await labcoat.connect({ mine: false })
  const tx = await counter.write.increment({ wallet })
  await assert.rejects(tx.wait({ timeoutMs: 1_500 }), /TRANSACTION_TIMEOUT|no result/)
  const [account] = await wallet.getAccounts()
  const response = await fetch(rpc, {
    method: "POST",
    body: JSON.stringify({ jsonrpc: "2.0", id: 1, method: "generatetoaddress", params: [1, account?.address] }),
  })
  assert.ok(response.ok)
  assert.equal((await tx.wait()).status, "success")
})
