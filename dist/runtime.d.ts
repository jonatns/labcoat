import oyl from "oyl-sdk";
export declare function setup(): Promise<{
    config: import("./types.js").LabcoatConfig;
    account: oyl.Account;
    provider: oyl.Provider;
    signer: oyl.Signer;
    deploy: (contractName: string) => Promise<{
        bitcoinTx: {
            commitTx: string;
            txId: string;
            rawTx: string;
            size: any;
            weight: any;
            fee: number;
            satsPerVByte: string;
        };
        alkanesId: string;
        status: any;
    }>;
    simulate: (contract: string, method: string, args: any[]) => Promise<void>;
}>;
export declare const labcoat: {
    setup: typeof setup;
};
