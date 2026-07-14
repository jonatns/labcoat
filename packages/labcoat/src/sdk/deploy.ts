import fs from "fs/promises";
import path from "path";
import ora from "ora";
import { invokeLabcoat } from "./rustBinary.js";
import { loadManifest, saveManifest } from "./manifest.js";
import type { LabcoatWallet } from "./types.js";

export async function deployContract(
  contractName: string,
  options: { feeRate?: number },
  wallet: LabcoatWallet
) {
  console.log(`🚀 Deploying ${contractName}...`);

  const spinner = ora("Preparing deployment...").start();

  try {
    // Deploy wants the RAW .wasm: the reveal envelope gzip-compresses
    // internally, so feeding .wasm.gz would double-compress and produce a
    // contract the indexer can't decode.
    const buildDir = path.resolve("build");
    const wasmPath = path.join(buildDir, `${contractName}.wasm`);
    await fs.access(wasmPath).catch(() => {
      throw new Error(
        `${wasmPath} not found — recompile with a current labcoat (which emits raw .wasm alongside .wasm.gz)`
      );
    });

    spinner.text = "Broadcasting commit/reveal...";
    const result = await invokeLabcoat<{
      txid: string;
      commitTxid?: string;
      alkanesId?: string;
      status: string;
      revertReason?: string;
    }>(
      ["deploy", wasmPath, "--name", contractName],
      { ...wallet.cli, feeRate: options.feeRate }
    );

    spinner.stop();
    console.log(`- 🔗 Tx ID: ${result.txid}`);

    const alkanesId = result.alkanesId ?? "unknown";
    const status = result.status ?? "unknown";

    // labcoat.lock is written by the core; keep the legacy manifest in sync
    // for scripts that still read deployments/manifest.json.
    const manifest = await loadManifest();
    manifest[contractName] = {
      ...(manifest[contractName] || {}),
      deployment: {
        status,
        txId: result.txid,
        alkanesId,
        deployedAt: Date.now(),
      },
    };
    await saveManifest(manifest);

    console.log(`- 📊 Deployment status: ${status}`);
    console.log(`- ⚛️ Alkane ID: ${alkanesId}`);

    return { txId: result.txid, alkanesId, status };
  } catch (err) {
    spinner.stop();
    console.log(`- ❌ Deployment failed: ${err}`);
    throw err;
  }
}
