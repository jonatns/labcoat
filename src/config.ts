import path from "path";
import fs from "fs";
import { pathToFileURL } from "url";
import type { AlkaliConfig } from "./types";

export async function loadAlkaliConfig(): Promise<AlkaliConfig> {
  const cwd = process.cwd();
  const configPathTs = path.join(cwd, "alkali.config.ts");
  const configPathJs = path.join(cwd, "alkali.config.js");

  if (fs.existsSync(configPathTs)) {
    const module = await import(pathToFileURL(configPathTs).href);
    return module.default || module;
  } else if (fs.existsSync(configPathJs)) {
    const module = await import(pathToFileURL(configPathJs).href);
    return module.default || module;
  } else {
    console.warn("⚠️ No alkali.config.{ts,js} found in project root.");
    return {} as AlkaliConfig;
  }
}
