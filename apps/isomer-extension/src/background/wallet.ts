/**
 * Wallet Service
 *
 * Integrates with @alkanes/ts-sdk for key management and signing
 */

import {
  AlkanesWallet,
  createWalletFromMnemonic,
  AddressType,
  createKeystore,
  unlockKeystore,
} from "@alkanes/ts-sdk";

export interface WalletState {
  initialized: boolean;
  unlocked: boolean;
  hasWallet: boolean;
  address?: string;
  balance?: number;
}

export interface Account {
  address: string;
  publicKey: string;
  addressType: string;
}

const STORAGE_KEY = "isomer_companion_wallet";
const MNEMONIC_KEY = "isomer_companion_mnemonic"; // For dev mode - stores mnemonic plaintext

export class WalletService {
  private wallet: AlkanesWallet | null = null;
  private encryptedKeystore: string | null = null;
  private mnemonic: string | null = null;
  private state: WalletState = {
    initialized: false,
    unlocked: false,
    hasWallet: false,
  };

  constructor() {}

  async initialize(): Promise<void> {
    // Load encrypted keystore from storage
    const stored = await chrome.storage.local.get([STORAGE_KEY, MNEMONIC_KEY]);
    this.encryptedKeystore = stored[STORAGE_KEY] || null;
    this.mnemonic = stored[MNEMONIC_KEY] || null;

    this.state = {
      initialized: true,
      unlocked: false,
      hasWallet: !!this.encryptedKeystore || !!this.mnemonic,
    };

    // For development convenience, auto-create and unlock wallet
    if (!this.mnemonic) {
      await this.createWallet();
    } else {
      await this.unlock();
    }
  }

  getState(): WalletState {
    return { ...this.state };
  }

  async createWallet(): Promise<WalletState> {
    // Use the createKeystore helper which handles mnemonic generation
    const { keystore, mnemonic } = await createKeystore("", {
      network: "regtest",
      wordCount: 12,
    });

    this.mnemonic = mnemonic;
    this.encryptedKeystore = JSON.stringify(keystore);

    // Create wallet for regtest
    this.wallet = createWalletFromMnemonic(mnemonic, "regtest");

    // Store mnemonic (dev mode - plaintext for easy debugging)
    await chrome.storage.local.set({
      [STORAGE_KEY]: this.encryptedKeystore,
      [MNEMONIC_KEY]: mnemonic,
    });

    // Get address using correct API: deriveAddress(type, change, index)
    const addressInfo = this.wallet.deriveAddress(AddressType.P2WPKH, 0, 0);
    const address = addressInfo.address;

    this.state = {
      initialized: true,
      unlocked: true,
      hasWallet: true,
      address,
    };

    console.log("[Isomer Companion] Wallet created:", address);
    console.log("[Isomer Companion] Mnemonic (dev only):", mnemonic);

    return this.state;
  }

  async unlock(_password: string = ""): Promise<WalletState> {
    if (!this.mnemonic && !this.encryptedKeystore) {
      throw new Error("No wallet found");
    }

    try {
      // For dev mode, we store mnemonic plaintext
      let mnemonic = this.mnemonic;

      if (!mnemonic && this.encryptedKeystore) {
        // Try to unlock the encrypted keystore
        const keystore = await unlockKeystore(
          JSON.parse(this.encryptedKeystore),
          _password
        );
        mnemonic = keystore.mnemonic;
      }

      if (!mnemonic) {
        throw new Error("Could not retrieve mnemonic");
      }

      this.mnemonic = mnemonic;
      this.wallet = createWalletFromMnemonic(mnemonic, "regtest");

      // deriveAddress(type, change, index)
      const addressInfo = this.wallet.deriveAddress(AddressType.P2WPKH, 0, 0);
      const address = addressInfo.address;

      this.state = {
        ...this.state,
        unlocked: true,
        address,
      };

      return this.state;
    } catch (error) {
      console.error("[Isomer Companion] Unlock failed:", error);
      throw new Error("Failed to unlock wallet");
    }
  }

  lock(): WalletState {
    this.wallet = null;
    this.state = {
      ...this.state,
      unlocked: false,
      address: undefined,
      balance: undefined,
    };
    return this.state;
  }

  getAccounts(): Account[] {
    if (!this.wallet) {
      throw new Error("Wallet not unlocked");
    }

    // deriveAddress(type, change, index)
    const p2wpkh = this.wallet.deriveAddress(AddressType.P2WPKH, 0, 0);
    const p2tr = this.wallet.deriveAddress(AddressType.P2TR, 0, 0);

    return [
      {
        address: p2wpkh.address,
        publicKey: p2wpkh.publicKey,
        addressType: "p2wpkh",
      },
      {
        address: p2tr.address,
        publicKey: p2tr.publicKey,
        addressType: "p2tr",
      },
    ];
  }

  getPublicKey(): string {
    if (!this.wallet) {
      throw new Error("Wallet not unlocked");
    }

    const info = this.wallet.deriveAddress(AddressType.P2TR, 0, 0);
    return info.publicKey;
  }

  async getBalance(): Promise<number> {
    if (!this.state.address) {
      return 0;
    }

    try {
      // Query Esplora for address balance
      const response = await fetch(
        `http://localhost:50010/address/${this.state.address}`
      );
      if (!response.ok) {
        return 0;
      }

      const data = await response.json();
      const balance =
        (data.chain_stats?.funded_txo_sum || 0) -
        (data.chain_stats?.spent_txo_sum || 0);
      this.state.balance = balance;
      return balance;
    } catch (error) {
      console.error("[Isomer Companion] Failed to fetch balance:", error);
      return 0;
    }
  }

  async signMessage(message: string, _address?: string): Promise<string> {
    if (!this.wallet) {
      throw new Error("Wallet not unlocked");
    }

    // signMessage is async
    return await this.wallet.signMessage(message, 0);
  }

  async signPsbt(psbtHex: string, _options?: unknown): Promise<string> {
    if (!this.wallet) {
      throw new Error("Wallet not unlocked");
    }

    // Convert hex to base64 for the wallet
    const psbtBase64 = Buffer.from(psbtHex, "hex").toString("base64");

    // signPsbt is async
    const signedBase64 = await this.wallet.signPsbt(psbtBase64);

    // Convert back to hex
    return Buffer.from(signedBase64, "base64").toString("hex");
  }

  // Dev helpers
  getMnemonic(): string | null {
    return this.mnemonic;
  }
}
