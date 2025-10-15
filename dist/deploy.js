import fs from "fs/promises";
import { inscribePayload } from "oyl-sdk/lib/alkanes/token.js";
import { encodeRunestoneProtostone, ProtoStone, encipher } from "alkanes";
import { waitForTrace } from "./helpers.js";
import { loadManifest, saveManifest } from "./manifest.js";
import { toAlkanesId } from "./utils.js";
import ora from "ora";
export async function deployContract(contractName, account, signer, provider, utxos) {
    console.log(`🚀 Deploying ${contractName}...`);
    const spinner = ora("Preparing deployment...").start();
    try {
        const buildDir = "./build";
        const wasmBuffer = await fs.readFile(`${buildDir}/${contractName}.wasm.gz`);
        const manifest = await loadManifest();
        manifest[contractName] = manifest[contractName] || {
            abi: "",
            wasm: "",
            deployment: {},
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
            account,
            signer,
            provider,
            utxos,
            feeRate: 0.5,
        });
        spinner.stop();
        console.log(`- 🔗 Tx ID: ${tx.txId}`);
        spinner.start("Waiting for Alkanes traces...");
        const createTrace = await waitForTrace(provider, tx.txId, "create");
        const returnTrace = await waitForTrace(provider, tx.txId, "return");
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
    }
    catch (err) {
        console.log(`- ❌ Deployment failed: ${err}`);
        throw err;
    }
}
