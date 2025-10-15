import fs from "fs/promises";
import { gzip as _gzip } from "node:zlib";
import { promisify } from "node:util";
import oyl from "oyl-sdk";
import { inscribePayload } from "oyl-sdk/lib/alkanes/token.js";
import { encipher, encodeRunestoneProtostone, ProtoStone } from "alkanes";
import { decodeRevertReason, encodeArgs, waitForTrace } from "./helpers.js";
import { loadLabcoatConfig } from "./config.js";
const gzip = promisify(_gzip);
const MANIFEST_PATH = "./deployments/manifest.json";
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
    // Helper to load or create manifest
    async function loadManifest() {
        try {
            const data = await fs.readFile(MANIFEST_PATH, "utf8");
            return JSON.parse(data);
        }
        catch {
            return {};
        }
    }
    async function saveManifest(manifest) {
        await fs.mkdir("./deployments", { recursive: true });
        await fs.writeFile(MANIFEST_PATH, JSON.stringify(manifest, null, 2));
    }
    async function deploy(contractName) {
        console.log(`🚀 Deploying ${contractName} contract...`);
        const buildDir = "./build";
        const wasmPath = `${buildDir}/${contractName}.wasm`;
        const abiPath = `${buildDir}/${contractName}.abi.json`;
        const bytecode = await fs.readFile(wasmPath);
        const abi = JSON.parse(await fs.readFile(abiPath, "utf8"));
        const manifest = await loadManifest();
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
        // Prepare inscribe payload
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
        await saveManifest(manifest);
        console.log(`📝 Manifest updated with pending deployment`);
        console.log(`🔗 Bitcoin Tx ID: ${bitcoinTx.txId}`);
        // Wait for Alkanes trace
        console.log("⏳ Waiting for Alkanes traces...");
        const createTrace = await waitForTrace(provider, bitcoinTx.txId, "create");
        const returnTrace = await waitForTrace(provider, bitcoinTx.txId, "return");
        const alkanesId = `${Number(createTrace.data.block)}:${Number(createTrace.data.tx)}`;
        const status = returnTrace?.data?.status ?? "unknown";
        if (status === "revert") {
            const hexData = returnTrace?.data?.response?.data ?? "0x";
            const revertReason = decodeRevertReason(hexData);
            console.warn(`⚠️ Revert reason: ${revertReason}`);
        }
        // Update manifest with final deployment
        manifest[contractName].deployment = {
            status,
            txId: bitcoinTx.txId,
            alkanesId,
            deployedAt: Date.now(),
        };
        await saveManifest(manifest);
        if (status === "success") {
            console.log(`✅ Contract deployed successfully!`);
            console.log(`🔗 Alkanes ID: ${alkanesId}`);
        }
        else if (status === "revert") {
            console.warn(`⚠️ Deployment reverted!`);
        }
        else {
            console.warn(`⚠️ Deployment status unknown`);
        }
        return { bitcoinTx, alkanesId, status };
    }
    async function simulate(contractName, methodName, args = []) {
        const manifest = await loadManifest();
        const contractInfo = manifest[contractName];
        if (!contractInfo)
            throw new Error(`Contract ${contractName} not found in manifest`);
        const abi = JSON.parse(await fs.readFile(contractInfo.abi, "utf8"));
        const normalizedMethod = methodName
            .replace(/([A-Z])/g, "_$1")
            .toLowerCase()
            .replace(/^_/, ""); // DoSomething -> do_something
        const method = abi.methods.find((m) => m.name.toLowerCase() === normalizedMethod.toLowerCase());
        if (!method)
            throw new Error(`Method ${methodName} not found in ABI of ${contractName}`);
        const [block, tx] = contractInfo.deployment.alkanesId
            .split(":")
            .map((p) => p.trim());
        const encodedArgs = encodeArgs(args);
        const request = {
            alkanes: [],
            transaction: "0x",
            block: "0x",
            height: "20000",
            txindex: 0,
            target: { block, tx },
            inputs: [method.opcode.toString(), ...encodedArgs],
            pointer: 0,
            refundPointer: 0,
            vout: 0,
        };
        const simulationResult = await provider.alkanes.simulate(request);
        console.log(`🧪 Simulation result:`, simulationResult);
        return simulationResult;
    }
    return { config, account, provider, signer, deploy, simulate };
}
export const labcoat = { setup };
