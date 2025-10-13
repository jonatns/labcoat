import path from "path";
import fs from "fs";
import { pathToFileURL } from "url";
import type { AlkaliConfig } from "./types";

export async function loadAlkaliConfig(): Promise<AlkaliConfig> {
  const cwd = process.cwd();
  const configPathTs = path.join(cwd, "alkali.config.ts");
  const configPathJs = path.join(cwd, "alkali.config.js");

  // Prefer TS over JS
  const targetPath = fs.existsSync(configPathTs)
    ? configPathTs
    : fs.existsSync(configPathJs)
    ? configPathJs
    : null;

  if (!targetPath) {
    console.warn("⚠️ No alkali.config.{ts,js} found in project root.");
    return {} as AlkaliConfig;
  }

  // If TypeScript config exists, register ts-node before import
  if (targetPath.endsWith(".ts")) {
    try {
      // @ts-expect-error no types for ts-node/register
      await import("ts-node/register");
    } catch {
      console.error(
        "❌ alkali.config.ts detected but ts-node is not installed.\nRun: npm i -D ts-node typescript"
      );
      process.exit(1);
    }
  }

  // Import using file URL to support both CJS/ESM
  try {
    const configModule = await import(pathToFileURL(targetPath).href);
    return configModule.default || configModule;
  } catch (err) {
    console.error("❌ Failed to load Alkali config:", err);
    process.exit(1);
  }
}
