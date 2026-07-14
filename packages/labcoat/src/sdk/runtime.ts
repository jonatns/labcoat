import { utxo } from "@oyl/sdk";
import { setupWallet } from "./wallet.js";
import { deployContract } from "./deploy.js";
import { simulateContract } from "./simulate..js";
import { executeContract } from "./execute.js";
import { TransactionOptions } from "./types.js";

export async function setup() {
  const { config, account, signer, provider } = await setupWallet();
  const wallet = {
    account,
    signer,
    provider,
    utxos: [],
  };
  const defaultOptions: TransactionOptions = { feeRate: 2 };

  async function fetchUtxos() {
    const { accountUtxos } = await utxo.accountUtxos({ account, provider });
    wallet.utxos = accountUtxos;
  }

  async function deploy(
    contractName: string,
    options: TransactionOptions = defaultOptions
  ) {
    await fetchUtxos();
    return deployContract(contractName, options, wallet);
  }

  async function simulate(
    contractName: string,
    methodName: string,
    args: any[] = []
  ) {
    return simulateContract(provider, contractName, methodName, args);
  }

  async function execute(
    contractName: string,
    methodName: string,
    args: any[] = [],
    options: TransactionOptions = defaultOptions
  ) {
    await fetchUtxos();
    return executeContract(contractName, methodName, args, options, wallet);
  }

  return { config, account, provider, signer, deploy, simulate, execute };
}

export const labcoat = { setup };
