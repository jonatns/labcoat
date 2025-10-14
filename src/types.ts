export interface LabcoatConfig {
  network: "oylnet" | "mainnet";
  mnemonic?: string;
  projectId?: string;
}

export type AlkanesPrimitive =
  | "u8"
  | "u16"
  | "u32"
  | "u64"
  | "u128"
  | "i8"
  | "i16"
  | "i32"
  | "i64"
  | "i128"
  | "String"
  | "bool"
  | "Vec<u8>";

export enum AlkanesOpcode {
  Initialize = 0,
  MintFrom = 47,
  Mint = 77,
  Name = 99,
  Symbol = 100,
  TotalSupply = 101,
  Data = 1000,
}

export type AlkanesType =
  | AlkanesPrimitive
  | { array: { type: AlkanesType; length: number } }
  | { vec: { type: AlkanesType } }
  | { tuple: AlkanesType[] };

export interface AlkanesParam {
  name: string;
  type: AlkanesType;
  components?: AlkanesParam[]; // For complex types
}

// Method definition
export interface AlkanesMethod {
  opcode: AlkanesOpcode | number;
  name: string;
  doc?: string;
  inputs: AlkanesParam[];
  outputs: AlkanesParam[];
}

export interface StorageKey {
  key: string;
  type: AlkanesType;
}

export type AlkanesDeploymentStatus =
  | "not-deployed"
  | "pending"
  | "success"
  | "revert";

export interface AlkanesDeployment {
  status: AlkanesDeploymentStatus;
  txId?: string;
  alkanesId?: string;
  updatedAt?: number;
}

export interface AlkanesABI {
  name: string;
  version: string;
  methods: AlkanesMethod[];
  storage: StorageKey[];
  opcodes: Record<string, number>;
  deployment: AlkanesDeployment;
}

// Response types
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

// Contract instance configuration
export interface ContractConfig {
  abi: AlkanesABI;
  bytecode: string;
  address?: string;
}
