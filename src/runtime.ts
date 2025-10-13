import fs from "fs/promises";
import { gzip as _gzip } from "node:zlib";
import { promisify } from "node:util";
import oyl from "oyl-sdk";
import { inscribePayload } from "oyl-sdk/lib/alkanes/token.js";
import { encipher, encodeRunestoneProtostone, ProtoStone } from "alkanes";
import { loadLabcoatConfig } from "./config.js";
import { waitForTrace } from "./helpers.js";

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

  const { accountUtxos } = await oyl.utxo.accountUtxos({
    account,
    provider,
  });

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

    const bytecode = await fs.readFile(`./build/${contractName}.wasm`);
    const abi = JSON.parse(
      await fs.readFile(`./build/${contractName}.abi.json`, "utf8")
    );

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

    const { block: AlkanesTxBlock, tx: alkanesTxId } = await waitForTrace(
      provider,
      bitcoinTx.txId,
      4
    );

    console.log("✅ Contract deployed!");
    console.log(`🔗 TxID: ${bitcoinTx.txId}`);
    console.log(`🔗 Alkanes ID: ${AlkanesTxBlock}:${alkanesTxId}`);

    return bitcoinTx;
  }

  async function simulate(contract: string, method: string, args: any[]) {
    // const [block, tx] = value.split(":").map((part) => part.trim());
    // const request = {
    //   alkanes: options.tokens,
    //   transaction: "0x",
    //   block: "0x",
    //   height: "20000",
    //   txindex: 0,
    //   target: options.target,
    //   inputs: options.inputs,
    //   pointer: 0,
    //   refundPointer: 0,
    //   vout: 0,
    // };
    // await provider.alkanes.simulate(request, decoder);
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
