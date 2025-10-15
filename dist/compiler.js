import { exec } from "child_process";
import { promisify } from "util";
import fs from "fs/promises";
import path from "path";
import { cargoTemplate } from "./cargo-template.js";
const execAsync = promisify(exec);
export class AlkanesCompiler {
    tempDir = ".labcoat";
    async compile(sourceCode) {
        try {
            await this.createProject(sourceCode);
            const { stderr } = await execAsync(`cargo clean && cargo build --target=wasm32-unknown-unknown --release`, { cwd: this.tempDir });
            if (stderr) {
                console.warn("Build warnings:", stderr);
            }
            const wasmPath = path.join(this.tempDir, "target", "wasm32-unknown-unknown", "release", "alkanes_contract.wasm");
            const wasmBuffer = await fs.readFile(wasmPath);
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
        await fs.mkdir(this.tempDir, { recursive: true });
        await fs.mkdir(path.join(this.tempDir, "src"), { recursive: true });
        await fs.writeFile(path.join(this.tempDir, "Cargo.toml"), cargoTemplate);
        await fs.writeFile(path.join(this.tempDir, "src", "lib.rs"), sourceCode);
    }
    async parseABI(sourceCode) {
        const methods = [];
        const opcodes = {};
        // Match enum variants with:
        // - #[opcode(N)]
        // - optional #[returns(Type)]
        // - variant name
        // - optional { inputs... }
        const messageRegex = /#\[opcode\((\d+)\)\](?:\s*#\[returns\(([^)]+)\)\])?\s*([A-Za-z_][A-Za-z0-9_]*)\s*(?:\{([^}]*)\})?/gm;
        let match;
        while ((match = messageRegex.exec(sourceCode)) !== null) {
            const [, opcodeStr, returnsType, variantName, inputBlock] = match;
            const opcodeNum = parseInt(opcodeStr, 10);
            const outputs = returnsType ? [returnsType.trim()] : [];
            const inputs = [];
            if (inputBlock && inputBlock.trim().length > 0) {
                // Split fields inside { ... }
                const fieldRegex = /(\w+)\s*:\s*([\w<>]+)/g;
                let fieldMatch;
                while ((fieldMatch = fieldRegex.exec(inputBlock)) !== null) {
                    const [, fieldName, fieldType] = fieldMatch;
                    inputs.push({
                        name: fieldName.trim(),
                        type: fieldType.trim(),
                    });
                }
            }
            console.log(`Parsed method: ${variantName} (opcode: ${opcodeNum})`);
            console.log(`  Inputs: ${JSON.stringify(inputs)}`);
            console.log(`  Outputs: ${JSON.stringify(outputs)}`);
            methods.push({
                opcode: opcodeNum,
                name: variantName,
                inputs,
                outputs,
            });
            opcodes[variantName] = opcodeNum;
        }
        // Parse struct name(s)
        const structRegex = /pub\s+struct\s+(\w+)/g;
        const structNames = [];
        let structMatch;
        while ((structMatch = structRegex.exec(sourceCode)) !== null) {
            structNames.push(structMatch[1]);
        }
        const name = structNames.length > 0 ? structNames[0] : "UnknownContract";
        // Parse storage pointers
        const storage = [];
        const storageRegex = /StoragePointer::from_keyword\("([^"]+)"\)/g;
        let storageMatch;
        while ((storageMatch = storageRegex.exec(sourceCode)) !== null) {
            storage.push({
                key: storageMatch[1],
                type: "Vec<u8>",
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
