import fs from "fs/promises";
import { gzip as _gzip } from "node:zlib";
import { promisify } from "node:util";
import oyl from "oyl-sdk";
import { inscribePayload } from "oyl-sdk/lib/alkanes/token.js";
import { encipher, encodeRunestoneProtostone, ProtoStone } from "alkanes";
import { loadLabcoatConfig } from "./config.js";
const gzip = promisify(_gzip);
export async function setup() {
    const config = await loadLabcoatConfig();
    const networkType = config.network === "oylnet" ? "regtest" : config.network;
    const network = oyl.getNetwork(networkType);
    const projectId = config.projectId ?? "regtest";
    const provider = new oyl.Provider({
        url: "https://oylnet.oyl.gg",
        version: "v2",
        projectId,
        network,
        networkType,
    });
    const account = oyl.mnemonicToAccount({
        mnemonic: config.mnemonic,
        opts: {
            network,
        },
    });
    console.log("account", account);
    const { accountUtxos } = await oyl.utxo.accountUtxos({
        account,
        provider,
    });
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
        const bytecode = await fs.readFile(`./build/${contractName}.wasm`);
        const abi = JSON.parse(await fs.readFile(`./build/${contractName}.abi.json`, "utf8"));
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
        const tx = await inscribePayload({
            protostone,
            payload,
            account,
            provider,
            signer,
            utxos: accountUtxos,
            feeRate: 2,
        });
        console.log("✅ Contract deployed!");
        console.log(`🔗 TxID: ${tx.txId}`);
        return tx;
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
