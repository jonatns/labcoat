import fs from "fs/promises";
import path from "path";
import { gzip as _gzip } from "node:zlib";
import { promisify } from "node:util";
import oyl from "oyl-sdk";
import { inscribePayload } from "oyl-sdk/lib/alkanes/token.js";
import { encipher, encodeRunestoneProtostone, ProtoStone } from "alkanes";
import { loadLabcoatConfig } from "./config.js";
import { decodeRevertReason, waitForTrace } from "./helpers.js";
const gzip = promisify(_gzip);
export async function setup() {
    const config = await loadLabcoatConfig();
    const url = config.network === "oylnet"
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
        mnemonic: config.mnemonic,
        opts: { network: account.network },
    });
    const signer = new oyl.Signer(account.network, {
        taprootPrivateKey: privateKeys.taproot.privateKey,
        segwitPrivateKey: privateKeys.nativeSegwit.privateKey,
        nestedSegwitPrivateKey: privateKeys.nestedSegwit.privateKey,
        legacyPrivateKey: privateKeys.legacy.privateKey,
    });
    async function deploy(contractName) {
        console.log(`🚀 Deploying ${contractName}...`);
        const buildDir = path.resolve("./build");
        const wasmPath = path.join(buildDir, `${contractName}.wasm`);
        const abiPath = path.join(buildDir, `${contractName}.abi.json`);
        const bytecode = await fs.readFile(wasmPath);
        const abi = JSON.parse(await fs.readFile(abiPath, "utf8"));
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
        console.log(`🔗 TxID: ${bitcoinTx.txId}`);
        abi.deployment = {
            ...abi.deployment,
            txid: bitcoinTx.txId,
            status: "pending",
            updatedAt: Date.now(),
        };
        await fs.writeFile(abiPath, JSON.stringify(abi, null, 2));
        console.log(`📝 ABI updated with pending deployment`);
        console.log("⏳ Waiting for Alkanes trace...");
        const trace = await waitForTrace(provider, bitcoinTx.txId, 4);
        const createEvent = trace.find(({ event }) => event === "create");
        const returnEvent = trace.find(({ event }) => event === "return");
        if (!createEvent) {
            console.error("❌ No create event found in trace.");
            Object.assign(abi.deployment, {
                status: "failed",
                error: "No create event found",
                updatedAt: Date.now(),
            });
            await fs.writeFile(abiPath, JSON.stringify(abi, null, 2));
            return;
        }
        const alkanesBlock = Number(createEvent.data.block);
        const alkanesTx = Number(createEvent.data.tx);
        const alkanesId = `${alkanesBlock}:${alkanesTx}`;
        const status = returnEvent?.data?.status ?? "unknown";
        let revertReason;
        if (status === "revert" && returnEvent?.data?.response?.data) {
            revertReason = decodeRevertReason(returnEvent.data.response.data);
        }
        Object.assign(abi.deployment, {
            alkanesId,
            status,
            updatedAt: Date.now(),
        });
        if (status === "success") {
            console.log("✅ Contract deployed successfully!");
            console.log(`🔗 Alkanes ID: ${alkanesId}`);
        }
        else if (status === "revert") {
            console.warn(`⚠️ Deployment reverted.`);
            if (revertReason) {
                console.warn(`💥 Revert reason: ${revertReason}`);
                abi.deployment.revertReason = revertReason;
            }
        }
        else {
            console.warn(`⚠️ Deployment ended with status: ${status}`);
        }
        await fs.writeFile(abiPath, JSON.stringify(abi, null, 2));
        console.log(`🧱 ABI updated with final status: ${status}`);
        return { bitcoinTx, alkanesId, status };
    }
    async function simulate(contract, method, args) {
        // Placeholder for simulation API integration
    }
    return {
        config,
        account,
        provider,
        signer,
        deploy,
        simulate,
    };
}
export const labcoat = { setup };
