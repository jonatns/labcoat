import fs from "fs/promises";
import { inscribePayload } from "@oyl/sdk/lib/alkanes/token.js";
import { encodeRunestoneProtostone, ProtoStone, encipher } from "alkanes";
import { waitForTrace } from "./helpers.js";
import { loadManifest, saveManifest } from "./manifest.js";
import { toAlkanesId } from "../utils/alkanes.js";
import { Account, FormattedUtxo, Provider, Signer } from "@oyl/sdk";
import ora from "ora";
import path from "path";

export async function deployContract(
  contractName: string,
  options: { feeRate?: number },
  wallet: {
    account: Account;
    signer: Signer;
    provider: Provider;
    utxos: FormattedUtxo[];
  }
) {
  console.log(`🚀 Deploying ${contractName}...`);

  const spinner = ora("Preparing deployment...").start();

  try {
    const buildDir = path.resolve("build");
    const wasmPath = path.join(buildDir, `${contractName}.wasm.gz`);
    const wasmBuffer = await fs.readFile(wasmPath);

    const manifest = await loadManifest();
    manifest[contractName] = {
      ...(manifest[contractName] || {}),
      deployment: manifest[contractName]?.deployment || {},
    };

    const payload = {
      body: wasmBuffer,
      cursed: false,
      tags: { contentType: "" },
    };

    const protostone = encodeRunestoneProtostone({
      protostones: [
        ProtoStone.message({
          protocolTag: 1n,
          edicts: [],
          pointer: 0,
          refundPointer: 0,
          calldata: encipher([1n, 0n]),
        }),
      ],
    }).encodedRunestone;

    spinner.text = "Broadcasting transaction...";
    const tx = await inscribePayload({
      protostone,
      payload,
      feeRate: options.feeRate,
      ...wallet,
    });

    spinner.stop();

    console.log(`- 🔗 Tx ID: ${tx.txId}`);

    spinner.start("Waiting for Alkanes traces...");
    const createTrace = await waitForTrace(wallet.provider, tx.txId, "create");
    const returnTrace = await waitForTrace(wallet.provider, tx.txId, "return");

    const alkanesId = toAlkanesId(createTrace.data);
    const status = returnTrace?.data?.status ?? "unknown";

    manifest[contractName].deployment = {
      status,
      txId: tx.txId,
      alkanesId,
      deployedAt: Date.now(),
    };
    await saveManifest(manifest);

    spinner.stop();
    console.log(`- 📊 Deployment status: ${status}`);
    console.log(`- ⚛️ Alkane ID: ${alkanesId}`);

    return { txId: tx.txId, alkanesId, status };
  } catch (err) {
    console.log(`- ❌ Deployment failed: ${err}`);
    throw err;
  }
}
