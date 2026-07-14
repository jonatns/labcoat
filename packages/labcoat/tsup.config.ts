import { defineConfig } from "tsup";

export default defineConfig([
  {
    entry: ["src/sdk/**/*.ts"],
    format: ["esm", "cjs"],
    dts: true,
    sourcemap: true,
    clean: true,
    target: "node18",
    outDir: "dist/sdk",
    bundle: false,
    platform: "node",
    treeshake: true,
  },
  {
    entry: ["src/cli/index.ts"],
    format: ["esm"],
    clean: true,
    target: "node18",
    outDir: "dist/cli",
    bundle: true,
    platform: "node",
    sourcemap: true,
    banner: { js: "#!/usr/bin/env node" },
    splitting: false,
  },
]);
