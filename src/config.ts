import path from "path";
import fs from "fs";
import { pathToFileURL } from "url";
import { AlkaliConfig } from "./types.js";

export async function loadAlkaliConfig(): Promise<AlkaliConfig> {
  const root = process.cwd();
  const configPathTs = path.resolve(root, "alkali.config.ts");
  const configPathJs = path.resolve(root, "alkali.config.js");

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

    console.warn("⚠️  No alkali.config.ts or alkali.config.js found");
    return {} as AlkaliConfig;
  } catch (err) {
    console.error("❌ Failed to load Alkali config:", err);
    return {} as AlkaliConfig;
  }
}
