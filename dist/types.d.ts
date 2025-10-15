export interface LabcoatConfig {
    network: "oylnet" | "mainnet";
    mnemonic?: string;
    projectId?: string;
}
export type AlkanesPrimitive = "u8" | "u16" | "u32" | "u64" | "u128" | "i8" | "i16" | "i32" | "i64" | "i128" | "String" | "bool" | "Vec<u8>";
export declare enum AlkanesOpcode {
    Initialize = 0,
    MintFrom = 47,
    Mint = 77,
    Name = 99,
    Symbol = 100,
    TotalSupply = 101,
    Data = 1000
}
export type AlkanesType = string;
export interface AlkanesInput {
    name: string;
    type: AlkanesType;
}
export interface AlkanesMethod {
    opcode: AlkanesOpcode | number;
    name: string;
    doc?: string;
    inputs: AlkanesInput[];
    outputs: string[];
}
export interface StorageKey {
    key: string;
    type: AlkanesType;
}
export interface AlkanesABI {
    name: string;
    version: string;
    methods: AlkanesMethod[];
    storage: StorageKey[];
    opcodes: Record<string, number>;
}
export interface AlkaneTransfer {
    id: string;
    value: bigint;
}
export interface CallResponse {
    data: Uint8Array;
    alkanes: {
        transfers: AlkaneTransfer[];
    };
}
export interface ContractConfig {
    abi: AlkanesABI;
    bytecode: string;
    address?: string;
}
