/**
 * Espo Explorer API Client
 * Connects to the local Espo instance at localhost:8081
 */

const ESPO_BASE_URL = "http://localhost:8081";

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

export interface SearchGuessItem {
  label: string;
  value: string;
  href: string | null;
  icon_url: string | null;
  fallback_letter: string | null;
}

export interface SearchGuessGroup {
  kind: string;
  title: string;
  items: SearchGuessItem[];
}

export interface SearchGuessResponse {
  query: string;
  groups: SearchGuessGroup[];
}

// ─────────────────────────────────────────────────────────────────────────────
// Client
// ─────────────────────────────────────────────────────────────────────────────

class EspoClient {
  private static instance: EspoClient;
  private baseUrl: string;

  private constructor(baseUrl: string = ESPO_BASE_URL) {
    this.baseUrl = baseUrl;
  }

  public static getInstance(): EspoClient {
    if (!EspoClient.instance) {
      EspoClient.instance = new EspoClient();
    }
    return EspoClient.instance;
  }

  /**
   * Fetch blocks around a center height with trace counts.
   * @param center - Center block height (defaults to tip)
   * @param radius - Number of blocks on each side (max 50)
   */
  async getCarouselBlocks(
    center?: number,
    radius: number = 10
  ): Promise<CarouselResponse> {
    const params = new URLSearchParams();
    if (center !== undefined) params.set("center", center.toString());
    params.set("radius", radius.toString());

    const url = `${this.baseUrl}/api/blocks/carousel?${params}`;

    try {
      const response = await fetch(url);
      if (!response.ok) {
        throw new Error(
          `Espo API Error: ${response.status} ${response.statusText}`
        );
      }
      return await response.json();
    } catch (error) {
      console.error("[EspoClient] getCarouselBlocks failed:", error);
      throw error;
    }
  }

  /**
   * Search for alkanes, blocks, addresses, or transactions.
   * @param query - Search query string
   */
  async searchGuess(query: string): Promise<SearchGuessResponse> {
    const params = new URLSearchParams({ q: query });
    const url = `${this.baseUrl}/api/search/guess?${params}`;

    try {
      const response = await fetch(url);
      if (!response.ok) {
        throw new Error(
          `Espo API Error: ${response.status} ${response.statusText}`
        );
      }
      return await response.json();
    } catch (error) {
      console.error("[EspoClient] searchGuess failed:", error);
      throw error;
    }
  }

  /**
   * Get the URL to open a block in the full Espo explorer.
   */
  getBlockUrl(height: number): string {
    return `${this.baseUrl}/block/${height}`;
  }

  /**
   * Get the URL to open an alkane in the full Espo explorer.
   */
  getAlkaneUrl(id: string): string {
    return `${this.baseUrl}/alkane/${id}`;
  }

  /**
   * Get the URL to open a transaction in the full Espo explorer.
   */
  getTxUrl(txid: string): string {
    return `${this.baseUrl}/tx/${txid}`;
  }
}

export const espo = EspoClient.getInstance();
