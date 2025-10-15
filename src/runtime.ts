import { utxo } from "oyl-sdk";
import { setupWallet } from "./wallet.js";
import { deployContract } from "./deploy.js";
import { simulateContract } from "./simulate..js";
import { executeContract } from "./execute.js";

export async function setup() {
  const { config, account, signer, provider } = await setupWallet();

  const { accountUtxos } = await utxo.accountUtxos({ account, provider });

  async function deploy(contractName: string) {
    return deployContract(
      contractName,
      account,
      signer,
      provider,
      accountUtxos
    );
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
    args: any[] = []
  ) {
    return executeContract(
      contractName,
      methodName,
      args,
      account,
      signer,
      provider,
      accountUtxos
    );
  }

  return { config, account, provider, signer, deploy, simulate, execute };
}

export const labcoat = { setup };
