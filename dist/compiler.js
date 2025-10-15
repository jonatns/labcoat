import { exec } from "child_process";
import { promisify } from "util";
import fs from "fs/promises";
import path from "path";
import { cargoTemplate } from "./cargo-template.js";
import { gzipWasm } from "./helpers.js";
import { loadManifest, saveManifest } from "./manifest.js";
const execAsync = promisify(exec);
export class AlkanesCompiler {
    tempDir = ".labcoat";
    async compile(contractName, sourceCode) {
        try {
            await this.createProject(sourceCode);
            const { stderr } = await execAsync(`cargo clean && cargo build --target=wasm32-unknown-unknown --release`, { cwd: this.tempDir });
            if (stderr) {
                console.warn("Build warnings:", stderr);
            }
            // Read compiled WASM
            const wasmPath = path.join(this.tempDir, "target", "wasm32-unknown-unknown", "release", "alkanes_contract.wasm");
            const wasmBuffer = await fs.readFile(wasmPath);
            const abi = await this.parseABI(sourceCode);
            const buildDir = "./build";
            await fs.mkdir(buildDir, { recursive: true });
            const abiPath = `${buildDir}/${contractName}.abi.json`;
            const wasmOutPath = `${buildDir}/${contractName}.wasm.gz`;
            await fs.writeFile(abiPath, JSON.stringify(abi, null, 2));
            await fs.writeFile(wasmOutPath, await gzipWasm(wasmBuffer));
            const manifest = await loadManifest();
            manifest[contractName] = {
                ...(manifest[contractName] || {}),
                abi: abiPath,
                wasm: wasmOutPath,
                compiledAt: Date.now(),
            };
            await saveManifest(manifest);
            console.log(`✅ Compiled ${contractName}`);
            console.log(`- ABI: ${abiPath}`);
            console.log(`- WASM: ${wasmOutPath}`);
            return { wasmBuffer, abi };
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
        // Match enum variants
        const messageRegex = /#\[opcode\((\d+)\)\](?:\s*#\[returns\(([^)]+)\)\])?\s*([A-Za-z_][A-Za-z0-9_]*)\s*(?:\{([^}]*)\})?/gm;
        let match;
        while ((match = messageRegex.exec(sourceCode)) !== null) {
            const [, opcodeStr, returnsType, variantName, inputBlock] = match;
            const opcodeNum = parseInt(opcodeStr, 10);
            const outputs = returnsType ? [returnsType.trim()] : [];
            const inputs = [];
            if (inputBlock && inputBlock.trim().length > 0) {
                const fieldRegex = /(\w+)\s*:\s*([\w<>]+)/g;
                let fieldMatch;
                while ((fieldMatch = fieldRegex.exec(inputBlock)) !== null) {
                    const [, fieldName, fieldType] = fieldMatch;
                    inputs.push({ name: fieldName.trim(), type: fieldType.trim() });
                }
            }
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
            storage.push({ key: storageMatch[1], type: "Vec<u8>" });
        }
        return { name, version: "1.0.0", methods, storage, opcodes };
    }
}
