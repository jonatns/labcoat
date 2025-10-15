import { Account, FormattedUtxo, Provider, Signer } from "oyl-sdk";
export declare function deployContract(contractName: string, options: {
    feeRate?: number;
}, wallet: {
    account: Account;
    signer: Signer;
    provider: Provider;
    utxos: FormattedUtxo[];
}): Promise<{
    txId: string;
    alkanesId: string;
    status: any;
}>;
