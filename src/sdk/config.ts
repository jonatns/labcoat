import fs from "fs";
import path from "path";
import { pathToFileURL } from "url";
import { LabcoatConfig } from "./types.js";
import { importTypeScriptModule } from "../utils/ts-runner.js";

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
 */
export async function loadConfig() {
  const labcoatConfig = await loadLabcoatConfig();

  return {
    mnemonic: "<your mnemonic>",
    network: "oylnet",
    projectId: "regtest",
    rpcUrl: "https://oylnet.oyl.gg",
    ...labcoatConfig,
  };
}
