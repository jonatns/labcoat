/**
 * Background Service Worker
 *
 * Handles wallet state, message routing, and RPC calls to Isomer
 */

import { WalletService } from "./wallet";

// Isomer's local endpoints
const ISOMER_RPC = "http://localhost:18888";

// Singleton wallet service
let walletService: WalletService | null = null;

// Initialize on install
chrome.runtime.onInstalled.addListener(async () => {
  console.log("[Isomer Companion] Extension installed");
  walletService = new WalletService();
  await walletService.initialize();
});

// Initialize on startup
chrome.runtime.onStartup.addListener(async () => {
  console.log("[Isomer Companion] Extension started");
  walletService = new WalletService();
  await walletService.initialize();
});

// Ensure wallet service is ready
async function getWalletService(): Promise<WalletService> {
  if (!walletService) {
    walletService = new WalletService();
    await walletService.initialize();
  }
  return walletService;
}

// Message types
interface RequestMessage {
  type: string;
  payload?: unknown;
  id: string;
}

// ResponseMessage type is used in sendResponse calls

// Handle messages from popup and content scripts
chrome.runtime.onMessage.addListener(
  (message: RequestMessage, sender, sendResponse) => {
    handleMessage(message, sender)
      .then((result) => sendResponse({ id: message.id, result }))
      .catch((error) => sendResponse({ id: message.id, error: error.message }));

    // Return true to indicate async response
    return true;
  }
);

async function handleMessage(
  message: RequestMessage,
  sender: chrome.runtime.MessageSender
): Promise<unknown> {
  const service = await getWalletService();

  switch (message.type) {
    // Wallet operations
    case "GET_STATE":
      return service.getState();

    case "CREATE_WALLET":
      return service.createWallet();

    case "UNLOCK_WALLET":
      return service.unlock(message.payload as string);

    case "LOCK_WALLET":
      return service.lock();

    case "GET_ACCOUNTS":
      return service.getAccounts();

    case "GET_PUBLIC_KEY":
      return service.getPublicKey();

    case "GET_BALANCE":
      return service.getBalance();

    case "SIGN_MESSAGE":
      const { message: msg, address } = message.payload as {
        message: string;
        address: string;
      };
      return service.signMessage(msg, address);

    case "SIGN_PSBT":
      const { psbtHex, options } = message.payload as {
        psbtHex: string;
        options?: any;
      };
      return service.signPsbt(psbtHex, options);

    // Isomer RPC operations
    case "GET_NETWORK":
      return "regtest";

    case "FAUCET_REQUEST":
      return requestFaucet(
        message.payload as { address: string; amount: number }
      );

    case "PUSH_TX":
      return pushTransaction(message.payload as string);

    // Provider connection
    case "CONNECT":
      return handleConnect(sender);

    case "DISCONNECT":
      return handleDisconnect(sender);

    default:
      throw new Error(`Unknown message type: ${message.type}`);
  }
}

// Connected sites
const connectedSites = new Map<string, boolean>();

async function handleConnect(
  sender: chrome.runtime.MessageSender
): Promise<unknown> {
  const origin = sender.origin || sender.url;
  if (!origin) {
    throw new Error("Unknown origin");
  }

  // For regtest, auto-approve all connections (dev mode)
  connectedSites.set(origin, true);

  const service = await getWalletService();
  return service.getAccounts();
}

async function handleDisconnect(
  sender: chrome.runtime.MessageSender
): Promise<void> {
  const origin = sender.origin || sender.url;
  if (origin) {
    connectedSites.delete(origin);
  }
}

async function requestFaucet(payload: {
  address: string;
  amount: number;
}): Promise<string> {
  // Call Isomer's Tauri backend faucet via the JSON-RPC endpoint
  const response = await fetch(ISOMER_RPC, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({
      jsonrpc: "2.0",
      method: "generatetoaddress",
      params: [1, payload.address],
      id: Date.now(),
    }),
  });

  if (!response.ok) {
    throw new Error("Faucet request failed");
  }

  const result = await response.json();
  if (result.error) {
    throw new Error(result.error.message || "Faucet error");
  }

  return result.result;
}

async function pushTransaction(txHex: string): Promise<string> {
  const response = await fetch(ISOMER_RPC, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({
      jsonrpc: "2.0",
      method: "sendrawtransaction",
      params: [txHex],
      id: Date.now(),
    }),
  });

  if (!response.ok) {
    throw new Error("Failed to broadcast transaction");
  }

  const result = await response.json();
  if (result.error) {
    throw new Error(result.error.message || "Broadcast error");
  }

  return result.result;
}

export {};
