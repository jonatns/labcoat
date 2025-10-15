export declare function setup(): Promise<{
    config: {
        network: "signet" | "mainnet" | "testnet" | "regtest";
        mnemonic: string;
        projectId: string;
        rpcUrl: string;
    };
    account: import("oyl-sdk").Account;
    provider: import("oyl-sdk").Provider;
    signer: import("oyl-sdk").Signer;
    deploy: (contractName: string) => Promise<{
        txId: string;
        alkanesId: string;
        status: any;
    }>;
    simulate: (contractName: string, methodName: string, args?: any[]) => Promise<string | number | bigint>;
    execute: (contractName: string, methodName: string, args?: any[]) => Promise<{
        frbtcWrapResult: any;
        executeResult: any;
        frbtcUnwrapResult: any;
    }>;
}>;
export declare const labcoat: {
    setup: typeof setup;
};
