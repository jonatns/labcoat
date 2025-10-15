import path from "path";
import fs from "fs";
import { pathToFileURL } from "url";
import { LabcoatConfig } from "./types.js";

async function loadLabcoatConfig(): Promise<LabcoatConfig> {
  const root = process.cwd();
  const configPathTs = path.resolve(root, "labcoat.config.ts");
  const configPathJs = path.resolve(root, "labcoat.config.js");

  try {
    if (fs.existsSync(configPathTs)) {
      await import("ts-node/register");
      const module = await import(pathToFileURL(configPathTs).href);
      return module.default ?? module;
    }

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
