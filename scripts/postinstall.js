#!/usr/bin/env node
import { execSync } from "child_process";
import fs from "fs";
import path from "path";
import url from "url";

const __dirname = path.dirname(url.fileURLToPath(import.meta.url));
const binaryPath = path.resolve(__dirname, "../dist/cli/index.js");

try {
  // Check if binary exists
  if (!fs.existsSync(binaryPath)) {
    console.warn("⚠️  Labcoat binary not found. Did you forget to build before publishing?");
    process.exit(0);
  }

  // Test running it
  execSync(`${binaryPath} --version`, { stdio: "ignore" });

  console.log(`
✨ Labcoat installed successfully!
🧪 Run "labcoat --help" to get started.
`);

} catch (err) {
  console.warn("⚠️  Labcoat postinstall check skipped or failed:", err?.message || err);
}
