"use strict";
var __importDefault = (this && this.__importDefault) || function (mod) {
    return (mod && mod.__esModule) ? mod : { "default": mod };
};
Object.defineProperty(exports, "__esModule", { value: true });
exports.AlkanesCompiler = void 0;
const child_process_1 = require("child_process");
const util_1 = require("util");
const promises_1 = __importDefault(require("fs/promises"));
const path_1 = __importDefault(require("path"));
const execAsync = (0, util_1.promisify)(child_process_1.exec);
class AlkanesCompiler {
    constructor() {
        this.tempDir = ".alkali";
    }
    async compile(sourceCode) {
        try {
            await this.createProject(sourceCode);
            const { stderr } = await execAsync(`cargo build --target=wasm32-unknown-unknown --release`, { cwd: this.tempDir });
            if (stderr) {
                console.warn("Build warnings:", stderr);
            }
            const wasmPath = path_1.default.join(this.tempDir, "target", "wasm32-unknown-unknown", "release", "alkanes_contract.wasm");
            const wasmBuffer = await promises_1.default.readFile(wasmPath);
            const abi = await this.parseABI(sourceCode);
            return {
                bytecode: wasmBuffer.toString("base64"),
                abi,
            };
        }
        catch (error) {
            if (error instanceof Error) {
                throw new Error(`Compilation failed: ${error.message}`);
            }
        }
    }
    async createProject(sourceCode) {
        await promises_1.default.mkdir(this.tempDir, { recursive: true });
        await promises_1.default.mkdir(path_1.default.join(this.tempDir, "src"), { recursive: true });
        const cargoToml = `
      [package]
      name = "alkanes-contract"
      version = "0.1.0"
      edition = "2021"

      [lib]
      crate-type = ["cdylib", "rlib"]

      [dependencies]
      alkanes-runtime = { git = "https://github.com/kungfuflex/alkanes-rs" }
      alkanes-support = { git = "https://github.com/kungfuflex/alkanes-rs" }
      metashrew-support = { git = "https://github.com/sandshrewmetaprotocols/metashrew" }
      anyhow = "1.0"
    `;
        await promises_1.default.writeFile(path_1.default.join(this.tempDir, "Cargo.toml"), cargoToml);
        await promises_1.default.writeFile(path_1.default.join(this.tempDir, "src", "lib.rs"), sourceCode);
    }
    async parseABI(sourceCode) {
        const methods = [];
        const opcodes = {};
        // Match MessageDispatch enum variants with #[opcode(N)]
        const messageRegex = /#\[opcode\((\d+)\)\]\s*(\w+)/g;
        let match;
        while ((match = messageRegex.exec(sourceCode)) !== null) {
            const [_, opcodeStr, methodNameRaw] = match;
            const opcodeNum = parseInt(opcodeStr, 10);
            // Convert enum variant name to camelCase for method name
            const methodName = methodNameRaw
                .replace(/([A-Z])/g, "_$1")
                .toLowerCase()
                .replace(/^_/, ""); // "DoSomething" => "do_something"
            methods.push({
                opcode: opcodeNum,
                name: methodName,
                inputs: [], // Could extend later if function has arguments
                outputs: [], // Could extend later if function returns data
            });
            opcodes[methodName] = opcodeNum;
        }
        // Parse struct name (pub struct Name)
        const structRegex = /pub\s+struct\s+(\w+)/;
        const structMatch = sourceCode.match(structRegex);
        const name = structMatch ? structMatch[1] : "UnknownContract";
        // Parse storage (StoragePointer::from_keyword)
        const storage = [];
        const storageRegex = /StoragePointer::from_keyword\("([^"]+)"\)/g;
        let storageMatch;
        while ((storageMatch = storageRegex.exec(sourceCode)) !== null) {
            storage.push({
                key: storageMatch[1],
                type: "Vec<u8>", // default type
            });
        }
        return {
            name,
            version: "1.0.0",
            methods,
            storage,
            opcodes,
        };
    }
}
exports.AlkanesCompiler = AlkanesCompiler;
//# sourceMappingURL=compiler.js.map