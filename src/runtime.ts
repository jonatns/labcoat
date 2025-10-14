import fs from "fs/promises";
import { gzip as _gzip } from "node:zlib";
import { promisify } from "node:util";
import oyl from "oyl-sdk";
import { inscribePayload } from "oyl-sdk/lib/alkanes/token.js";
import { encipher, encodeRunestoneProtostone, ProtoStone } from "alkanes";
import { decodeRevertReason, waitForTrace } from "./helpers.js";
import { loadLabcoatConfig } from "./config.js";

const gzip = promisify(_gzip);
const MANIFEST_PATH = "./deployments/manifest.json";

export async function setup() {
  const config = await loadLabcoatConfig();
  const url =
    config.network === "oylnet"
      ? "https://oylnet.oyl.gg"
      : "https://mainnet.sandshrew.io";
  const networkType = config.network === "oylnet" ? "regtest" : config.network;
  const network = oyl.getNetwork(networkType);
  const projectId = config.projectId ?? "regtest";

  const provider = new oyl.Provider({
    version: "v2",
    url,
    projectId,
    network,
    networkType,
  });

  const account = oyl.mnemonicToAccount({
    mnemonic: config.mnemonic,
    opts: { network },
  });

  const { accountUtxos } = await oyl.utxo.accountUtxos({ account, provider });

  const privateKeys = oyl.getWalletPrivateKeys({
    mnemonic: config.mnemonic!,
    opts: { network: account.network },
  });

  const signer = new oyl.Signer(account.network, {
    taprootPrivateKey: privateKeys.taproot.privateKey,
    segwitPrivateKey: privateKeys.nativeSegwit.privateKey,
    nestedSegwitPrivateKey: privateKeys.nestedSegwit.privateKey,
    legacyPrivateKey: privateKeys.legacy.privateKey,
  });

  async function deploy(contractName: string) {
    console.log(`🚀 Deploying ${contractName}...`);

    const buildDir = "./build";
    const wasmPath = `${buildDir}/${contractName}.wasm`;
    const abiPath = `${buildDir}/${contractName}.abi.json`;

    const bytecode = await fs.readFile(wasmPath);
    const abi = JSON.parse(await fs.readFile(abiPath, "utf8"));

    // Load or create manifest
    let manifest: Record<string, any> = {};
    try {
      manifest = JSON.parse(await fs.readFile(MANIFEST_PATH, "utf8"));
    } catch {
      manifest = {};
    }

    manifest[contractName] = manifest[contractName] || {
      abi: abiPath,
      wasm: wasmPath,
      deployment: {
        status: "pending",
        txId: null,
        alkanesId: null,
        deployedAt: Date.now(),
      },
    };

    const payload = {
      body: await gzip(bytecode, { level: 9 }),
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

    const bitcoinTx = await inscribePayload({
      protostone,
      payload,
      account,
      provider,
      signer,
      utxos: accountUtxos,
      feeRate: 2,
    });

    // Update manifest with pending deployment
    manifest[contractName].deployment = {
      status: "pending",
      txId: bitcoinTx.txId,
      alkanesId: null,
      deployedAt: Date.now(),
    };
    await fs.mkdir("./deployments", { recursive: true });
    await fs.writeFile(MANIFEST_PATH, JSON.stringify(manifest, null, 2));
    console.log(`📝 Manifest updated with pending deployment`);
    console.log(`🔗 Bitcoin Tx ID: ${bitcoinTx.txId}`);

    console.log("⏳ Waiting for Alkanes create trace...");
    const createTrace = await waitForTrace(
      provider,
      bitcoinTx.txId,
      4,
      "create"
    );
    const returnTrace = await waitForTrace(
      provider,
      bitcoinTx.txId,
      4,
      "return"
    );

    const alkanesId = `${Number(createTrace.data.block)}:${Number(
      createTrace.data.tx
    )}`;
    const status = returnTrace?.data?.status ?? "unknown";

    if (status === "revert") {
      const hexData = returnTrace?.data?.response?.data ?? "0x";
      const revertReason = decodeRevertReason(hexData);
      console.warn(`⚠️ Revert reason: ${revertReason}`);
    }

    manifest[contractName].deployment = {
      status,
      txId: bitcoinTx.txId,
      alkanesId,
      deployedAt: Date.now(),
    };
    await fs.writeFile(MANIFEST_PATH, JSON.stringify(manifest, null, 2));

    if (status === "success") {
      console.log(`✅ Contract deployed successfully!`);
      console.log(`🔗 Alkanes ID: ${alkanesId}`);
    } else if (status === "revert") {
      console.warn(`⚠️ Deployment reverted!`);
    } else {
      console.warn(`⚠️ Deployment status unknown`);
    }

    return {
      bitcoinTx,
      alkanesId,
      status,
    };
  }

  return {
    config,
    account,
    provider,
    signer,
    deploy,
  };
}

export const labcoat = { setup };
