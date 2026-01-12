import { invoke } from "@tauri-apps/api/core";

const RPC_ENDPOINT = "http://localhost:18888";

export interface BlockSummary {
  height: number;
  hash: string;
  time: number;
  txCount: number;
  size: number;
}

export interface BlockDetail extends BlockSummary {
  previousblockhash: string;
  merkleroot: string;
  bits: string;
  difficulty: number;
  noonce: number;
}

export class IsomerRpcClient {
  private static instance: IsomerRpcClient;

  private constructor() {}

  public static getInstance(): IsomerRpcClient {
    if (!IsomerRpcClient.instance) {
      IsomerRpcClient.instance = new IsomerRpcClient();
    }
    return IsomerRpcClient.instance;
  }

  private async call<T>(method: string, params: any[] = []): Promise<T> {
    const id = Date.now();
    const body = JSON.stringify({
      jsonrpc: "2.0",
      method,
      params,
      id,
    });

    try {
      const response = await fetch(RPC_ENDPOINT, {
        method: "POST",
        headers: {
          "Content-Type": "application/json",
        },
        body,
      });

      if (!response.ok) {
        throw new Error(
          `RPC HTTP Error: ${response.status} ${response.statusText}`
        );
      }

      const json = await response.json();

      if (json.error) {
        throw new Error(
          `RPC Error: ${json.error.message || JSON.stringify(json.error)}`
        );
      }

      return json.result as T;
    } catch (error) {
      console.error(`RPC Call Failed [${method}]:`, error);
      throw error;
    }
  }

  async getBlockCount(): Promise<number> {
    return this.call<number>("getblockcount");
  }

  async getBlockHash(height: number): Promise<string> {
    return this.call<string>("getblockhash", [height]);
  }

  async getBlock(hash: string, verbosity: number = 1): Promise<any> {
    return this.call<any>("getblock", [hash, verbosity]);
  }

  /**
   * Fetches the latest N blocks.
   * Uses getblockcount -> getblockhash(height) strategy.
   */
  async getLatestBlocks(limit: number = 10): Promise<BlockSummary[]> {
    try {
      const count = await this.getBlockCount();
      const startHeight = count;
      const endHeight = Math.max(0, count - limit + 1);

      const promises: Promise<BlockSummary>[] = [];

      for (let h = startHeight; h >= endHeight; h--) {
        promises.push(this.fetchBlockByHeight(h));
      }

      return await Promise.all(promises);
    } catch (error) {
      console.error("Failed to fetch latest blocks:", error);
      return [];
    }
  }

  private async fetchBlockByHeight(height: number): Promise<BlockSummary> {
    try {
      const hash = await this.getBlockHash(height);
      const block = await this.getBlock(hash, 1);

      return {
        height: block.height,
        hash: block.hash,
        time: block.time,
        txCount: block.nTx || (block.tx ? block.tx.length : 0),
        size: block.size,
      };
    } catch (error) {
      console.warn(`Failed to fetch block at height ${height}:`, error);
      throw error;
    }
  }
}

export const rpc = IsomerRpcClient.getInstance();
