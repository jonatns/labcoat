import { defineConfig } from "tsup";

export default defineConfig([
  {
    entry: ["src/sdk/index.ts"],
    format: ["esm", "cjs"],
    dts: true,
    sourcemap: true,
    clean: true,
    target: "node18",
    outDir: "dist/sdk",
    bundle: true,
    platform: "node",
    treeshake: true,
  },
  {
    entry: ["src/cli/index.ts"],
    format: ["esm"],
    clean: false,
    target: "node18",
    outDir: "dist/cli",
    bundle: true,
    platform: "node",
    sourcemap: true,
    banner: { js: "#!/usr/bin/env node" },
    splitting: false,
  },
]);
