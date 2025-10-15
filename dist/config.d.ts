export declare function loadConfig(): Promise<{
    network: "signet" | "mainnet" | "testnet" | "regtest";
    mnemonic: string;
    projectId: string;
    rpcUrl: string;
}>;
