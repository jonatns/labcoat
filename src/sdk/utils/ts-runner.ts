import { spawn } from "child_process";
import { createRequire } from "module";
import path from "path";
import { pathToFileURL } from "url";
import { fileURLToPath } from "url";

/**
 * Run a TypeScript file with tsx (no build step).
 */
const __dirname = path.dirname(fileURLToPath(import.meta.url));
const require = createRequire(import.meta.url);

export async function runTypeScriptFile(filePath: string, args: string[] = []) {
  // Resolve tsx from this package (labcoat), not the cwd
  const tsxBin = require.resolve(".bin/tsx", { paths: [__dirname] });

  console.log(`📦 Running script with tsx...`);
  console.log(`   → Source: ${filePath}`);

  const child = spawn(tsxBin, [filePath, ...args], { stdio: "inherit" });
  child.on("exit", (code) => process.exit(code ?? 0));
}

/**
 * Dynamically import a TypeScript module with tsx runtime.
 */
export async function importTypeScriptModule(filePath: string) {
  const absPath = path.resolve(filePath);

  try {
    // Dynamically load tsx's ESM register hook
    // This enables importing `.ts` files seamlessly
    const tsx = await import(require.resolve("tsx", { paths: [__dirname] }));

    // Ensure the register hook is active before importing your file
    if (tsx && typeof tsx.register === "function") {
      await tsx.register(); // initializes tsx runtime for on-the-fly TS execution
    }

    // Import the TS module using native dynamic import
    const module = await import(pathToFileURL(absPath).href);

    return module.default ?? module;
  } catch (err) {
    console.error(`❌ Failed to import module at ${filePath}:`, err);
    return {};
  }
}
