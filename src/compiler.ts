import { exec } from "child_process";
import { promisify } from "util";
import fs from "fs/promises";
import path from "path";
import {
  AlkanesABI,
  AlkanesMethod,
  AlkanesPrimitive,
  AlkanesType,
  StorageKey,
} from "./types";

const execAsync = promisify(exec);

export class AlkanesCompiler {
  private tempDir: string;

  constructor(tempDir: string = ".alkanes") {
    this.tempDir = tempDir;
  }

  async compile(
    sourceCode: string
  ): Promise<{ bytecode: string; abi: AlkanesABI } | void> {
    try {
      // Create temporary project
      await this.createProject(sourceCode);

      const { stdout, stderr } = await execAsync(
        "cargo build --release --target wasm32-unknown-unknown",
        { cwd: this.tempDir }
      );

      if (stderr) {
        console.warn("Build warnings:", stderr);
      }

      // Read the WASM file
      const wasmPath = path.join(
        this.tempDir,
        "target",
        "wasm32-unknown-unknown",
        "release",
        "alkanes_contract.wasm"
      );
      const wasmBuffer = await fs.readFile(wasmPath);

      // Parse ABI from source code
      const abi = await this.parseABI(sourceCode);

      // Clean up (optional)
      // await fs.rm(this.tempDir, { recursive: true, force: true });

      return {
        bytecode: wasmBuffer.toString("base64"),
        abi,
      };
    } catch (error) {
      if (error instanceof Error) {
        throw new Error(`Compilation failed: ${error.message}`);
      }
    }
  }

  private async createProject(sourceCode: string) {
    // Create project directory
    await fs.mkdir(this.tempDir, { recursive: true });
    await fs.mkdir(path.join(this.tempDir, "src"), { recursive: true });

    // Create Cargo.toml
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

    await fs.writeFile(path.join(this.tempDir, "Cargo.toml"), cargoToml);

    // Write source code
    await fs.writeFile(path.join(this.tempDir, "src", "lib.rs"), sourceCode);
  }

  public async parseABI(sourceCode: string): Promise<AlkanesABI> {
    const methods: AlkanesMethod[] = [];
    const opcodes: Record<string, number> = {};

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
      const method: AlkanesMethod = {
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
    const storage: StorageKey[] = [];
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

  private parseMethodSignature(comment: string): {
    name: string;
    inputs: Array<{ name: string; type: AlkanesType }>;
    outputs: Array<{ name: string; type: AlkanesType }>;
  } {
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
                type: baseType as AlkanesPrimitive,
                length: parseInt(length),
              },
            },
          };
        }

        // Handle primitive types
        return {
          name: `param${index}`,
          type: param as AlkanesPrimitive,
        };
      });

    return {
      name,
      inputs,
      outputs: [], // Could parse return types later
    };
  }
}
