#!/usr/bin/env node
"use strict";
var __importDefault = (this && this.__importDefault) || function (mod) {
    return (mod && mod.__esModule) ? mod : { "default": mod };
};
Object.defineProperty(exports, "__esModule", { value: true });
const commander_1 = require("commander");
const index_1 = require("./index");
const promises_1 = __importDefault(require("fs/promises"));
const path_1 = __importDefault(require("path"));
const child_process_1 = require("child_process");
function handleCommandError(error) {
    if (error instanceof Error) {
        console.error("❌ Command failed:", error.message);
    }
    else {
        console.error("❌ Command failed:", error);
    }
    process.exit(1);
}
const program = new commander_1.Command();
program
    .name("alkali")
    .description("Smart contract development toolkit for Bitcoin Alkanes")
    .version("0.1.0");
program
    .command("init")
    .description("Initialize a new Alkali project")
    .option("-t, --template <name>", "Template to use", "default")
    .action(async (options) => {
    try {
        console.log("🔥 Initializing Alkali project...");
        const templatePath = path_1.default.join(__dirname, "..", "templates", options.template);
        try {
            await promises_1.default.access(templatePath);
            await promises_1.default.cp(templatePath, process.cwd(), { recursive: true });
        }
        catch (err) {
            console.error(`Template "${options.template}" not found`);
            process.exit(1);
        }
        console.log("✅ Project initialized successfully");
        console.log("\nNext steps:");
        console.log("  1. npx alkali compile            # Compile contracts");
        console.log("  2. npx alkali test               # Run tests");
        console.log("  3. npx alkali deploy             # Deploy contracts");
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
        const compiler = new index_1.AlkanesCompiler();
        const outputDir = options.output;
        // Create output directory
        await promises_1.default.mkdir(outputDir, { recursive: true });
        let filesToCompile = [];
        if (file) {
            // Specific file provided
            filesToCompile = [file];
        }
        else {
            // No file -> compile all .rs in contracts directory
            const contractsDir = path_1.default.join(process.cwd(), "contracts");
            try {
                const entries = await promises_1.default.readdir(contractsDir, {
                    withFileTypes: true,
                });
                filesToCompile = entries
                    .filter((e) => e.isFile() && e.name.endsWith(".rs"))
                    .map((e) => path_1.default.join(contractsDir, e.name));
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
            const fileName = path_1.default.basename(filePath, ".rs");
            console.log(`🔨 Compiling ${fileName}.rs...`);
            const sourceCode = await promises_1.default.readFile(filePath, "utf8");
            const result = await compiler.compile(sourceCode);
            if (!result)
                throw new Error(`Compilation failed for ${fileName}`);
            const { bytecode, abi } = result;
            // Write compiled output
            const wasmPath = path_1.default.join(outputDir, `${fileName}.wasm`);
            const abiPath = path_1.default.join(outputDir, `${fileName}.abi.json`);
            await promises_1.default.writeFile(wasmPath, Buffer.from(bytecode, "base64"));
            await promises_1.default.writeFile(abiPath, JSON.stringify(abi, null, 2));
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
        const bytecode = await promises_1.default.readFile(options.wasm);
        const abi = JSON.parse(await promises_1.default.readFile(options.abi, "utf8"));
        // Create contract instance
        const contract = new index_1.AlkanesContract({
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
    .description("Run a custom Alkali script (.ts or .js)")
    .action((script) => {
    const scriptPath = path_1.default.resolve(script);
    console.log(`🧩 Running script: ${scriptPath}`);
    const isTs = scriptPath.endsWith(".ts");
    const args = isTs ? ["ts-node", scriptPath] : ["node", scriptPath];
    const child = (0, child_process_1.spawn)("npx", args, {
        stdio: "inherit",
        shell: true, // important for cross-platform npx
    });
    child.on("exit", (code) => process.exit(code ?? 0));
});
program.parse();
//# sourceMappingURL=cli.js.map