import { FormattedUtxo, utxo } from "oyl-sdk";
import { setupWallet } from "./wallet.js";
import { deployContract } from "./deploy.js";
import { simulateContract } from "./simulate..js";
import { executeContract } from "./execute.js";

export async function setup() {
  let utxos: FormattedUtxo[];
  const { config, account, signer, provider } = await setupWallet();

  async function fetchUtxos() {
    const { accountUtxos } = await utxo.accountUtxos({ account, provider });
    utxos = accountUtxos;
  }

  async function deploy(contractName: string) {
    await fetchUtxos();
    return deployContract(contractName, account, signer, provider, utxos);
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
    await fetchUtxos();
    return executeContract(
      contractName,
      methodName,
      args,
      account,
      signer,
      provider,
      utxos
    );
  }

  return { config, account, provider, signer, deploy, simulate, execute };
}

export const labcoat = { setup };
