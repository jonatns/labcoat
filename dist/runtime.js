"use strict";
var __createBinding = (this && this.__createBinding) || (Object.create ? (function(o, m, k, k2) {
    if (k2 === undefined) k2 = k;
    var desc = Object.getOwnPropertyDescriptor(m, k);
    if (!desc || ("get" in desc ? !m.__esModule : desc.writable || desc.configurable)) {
      desc = { enumerable: true, get: function() { return m[k]; } };
    }
    Object.defineProperty(o, k2, desc);
}) : (function(o, m, k, k2) {
    if (k2 === undefined) k2 = k;
    o[k2] = m[k];
}));
var __setModuleDefault = (this && this.__setModuleDefault) || (Object.create ? (function(o, v) {
    Object.defineProperty(o, "default", { enumerable: true, value: v });
}) : function(o, v) {
    o["default"] = v;
});
var __importStar = (this && this.__importStar) || (function () {
    var ownKeys = function(o) {
        ownKeys = Object.getOwnPropertyNames || function (o) {
            var ar = [];
            for (var k in o) if (Object.prototype.hasOwnProperty.call(o, k)) ar[ar.length] = k;
            return ar;
        };
        return ownKeys(o);
    };
    return function (mod) {
        if (mod && mod.__esModule) return mod;
        var result = {};
        if (mod != null) for (var k = ownKeys(mod), i = 0; i < k.length; i++) if (k[i] !== "default") __createBinding(result, mod, k[i]);
        __setModuleDefault(result, mod);
        return result;
    };
})();
var __importDefault = (this && this.__importDefault) || function (mod) {
    return (mod && mod.__esModule) ? mod : { "default": mod };
};
Object.defineProperty(exports, "__esModule", { value: true });
exports.alkali = void 0;
exports.setup = setup;
const promises_1 = __importDefault(require("fs/promises"));
const bitcoin = __importStar(require("bitcoinjs-lib"));
const node_zlib_1 = require("node:zlib");
const node_util_1 = require("node:util");
const oyl_sdk_1 = __importDefault(require("oyl-sdk"));
const token_js_1 = require("oyl-sdk/lib/alkanes/token.js");
const alkanes_1 = require("alkanes");
const config_1 = require("./config");
const gzip = (0, node_util_1.promisify)(node_zlib_1.gzip);
async function setup() {
    const config = await (0, config_1.loadAlkaliConfig)();
    const account = oyl_sdk_1.default.mnemonicToAccount({ mnemonic: config.mnemonic });
    const provider = new oyl_sdk_1.default.Provider({
        url: "https://oylnet.oyl.gg",
        projectId: config.network ?? "oylnet",
        version: "v2",
        network: bitcoin.networks.regtest,
        networkType: "regtest",
    });
    const { accountUtxos } = await oyl_sdk_1.default.utxo.accountUtxos({
        account,
        provider,
    });
    const privateKeys = oyl_sdk_1.default.getWalletPrivateKeys({
        mnemonic: config.mnemonic,
        opts: { network: account.network },
    });
    const signer = new oyl_sdk_1.default.Signer(account.network, {
        taprootPrivateKey: privateKeys.taproot.privateKey,
        segwitPrivateKey: privateKeys.nativeSegwit.privateKey,
        nestedSegwitPrivateKey: privateKeys.nestedSegwit.privateKey,
        legacyPrivateKey: privateKeys.legacy.privateKey,
    });
    async function deploy(contractName) {
        console.log(`🚀 Deploying ${contractName}...`);
        const bytecode = await promises_1.default.readFile(`./build/${contractName}.wasm`);
        const abi = JSON.parse(await promises_1.default.readFile(`./build/${contractName}.abi.json`, "utf8"));
        const payload = {
            body: await gzip(bytecode, { level: 9 }),
            cursed: false,
            tags: { contentType: "" },
        };
        const protostone = (0, alkanes_1.encodeRunestoneProtostone)({
            protostones: [
                alkanes_1.ProtoStone.message({
                    protocolTag: 1n,
                    edicts: [],
                    pointer: 0,
                    refundPointer: 0,
                    calldata: (0, alkanes_1.encipher)([1n, 0n]),
                }),
            ],
        }).encodedRunestone;
        const tx = await (0, token_js_1.inscribePayload)({
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
exports.alkali = { setup };
//# sourceMappingURL=runtime.js.map