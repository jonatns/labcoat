// #!/usr/bin/env node

import { Command } from "commander";
import { AlkanesCompiler, AlkanesContract } from "./index";
import fs from "fs/promises";
import path from "path";

function handleCommandError(error: any) {
  if (error instanceof Error) {
    console.error("❌ Command failed:", error.message);
  } else {
    console.error("❌ Command failed:", error);
  }
  process.exit(1);
}

const program = new Command();

program
  .name("alkanes")
  .description("CLI for deploying Alkanes contracts")
  .version("0.1.0");

program
  .command("compile <file>")
  .description("Compile a Rust contract to WASM")
  .option("-o, --output <dir>", "Output directory", "./build")
  .action(async (file: string, options) => {
    try {
      const sourceCode = await fs.readFile(file, "utf8");
      const compiler = new AlkanesCompiler("http://localhost:3000");

      const result = await compiler.compile(sourceCode);
      if (!result) {
        throw new Error("Compilation failed, no result returned");
      }
      const { bytecode, abi } = result;

      // Create output directory
      await fs.mkdir(options.output, { recursive: true });

      // Save bytecode
      const wasmPath = path.join(options.output, "contract.wasm");
      await fs.writeFile(wasmPath, Buffer.from(bytecode, "base64"));

      // Save ABI
      const abiPath = path.join(options.output, "abi.json");
      await fs.writeFile(abiPath, JSON.stringify(abi, null, 2));

      console.log(`✅ Contract compiled successfully:
- Bytecode: ${wasmPath}
- ABI: ${abiPath}`);
    } catch (error) {
      handleCommandError(error);
    }
  });

program
  .command("deploy")
  .description("Deploy a compiled contract")
  .requiredOption("--wasm <file>", "WASM bytecode file")
  .requiredOption("--abi <file>", "ABI JSON file")
  .option("--args <args...>", "Constructor arguments")
  .action(async (options) => {
    try {
      // Load files
      const bytecode = await fs.readFile(options.wasm);
      const abi = JSON.parse(await fs.readFile(options.abi, "utf8"));

      // Create contract instance
      const contract = new AlkanesContract({
        bytecode: bytecode.toString("base64"),
        abi,
      });

      // Deploy
      const address = await contract.deploy(options.args || []);

      console.log(`✅ Contract deployed successfully:
Address: ${address}`);
    } catch (error) {
      handleCommandError(error);
    }
  });

program.parse();
