/**
 * Inpage Provider
 *
 * Injected into web pages to provide window.alkanes API
 * Similar to Sats Connect / UniSat API for easy dApp integration
 */

interface RequestArgs {
  method: string;
  params?: unknown;
}

interface PsbtSigningOptions {
  autoFinalized?: boolean;
  toSignInputs?: Array<{
    index: number;
    address?: string;
    sighashTypes?: number[];
  }>;
}

interface Account {
  address: string;
  publicKey: string;
  addressType: string;
}

// Pending requests
const pendingRequests = new Map<
  string,
  { resolve: (value: unknown) => void; reject: (error: Error) => void }
>();

// Generate unique request ID
function generateId(): string {
  return `${Date.now()}-${Math.random().toString(36).slice(2, 11)}`;
}

// Send message to content script
function sendMessage(type: string, payload?: unknown): Promise<unknown> {
  return new Promise((resolve, reject) => {
    const id = generateId();
    pendingRequests.set(id, { resolve, reject });

    window.postMessage(
      {
        target: "isomer-companion-content",
        type,
        payload,
        id,
      },
      "*"
    );

    // Timeout after 60 seconds
    setTimeout(() => {
      if (pendingRequests.has(id)) {
        pendingRequests.delete(id);
        reject(new Error("Request timeout"));
      }
    }, 60000);
  });
}

// Handle responses from content script
window.addEventListener("message", (event) => {
  if (event.source !== window || !event.data) return;
  if (event.data.target !== "isomer-companion-inpage") return;

  const { id, result, error, type } = event.data;

  // Handle provider ready event
  if (type === "PROVIDER_READY") {
    console.log("[Isomer Companion] Provider ready");
    return;
  }

  // Handle responses
  const pending = pendingRequests.get(id);
  if (pending) {
    pendingRequests.delete(id);
    if (error) {
      pending.reject(new Error(error));
    } else {
      pending.resolve(result);
    }
  }
});

// Provider API
const alkanes = {
  /**
   * Check if wallet is installed and available
   */
  isInstalled: true,

  /**
   * Request connection to wallet
   * Returns connected accounts
   */
  requestAccounts: async (): Promise<string[]> => {
    const accounts = (await sendMessage("CONNECT")) as Account[];
    return accounts.map((a) => a.address);
  },

  /**
   * Get connected accounts
   */
  getAccounts: async (): Promise<string[]> => {
    const accounts = (await sendMessage("GET_ACCOUNTS")) as Account[];
    return accounts.map((a) => a.address);
  },

  /**
   * Get public key (x-only for taproot)
   */
  getPublicKey: async (): Promise<string> => {
    return sendMessage("GET_PUBLIC_KEY") as Promise<string>;
  },

  /**
   * Get current network (always 'regtest' for Isomer)
   */
  getNetwork: async (): Promise<string> => {
    return "regtest";
  },

  /**
   * Sign a message
   */
  signMessage: async (message: string, address?: string): Promise<string> => {
    return sendMessage("SIGN_MESSAGE", { message, address }) as Promise<string>;
  },

  /**
   * Sign a PSBT
   * @param psbtHex - PSBT in hex format
   * @param options - Signing options
   * @returns Signed PSBT in hex format
   */
  signPsbt: async (
    psbtHex: string,
    options?: PsbtSigningOptions
  ): Promise<string> => {
    return sendMessage("SIGN_PSBT", { psbtHex, options }) as Promise<string>;
  },

  /**
   * Sign multiple PSBTs
   */
  signPsbts: async (
    psbtHexs: string[],
    options?: PsbtSigningOptions
  ): Promise<string[]> => {
    const results: string[] = [];
    for (const psbtHex of psbtHexs) {
      const signed = (await sendMessage("SIGN_PSBT", {
        psbtHex,
        options,
      })) as string;
      results.push(signed);
    }
    return results;
  },

  /**
   * Push a signed transaction
   */
  pushTx: async (txHex: string): Promise<string> => {
    return sendMessage("PUSH_TX", txHex) as Promise<string>;
  },

  /**
   * Get wallet balance in satoshis
   */
  getBalance: async (): Promise<number> => {
    return sendMessage("GET_BALANCE") as Promise<number>;
  },

  /**
   * Disconnect wallet
   */
  disconnect: async (): Promise<void> => {
    await sendMessage("DISCONNECT");
  },

  /**
   * Generic request method (Sats Connect style)
   */
  request: async (args: RequestArgs): Promise<unknown> => {
    const { method, params } = args;

    switch (method) {
      case "getAccounts":
        return alkanes.getAccounts();
      case "getPublicKey":
        return alkanes.getPublicKey();
      case "signMessage":
        const { message, address } = params as {
          message: string;
          address?: string;
        };
        return alkanes.signMessage(message, address);
      case "signPsbt":
        const { psbt, signInputs, broadcast } = params as {
          psbt: string;
          signInputs?: unknown;
          broadcast?: boolean;
        };
        const signed = await alkanes.signPsbt(
          psbt,
          signInputs as PsbtSigningOptions
        );
        if (broadcast) {
          // Extract and broadcast
          return alkanes.pushTx(signed);
        }
        return signed;
      default:
        throw new Error(`Unsupported method: ${method}`);
    }
  },
};

// Expose as window.alkanes
declare global {
  interface Window {
    alkanes: typeof alkanes;
  }
}

window.alkanes = alkanes;

// Also expose as window.AlkanesProvider for compatibility
(window as any).AlkanesProvider = alkanes;

console.log("[Isomer Companion] Provider injected: window.alkanes");
