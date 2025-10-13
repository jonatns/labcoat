#!/usr/bin/env node
import { Command } from "commander";
import { AlkanesCompiler, AlkanesContract } from "./index.js";
import fs from "fs/promises";
import path from "path";
import { spawn } from "child_process";
import { fileURLToPath } from "url";
function handleCommandError(error) {
    if (error instanceof Error) {
        console.error("❌ Command failed:", error.message);
    }
    else {
        console.error("❌ Command failed:", error);
    }
    process.exit(1);
}
const program = new Command();
program
    .name("labcoat")
    .description("Smart contract development toolkit for Bitcoin Alkanes")
    .version("0.1.0");
program
    .command("init")
    .description("Initialize a new Labcoat project")
    .option("-t, --template <name>", "Template to use", "default")
    .action(async (options) => {
    try {
        console.log("🔥 Initializing Labcoat project...");
        const __filename = fileURLToPath(import.meta.url);
        const __dirname = path.dirname(__filename);
        const templatePath = path.join(__dirname, "..", "templates", options.template);
        try {
            await fs.access(templatePath);
            await fs.cp(templatePath, process.cwd(), { recursive: true });
        }
        catch (err) {
            console.error(`Template "${options.template}" not found`);
            process.exit(1);
        }
        console.log("✅ Project initialized successfully");
        console.log("\nNext steps:");
        console.log("  1. npx labcoat compile               # Compile contracts");
        console.log("  2. npx labcoat test                  # Run tests");
        console.log("  3. npx labcoat run scripts/deploy.ts # Deploy contracts");
    }
    catch (error) {
        console.error("❌ Failed to initialize project", error);
        process.exit(1);
    }
});
program
    .command("compile [file]")
    .description("Compile one or all Rust contracts in the contracts directory")
    .option("-o, --output <dir>", "Output directory", "./build")
    .action(async (file, options) => {
    try {
        const compiler = new AlkanesCompiler();
        const outputDir = options.output;
        // Create output directory
        await fs.mkdir(outputDir, { recursive: true });
        let filesToCompile = [];
        if (file) {
            // Specific file provided
            filesToCompile = [file];
        }
        else {
            // No file -> compile all .rs in contracts directory
            const contractsDir = path.join(process.cwd(), "contracts");
            try {
                const entries = await fs.readdir(contractsDir, {
                    withFileTypes: true,
                });
                filesToCompile = entries
                    .filter((e) => e.isFile() && e.name.endsWith(".rs"))
                    .map((e) => path.join(contractsDir, e.name));
                if (filesToCompile.length === 0) {
                    console.error("❌ No .rs files found in ./contracts");
                    process.exit(1);
                }
            }
            catch {
                console.error("❌ Could not find a ./contracts directory");
                process.exit(1);
            }
        }
        console.log(`🦾 Compiling ${filesToCompile.length} contract(s)...`);
        for (const filePath of filesToCompile) {
            const fileName = path.basename(filePath, ".rs");
            console.log(`🔨 Compiling ${fileName}.rs...`);
            const sourceCode = await fs.readFile(filePath, "utf8");
            const result = await compiler.compile(sourceCode);
            if (!result)
                throw new Error(`Compilation failed for ${fileName}`);
            const { bytecode, abi } = result;
            // Write compiled output
            const wasmPath = path.join(outputDir, `${fileName}.wasm`);
            const abiPath = path.join(outputDir, `${fileName}.abi.json`);
            await fs.writeFile(wasmPath, Buffer.from(bytecode, "base64"));
            await fs.writeFile(abiPath, JSON.stringify(abi, null, 2));
            console.log(`✅ ${fileName}.rs compiled successfully:
- Bytecode: ${wasmPath}
- ABI: ${abiPath}\n`);
        }
        console.log("🎉 All contracts compiled successfully!");
    }
    catch (error) {
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
    }
    catch (error) {
        handleCommandError(error);
    }
});
program
    .command("run <script>")
    .description("Run a custom Labcoat script (.ts or .js)")
    .action((script) => {
    const scriptPath = path.resolve(script);
    console.log(`🧩 Running script: ${scriptPath}`);
    const isTs = scriptPath.endsWith(".ts");
    const args = isTs ? ["ts-node", scriptPath] : ["node", scriptPath];
    const child = spawn("npx", args, {
        stdio: "inherit",
        shell: true, // important for cross-platform npx
    });
    child.on("exit", (code) => process.exit(code ?? 0));
});
program.parse();
