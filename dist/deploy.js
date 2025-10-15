import fs from "fs/promises";
import { inscribePayload } from "oyl-sdk/lib/alkanes/token.js";
import { encodeRunestoneProtostone, ProtoStone, encipher } from "alkanes";
import { waitForTrace } from "./helpers.js";
import { loadManifest, saveManifest } from "./manifest.js";
import { toAlkanesId } from "./utils.js";
export async function deployContract(contractName, account, signer, provider, utxos) {
    console.log(`🚀 Deploying ${contractName}...`);
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
    const tx = await inscribePayload({
        protostone,
        payload,
        account,
        signer,
        provider,
        utxos,
        feeRate: 2,
    });
    console.log(`🔗 Tx ID: ${tx.txId}`);
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
    console.log(`📊 Deployment ${status.toUpperCase()}: Alkanes ID ${alkanesId}`);
    return { txId: tx.txId, alkanesId, status };
}
