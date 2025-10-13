import oyl from "oyl-sdk";
export declare function setup(): Promise<{
    config: import("./types.js").LabcoatConfig;
    account: oyl.Account;
    provider: oyl.Provider;
    signer: oyl.Signer;
    deploy: (contractName: string) => Promise<{
        commitTx: string;
        txId: string;
        rawTx: string;
        size: any;
        weight: any;
        fee: number;
        satsPerVByte: string;
    }>;
}>;
export declare const labcoat: {
    setup: typeof setup;
};
