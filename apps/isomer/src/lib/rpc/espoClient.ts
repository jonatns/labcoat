/**
 * Espo Explorer API Client
 * Uses Tauri commands to fetch data from the local Espo instance
 */

import { invoke } from "@tauri-apps/api/core";

// ─────────────────────────────────────────────────────────────────────────────
// Types
// ─────────────────────────────────────────────────────────────────────────────

export interface CarouselBlock {
  height: number;
  traces: number;
  time: number | null;
}

export interface CarouselResponse {
  espo_tip: number;
  blocks: CarouselBlock[];
}

export interface TransactionInfo {
  txid: string;
  is_trace: boolean;
}

export interface BlockDetails {
  height: number;
  hash: string;
  time: number | null;
  transactions: TransactionInfo[];
}

// ─────────────────────────────────────────────────────────────────────────────
// Client
// ─────────────────────────────────────────────────────────────────────────────

const ESPO_BASE_URL = "http://127.0.0.1:8081";

class EspoClient {
  private static instance: EspoClient;

  private constructor() {}

  public static getInstance(): EspoClient {
    if (!EspoClient.instance) {
      EspoClient.instance = new EspoClient();
    }
    return EspoClient.instance;
  }

  /**
   * Fetch blocks around a center height with trace counts.
   * Uses Tauri command to avoid CORS issues.
   * @param center - Center block height (defaults to tip)
   * @param radius - Number of blocks on each side (max 50)
   */
  async getCarouselBlocks(
    center?: number,
    radius: number = 10
  ): Promise<CarouselResponse> {
    const response = await invoke<CarouselResponse>("get_espo_blocks", {
      center: center ?? null,
      radius,
    });
    return response;
  }

  /**
   * Get the latest block directly from Bitcoin Core for instant updates.
   */
  async getLatestBlock(): Promise<CarouselBlock> {
    return await invoke<CarouselBlock>("get_latest_block");
  }

  /**
   * Get full block details including transactions from Bitcoin Core.
   */
  async getBlockDetails(height: number): Promise<BlockDetails> {
    return await invoke<BlockDetails>("get_block_details", { height });
  }

  /**
   * Get the URL to open a block in the full Espo explorer.
   */
  getBlockUrl(height: number): string {
    return `${ESPO_BASE_URL}/block/${height}`;
  }

  /**
   * Get the URL to open an alkane in the full Espo explorer.
   */
  getAlkaneUrl(id: string): string {
    return `${ESPO_BASE_URL}/alkane/${id}`;
  }

  /**
   * Get the URL to open a transaction in the full Espo explorer.
   */
  getTxUrl(txid: string): string {
    return `${ESPO_BASE_URL}/tx/${txid}`;
  }

  /**
   * Get the base URL for the full Espo explorer.
   */
  getExplorerUrl(): string {
    return ESPO_BASE_URL;
  }
}

export const espo = EspoClient.getInstance();
