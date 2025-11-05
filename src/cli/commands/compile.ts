import { Command } from "commander";
import fs from "fs/promises";
import path from "path";
import { AlkanesCompiler } from "@/sdk/compiler.js";
import { gzipWasm } from "@/sdk/helpers.js";

export const compileCommand = new Command("compile")
  .argument("[file]", "Specific contract file to compile")
  .option("-o, --output <dir>", "Output directory", "./build")
  .description("Compile one or all Rust contracts in the contracts directory")
  .action(async (file, options) => {
    const compiler = new AlkanesCompiler();
    const outputDir = path.resolve(options.output);
    await fs.mkdir(outputDir, { recursive: true });

    let filesToCompile: string[] = [];

    if (file) {
      filesToCompile = [path.resolve(file)];
    } else {
      const contractsDir = path.join(process.cwd(), "contracts");
      const entries = await fs.readdir(contractsDir, { withFileTypes: true });
      filesToCompile = entries
        .filter((e) => e.isFile() && e.name.endsWith(".rs"))
        .map((e) => path.join(contractsDir, e.name));
    }

    if (!filesToCompile.length) {
      console.error("❌ No .rs files found in ./contracts");
      process.exit(1);
    }

    console.log(`🦾 Compiling ${filesToCompile.length} contract(s)...`);
    for (const filePath of filesToCompile) {
      const name = path.basename(filePath, ".rs");
      console.log(`🔨 Compiling ${name}.rs...`);

      const source = await fs.readFile(filePath, "utf8");
      const result = await compiler.compile(name, source);
      const { wasmBuffer, abi } = result;

      const gzipped = await gzipWasm(wasmBuffer);
      await fs.writeFile(path.join(outputDir, `${name}.wasm.gz`), gzipped);
      await fs.writeFile(
        path.join(outputDir, `${name}.abi.json`),
        JSON.stringify(abi, null, 2)
      );

      console.log(`✅ ${name} compiled successfully`);
    }
  });
