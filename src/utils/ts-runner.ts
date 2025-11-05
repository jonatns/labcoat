import { spawn } from "child_process";
import path from "path";
import os from "os";
import fs from "fs/promises";

/**
 * Run a TypeScript file with tsx (no build step).
 */
export async function runTypeScriptFile(filePath: string, args: string[] = []) {
  const tsxBin = path.resolve(process.cwd(), "node_modules/.bin/tsx");

  console.log(`📦 Running script with tsx...`);
  console.log(`   → Source: ${filePath}`);

  const child = spawn(tsxBin, [filePath, ...args], {
    stdio: "inherit",
  });

  child.on("exit", (code) => process.exit(code ?? 0));
}

/**
 * Dynamically import a TypeScript module with tsx.
 */
export async function importTypeScriptModule(filePath: string) {
  // Resolve the absolute path and use the tsx runtime loader
  const absPath = path.resolve(filePath);

  try {
    // Use dynamic import with tsx register hook
    const module = await import(`tsx/esm:${absPath}`);
    return module.default ?? module;
  } catch (err) {
    console.error(`❌ Failed to import module:`, err);
    return {};
  }
}
