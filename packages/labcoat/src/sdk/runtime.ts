import { loadConfig } from "./config.js";
import { deployContract } from "./deploy.js";
import { simulateContract } from "./simulate.js";
import { executeContract } from "./execute.js";
import { invokeLabcoat, InvokeOptions } from "./rustBinary.js";
import { TransactionOptions } from "./types.js";

export interface LabcoatWalletAddress {
  address: string;
  scriptType: string;
  derivationPath: string;
  index: number;
}

/**
 * The labcoat runtime. Public surface is unchanged from the oyl-sdk era —
 * `labcoat.setup()` returning { config, account, provider, signer, deploy,
 * simulate, execute } — but everything now runs through the Rust core
 * (pinned alkanes-rs develop) via the `labcoat` CLI.
 */
export async function setup() {
  const config = await loadConfig();

  const cli: InvokeOptions = {
    network: config.network,
    rpcUrl: config.rpcUrl,
    walletFile: (config as any).walletFile,
    env: process.env.LABCOAT_WALLET_PASSPHRASE
      ? { LABCOAT_WALLET_PASSPHRASE: process.env.LABCOAT_WALLET_PASSPHRASE }
      : undefined,
  };

  // Ensure the project wallet exists. A mnemonic from labcoat.config is
  // passed over stdin (never argv); the same mnemonic derives the same
  // addresses as the old oyl-sdk Signer (standard BIP-86/84/49/44 paths).
  const walletInfo = await invokeLabcoat<{
    address: string;
    created: boolean;
    mnemonic?: string;
  }>(["wallet", "init", ...(config.mnemonic ? ["--mnemonic-stdin"] : [])], {
    ...cli,
    stdin: config.mnemonic,
  });
  if (walletInfo.mnemonic) {
    console.warn(
      "🔐 Generated a new wallet mnemonic — write it down now:\n   " +
        walletInfo.mnemonic
    );
  }

  const addresses = await invokeLabcoat<LabcoatWalletAddress[]>(
    ["wallet", "addresses", "--count", "1"],
    cli
  );
  const byType = (fragment: string) =>
    addresses.find((a) => a.scriptType.toLowerCase().includes(fragment));

  // Account keeps the oyl-era shape (taproot/nativeSegwit/nestedSegwit/
  // legacy address slots) so existing scripts' address lookups survive.
  const account = {
    taproot: { address: byType("p2tr")?.address ?? walletInfo.address },
    nativeSegwit: { address: byType("p2wpkh")?.address },
    nestedSegwit: { address: byType("p2sh")?.address },
    legacy: { address: byType("p2pkh")?.address },
    addresses,
  };

  // The oyl Provider/Signer are gone; provider carries connection info and
  // signer is retained (null) only for destructuring compatibility.
  const provider = { network: config.network, rpcUrl: config.rpcUrl };
  const signer = null;

  const wallet = { account, provider, cli };
  const defaultOptions: TransactionOptions = { feeRate: 2 };

  async function deploy(
    contractName: string,
    options: TransactionOptions = defaultOptions
  ) {
    return deployContract(contractName, options, wallet);
  }

  async function simulate(
    contractName: string,
    methodName: string,
    args: any[] = []
  ) {
    return simulateContract(contractName, methodName, args, wallet);
  }

  async function execute(
    contractName: string,
    methodName: string,
    args: any[] = [],
    options: TransactionOptions = defaultOptions
  ) {
    return executeContract(contractName, methodName, args, options, wallet);
  }

  return { config, account, provider, signer, deploy, simulate, execute };
}

export const labcoat = { setup };
