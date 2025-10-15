import { Provider } from "oyl-sdk";
export declare function setupWallet(): Promise<{
    config: {
        network: "signet" | "mainnet" | "testnet" | "regtest";
        mnemonic: string;
        projectId: string;
        rpcUrl: string;
    };
    account: import("oyl-sdk").Account;
    signer: import("oyl-sdk").Signer;
    provider: Provider;
}>;
