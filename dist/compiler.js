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
    constructor(tempDir = ".alkanes") {
        this.tempDir = tempDir;
    }
    async compile(sourceCode) {
        try {
            // Create temporary project
            await this.createProject(sourceCode);
            // Run wasm-pack build
            const { stdout, stderr } = await execAsync("wasm-pack build --target web", { cwd: this.tempDir });
            if (stderr) {
                console.warn("Build warnings:", stderr);
            }
            // Read the WASM file
            const wasmPath = path_1.default.join(this.tempDir, "pkg", "alkanes_contract_bg.wasm");
            const wasmBuffer = await promises_1.default.readFile(wasmPath);
            // Parse ABI from source code
            const abi = await this.parseABI(sourceCode);
            // Clean up (optional)
            // await fs.rm(this.tempDir, { recursive: true, force: true });
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
        // Create project directory
        await promises_1.default.mkdir(this.tempDir, { recursive: true });
        await promises_1.default.mkdir(path_1.default.join(this.tempDir, "src"), { recursive: true });
        // Create Cargo.toml
        const cargoToml = `
[package]
name = "alkanes-contract"
version = "0.1.0"
edition = "2021"

[lib]
crate-type = ["cdylib"]

[dependencies]
alkanes-runtime = { git = "https://github.com/kungfuflex/alkanes-rs" }
alkanes-support = { git = "https://github.com/kungfuflex/alkanes-rs" }
metashrew-support = { git = "https://github.com/kungfuflex/alkanes-rs" }
anyhow = "1.0"
hex-lit = "0.1.1"
    `;
        await promises_1.default.writeFile(path_1.default.join(this.tempDir, "Cargo.toml"), cargoToml);
        // Write source code
        await promises_1.default.writeFile(path_1.default.join(this.tempDir, "src", "lib.rs"), sourceCode);
    }
    async parseABI(sourceCode) {
        const methods = [];
        const opcodes = {};
        // Updated regex to better match method comments and opcodes
        // This looks for comments between match arms
        const methodRegex = /\/\*\s*([^*]+?)\s*\*\/\s*(\d+)\s*=>/g;
        let match;
        while ((match = methodRegex.exec(sourceCode)) !== null) {
            const [_, comment, opcode] = match;
            const opcodeNum = parseInt(opcode);
            // Parse method signature from comment
            const methodInfo = this.parseMethodSignature(comment.trim());
            // Add to methods array
            const method = {
                opcode: opcodeNum,
                name: methodInfo.name,
                inputs: methodInfo.inputs,
                outputs: methodInfo.outputs,
            };
            methods.push(method);
            // Add to opcodes map
            opcodes[methodInfo.name] = opcodeNum;
        }
        // Parse struct name (captures pub struct Name)
        const structRegex = /pub\s+struct\s+(\w+)/;
        const structMatch = sourceCode.match(structRegex);
        const name = structMatch ? structMatch[1] : "UnknownContract";
        // Parse storage
        const storage = [];
        const storageRegex = /StoragePointer::from_keyword\("([^"]+)"\)/g;
        let storageMatch;
        while ((storageMatch = storageRegex.exec(sourceCode)) !== null) {
            storage.push({
                key: storageMatch[1],
                type: "Vec<u8>", // Default type
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
    parseMethodSignature(comment) {
        // Updated regex to better handle method signatures
        const match = comment.trim().match(/(\w+)\((.*?)\)/);
        if (!match) {
            return { name: "unknown", inputs: [], outputs: [] };
        }
        const [_, name, paramsStr] = match;
        const inputs = paramsStr
            .split(",")
            .map((param) => param.trim())
            .filter((param) => param.length > 0)
            .map((param, index) => {
            // Parse array types like "u128[2]"
            const arrayMatch = param.match(/(\w+)\[(\d+)\]/);
            if (arrayMatch) {
                const [_, baseType, length] = arrayMatch;
                return {
                    name: `param${index}`,
                    type: {
                        array: {
                            type: baseType,
                            length: parseInt(length),
                        },
                    },
                };
            }
            // Handle primitive types
            return {
                name: `param${index}`,
                type: param,
            };
        });
        return {
            name,
            inputs,
            outputs: [], // Could parse return types later
        };
    }
}
exports.AlkanesCompiler = AlkanesCompiler;
//# sourceMappingURL=compiler.js.map