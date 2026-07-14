import fs from "fs";
import path from "path";
import { pathToFileURL } from "url";
import { LabcoatConfig } from "./types.js";
import { importTypeScriptModule } from "./utils/ts-runner.js";

/**
 * Loads labcoat.config.ts or labcoat.config.js (prefers TS)
 */
async function loadLabcoatConfig(): Promise<LabcoatConfig> {
  const root = process.cwd();
  const configPathTs = path.resolve(root, "labcoat.config.ts");
  const configPathJs = path.resolve(root, "labcoat.config.js");

  try {
    // Prefer TypeScript config if present
    if (fs.existsSync(configPathTs)) {
      return await importTypeScriptModule(configPathTs);
    }

    // Fall back to JavaScript config
    if (fs.existsSync(configPathJs)) {
      const module = await import(pathToFileURL(configPathJs).href);
      return module.default ?? module;
    }

    console.warn("⚠️  No labcoat.config.ts or labcoat.config.js found");
    return {} as LabcoatConfig;
  } catch (err) {
    console.error("❌ Failed to load Labcoat config:", err);
    return {} as LabcoatConfig;
  }
}

/**
 * Loads the user's Labcoat config merged with defaults.
 *
 * Defaults target the local devnet (`labcoat up`): regtest via the unified
 * JSON-RPC gateway. The deprecated "oylnet" network value maps to regtest;
 * `projectId` (a Sandshrew/oyl concept) is accepted but ignored.
 */
export async function loadConfig() {
  const labcoatConfig = await loadLabcoatConfig();

  const merged = {
    network: "regtest",
    rpcUrl: "http://localhost:18888",
    walletFile: ".labcoat/wallet.json",
    ...labcoatConfig,
  } as { network: string; rpcUrl: string; walletFile: string; mnemonic?: string };

  if (merged.network === "oylnet") {
    console.warn("⚠️  network 'oylnet' is deprecated; using 'regtest'");
    merged.network = "regtest";
  }
  if ((merged as any).projectId) {
    console.warn(
      "⚠️  labcoat.config projectId is no longer used (oyl-sdk was removed)"
    );
  }

  return merged;
}
