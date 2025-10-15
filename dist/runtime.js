import { utxo } from "oyl-sdk";
import { setupWallet } from "./wallet.js";
import { deployContract } from "./deploy.js";
import { simulateContract } from "./simulate..js";
import { executeContract } from "./execute.js";
export async function setup() {
    const { config, account, signer, provider } = await setupWallet();
    const wallet = {
        account,
        signer,
        provider,
        utxos: [],
    };
    const defaultOptions = { feeRate: 2 };
    async function fetchUtxos() {
        const { accountUtxos } = await utxo.accountUtxos({ account, provider });
        wallet.utxos = accountUtxos;
    }
    async function deploy(contractName, options = defaultOptions) {
        await fetchUtxos();
        return deployContract(contractName, options, wallet);
    }
    async function simulate(contractName, methodName, args = []) {
        return simulateContract(provider, contractName, methodName, args);
    }
    async function execute(contractName, methodName, args = [], options = defaultOptions) {
        await fetchUtxos();
        return executeContract(contractName, methodName, args, options, wallet);
    }
    return { config, account, provider, signer, deploy, simulate, execute };
}
export const labcoat = { setup };
