import { Command } from "commander";
import fs from "fs/promises";
import path from "path";
import { AlkanesCompiler } from "@/sdk/compiler.js";
import { gzipWasm } from "@/sdk/helpers.js";

export const compileCommand = new Command("compile")
  .argument("[target]", "Contract file or directory to compile")
  .option("-o, --output <dir>", "Output directory", "./build")
  .description("Compile one or all Rust contracts in the contracts directory")
  .action(async (target, options) => {
    const compiler = new AlkanesCompiler();
    const outputDir = path.resolve(options.output);
    await fs.mkdir(outputDir, { recursive: true });

    let contractsToCompile: string[] = [];

    const contractsDir = path.join(process.cwd(), "contracts");

    // Determine what to compile
    if (target) {
      const resolved = path.resolve(target);
      contractsToCompile = [resolved];
    } else {
      // Collect all files and folders inside ./contracts
      const entries = await fs.readdir(contractsDir, { withFileTypes: true });
      contractsToCompile = entries
        .filter(
          (e) => (e.isFile() && e.name.endsWith(".rs")) || e.isDirectory()
        )
        .map((e) => path.join(contractsDir, e.name));
    }

    if (!contractsToCompile.length) {
      console.error("❌ No contracts found in ./contracts");
      process.exit(1);
    }

    console.log(`🦾 Compiling ${contractsToCompile.length} contract(s)...`);

    for (const contractPath of contractsToCompile) {
      const stat = await fs.stat(contractPath);
      const name = stat.isFile()
        ? path.basename(contractPath, ".rs")
        : path.basename(contractPath);

      console.log(`🔨 Compiling ${name}...`);

      const result = stat.isFile()
        ? await compiler.compile(name, await fs.readFile(contractPath, "utf8"))
        : await compiler.compile(name, contractPath);

      const { wasmBuffer, abi } = result;

      const wasmOut = path.join(outputDir, `${name}.wasm.gz`);
      const abiOut = path.join(outputDir, `${name}.abi.json`);

      await fs.writeFile(wasmOut, await gzipWasm(wasmBuffer));
      await fs.writeFile(abiOut, JSON.stringify(abi, null, 2));

      console.log(`✅ ${name} compiled successfully`);
      console.log(`   → ${wasmOut}`);
      console.log(`   → ${abiOut}`);
    }
  });
