// Isomer API types - mirrors Rust types from src-tauri

export type ServiceStatus =
  | "running"
  | "starting"
  | "stopped"
  | { error: string };

export interface ServiceInfo {
  id: string;
  name: string;
  status: ServiceStatus;
  port: number;
  pid: number | null;
  uptime_secs: number | null;
  version: string | null;
}

export interface SystemStatus {
  services: ServiceInfo[];
  block_height: number;
  mempool_size: number;
  is_ready: boolean;
}

export interface LogEntry {
  service: string;
  timestamp: number;
  message: string;
  is_stderr: boolean;
}

export interface Account {
  index: number;
  address: string;
  private_key: string;
  balance_sats: number;
}

export type BinaryStatus =
  | "notinstalled"
  | { downloading: { progress: number } }
  | { installed: { version: string } }
  | { updateavailable: { current: string; latest: string } };

export interface BinaryInfo {
  service: string;
  status: BinaryStatus;
  path: string;
  size_bytes: number | null;
}

export interface PortConfig {
  bitcoind_rpc: number;
  bitcoind_p2p: number;
  metashrew: number;
  memshrew: number;
  ord: number;
  esplora_http: number;
  esplora_electrum: number;
  jsonrpc: number;
}

export interface BitcoindConfig {
  rpc_user: string;
  rpc_password: string;
  fallback_fee: number;
}

export interface MiningConfig {
  auto_mine: boolean;
  block_interval_ms: number;
  initial_blocks: number;
}

export interface IsomerConfig {
  ports: PortConfig;
  bitcoind: BitcoindConfig;
  mining: MiningConfig;
  mnemonic: string | null;
}
