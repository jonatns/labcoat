import { Account, FormattedUtxo, Provider, Signer } from "oyl-sdk";
export declare function executeContract(contractName: string, methodName: string, args: any[], account: Account, signer: Signer, provider: Provider, utxos: FormattedUtxo[]): Promise<{
    frbtcWrapResult: any;
    executeResult: any;
    frbtcUnwrapResult: any;
}>;
