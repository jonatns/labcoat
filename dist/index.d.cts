import * as oyl_sdk from 'oyl-sdk';

interface LabcoatConfig {
    network: "signet" | "mainnet" | "testnet" | "regtest";
    mnemonic?: string;
    projectId?: string;
}
interface TransactionOptions {
    feeRate?: number;
}
type AlkanesPrimitive = "u8" | "u16" | "u32" | "u64" | "u128" | "i8" | "i16" | "i32" | "i64" | "i128" | "String" | "bool" | "Vec<u8>";
declare enum AlkanesOpcode {
    Initialize = 0,
    MintFrom = 47,
    Mint = 77,
    Name = 99,
    Symbol = 100,
    TotalSupply = 101,
    Data = 1000
}
type AlkanesType = string;
interface AlkanesInput {
    name: string;
    type: AlkanesType;
}
interface AlkanesMethod {
    opcode: AlkanesOpcode | number;
    name: string;
    doc?: string;
    inputs: AlkanesInput[];
    outputs: string[];
}
interface StorageKey {
    key: string;
    type: AlkanesType;
}
interface AlkanesABI {
    name: string;
    version: string;
    methods: AlkanesMethod[];
    storage: StorageKey[];
    opcodes: Record<string, number>;
}
interface AlkaneTransfer {
    id: string;
    value: bigint;
}
interface CallResponse {
    data: Uint8Array;
    alkanes: {
        transfers: AlkaneTransfer[];
    };
}
interface ContractConfig {
    abi: AlkanesABI;
    bytecode: string;
    address?: string;
}

declare class AlkanesCompiler {
    private baseDir;
    private cleanupAfter;
    constructor(options?: {
        baseDir?: string;
        cleanup?: boolean;
    });
    private getTempDir;
    compile(contractName: string, sourceCode: string): Promise<{
        wasmBuffer: Buffer;
        abi: AlkanesABI;
    }>;
    private createProject;
    parseABI(sourceCode: string): Promise<AlkanesABI>;
}

declare function loadConfig(): Promise<{
    network: "signet" | "mainnet" | "testnet" | "regtest";
    mnemonic: string;
    projectId: string;
    rpcUrl: string;
}>;

declare function setup(): Promise<{
    config: {
        network: "signet" | "mainnet" | "testnet" | "regtest";
        mnemonic: string;
        projectId: string;
        rpcUrl: string;
    };
    account: oyl_sdk.Account;
    provider: oyl_sdk.Provider;
    signer: oyl_sdk.Signer;
    deploy: (contractName: string, options?: TransactionOptions) => Promise<{
        txId: string;
        alkanesId: string;
        status: any;
    }>;
    simulate: (contractName: string, methodName: string, args?: any[]) => Promise<string | number | bigint>;
    execute: (contractName: string, methodName: string, args?: any[], options?: TransactionOptions) => Promise<{
        frbtcWrapResult: any;
        executeResult: any;
        frbtcUnwrapResult: any;
    }>;
}>;
declare const labcoat: {
    setup: typeof setup;
};

export { type AlkaneTransfer, type AlkanesABI, AlkanesCompiler, type AlkanesInput, type AlkanesMethod, AlkanesOpcode, type AlkanesPrimitive, type AlkanesType, type CallResponse, type ContractConfig, type LabcoatConfig, type StorageKey, type TransactionOptions, labcoat, loadConfig, setup };
