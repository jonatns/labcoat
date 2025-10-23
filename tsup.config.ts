import { defineConfig } from "tsup";
import fs from "fs";
import path from "path";

function listSrcFiles(dir: string) {
  const entries: Record<string, string> = {};
  for (const file of fs.readdirSync(dir)) {
    if (file.endsWith(".ts")) {
      const name = file.replace(/\.ts$/, "");
      entries[name] = path.join(dir, file);
    }
  }
  return entries;
}

const srcDir = path.resolve("src");
const entries = listSrcFiles(srcDir);

export default defineConfig([
  {
    entry: entries,
    format: ["esm", "cjs"],
    dts: true,
    bundle: false,
    outDir: "dist",
    clean: true,
    outExtension({ format }) {
      return { js: format === "cjs" ? ".cjs" : ".js" };
    },
  },
  {
    entry: { cli: "src/cli.ts" },
    format: ["cjs"],
    outDir: "dist",
    banner: { js: "#!/usr/bin/env node" },
    external: ["path", "fs", "url", "buffer", "bitcoinjs-lib"],
  },
]);
