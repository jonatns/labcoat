import { invoke } from "@tauri-apps/api/core";
import type { SystemStatus, Account, BinaryInfo, IsomerConfig } from "./types";

/**
 * Isomer API client - wraps Tauri commands
 */
export const api = {
  /**
   * Get current system status
   */
  getStatus: () => invoke<SystemStatus>("get_status"),

  /**
   * Start all services
   */
  startServices: () => invoke<void>("start_services"),

  /**
   * Stop all services
   */
  stopServices: () => invoke<void>("stop_services"),

  /**
   * Mine blocks
   */
  mineBlocks: (count: number, address?: string) =>
    invoke<number>("mine_blocks", { count, address }),

  /**
   * Get pre-funded accounts
   */
  getAccounts: () => invoke<Account[]>("get_accounts"),

  /**
   * Check binary installation status
   */
  checkBinaries: () => invoke<BinaryInfo[]>("check_binaries"),

  /**
   * Download missing binaries
   */
  downloadBinaries: () => invoke<void>("download_binaries"),

  /**
   * Get current configuration
   */
  getConfig: () => invoke<IsomerConfig>("get_config"),

  /**
   * Update configuration
   */
  updateConfig: (config: IsomerConfig) =>
    invoke<void>("update_config", { config }),
};

export default api;
