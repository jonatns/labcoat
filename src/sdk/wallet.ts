import { loadConfig } from "./config.js";
import { setupAccount } from "./account.js";
import { Network, Provider } from "@oyl/sdk";
import * as bitcoin from "bitcoinjs-lib";

function getBitcoinNetwork(network: Network): bitcoin.networks.Network {
  switch (network) {
    case "mainnet":
      return bitcoin.networks.bitcoin;
    case "testnet":
      return bitcoin.networks.testnet;
    case "signet":
      return bitcoin.networks.testnet;
    case "regtest":
      return bitcoin.networks.regtest;
    case "oylnet":
      return bitcoin.networks.regtest;
    default:
      throw new Error(`Unsupported network: ${network}`);
  }
}

export async function setupWallet() {
  const config = await loadConfig();
  const bitcoinNetwork = getBitcoinNetwork(config.network);
  const { account, signer } = setupAccount(config.mnemonic, bitcoinNetwork);
  const provider = new Provider({
    version: "v2",
    url: config.rpcUrl,
    projectId: config.projectId ?? "regtest",
    network: bitcoinNetwork,
    networkType: config.network,
  });

  return { config, account, signer, provider };
}
