import { exec } from "child_process";
import { promisify } from "util";
import fs from "fs/promises";
import path from "path";
import {
  AlkanesABI,
  AlkanesDeployment,
  AlkanesDeploymentStatus,
  AlkanesMethod,
  StorageKey,
} from "./types.js";
import { cargoTemplate } from "./cargo-template.js";

const execAsync = promisify(exec);

export class AlkanesCompiler {
  private tempDir: string = ".labcoat";

  async compile(
    sourceCode: string
  ): Promise<{ bytecode: string; abi: AlkanesABI } | void> {
    try {
      await this.createProject(sourceCode);

      const { stderr } = await execAsync(
        `cargo build --target=wasm32-unknown-unknown --release`,
        { cwd: this.tempDir }
      );

      if (stderr) {
        console.warn("Build warnings:", stderr);
      }

      const wasmPath = path.join(
        this.tempDir,
        "target",
        "wasm32-unknown-unknown",
        "release",
        "alkanes_contract.wasm"
      );
      const wasmBuffer = await fs.readFile(wasmPath);

      const abi = await this.parseABI(sourceCode);

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
    await fs.mkdir(this.tempDir, { recursive: true });
    await fs.mkdir(path.join(this.tempDir, "src"), { recursive: true });
    await fs.writeFile(path.join(this.tempDir, "Cargo.toml"), cargoTemplate);
    await fs.writeFile(path.join(this.tempDir, "src", "lib.rs"), sourceCode);
  }

  public async parseABI(sourceCode: string): Promise<AlkanesABI> {
    const methods: AlkanesMethod[] = [];
    const opcodes: Record<string, number> = {};

    // Match MessageDispatch enum variants with #[opcode(N)]
    const messageRegex = /#\[opcode\((\d+)\)\]\s*(\w+)/g;
    let match: RegExpExecArray | null;

    while ((match = messageRegex.exec(sourceCode)) !== null) {
      const [_, opcodeStr, methodNameRaw] = match;
      const opcodeNum = parseInt(opcodeStr, 10);

      // Convert enum variant name to snake_case for method name
      const methodName = methodNameRaw
        .replace(/([A-Z])/g, "_$1")
        .toLowerCase()
        .replace(/^_/, "");

      methods.push({
        opcode: opcodeNum,
        name: methodName,
        inputs: [],
        outputs: [],
      });

      opcodes[methodName] = opcodeNum;
    }

    // Parse struct name (pub struct Name)
    const structRegex = /pub\s+struct\s+(\w+)/;
    const structMatch = sourceCode.match(structRegex);
    const name = structMatch ? structMatch[1] : "UnknownContract";

    // Parse storage (StoragePointer::from_keyword)
    const storage: StorageKey[] = [];
    const storageRegex = /StoragePointer::from_keyword\("([^"]+)"\)/g;
    let storageMatch;
    while ((storageMatch = storageRegex.exec(sourceCode)) !== null) {
      storage.push({
        key: storageMatch[1],
        type: "Vec<u8>", // default type
      });
    }

    const deployment: AlkanesDeployment = {
      status: "not-deployed",
    };

    return {
      name,
      version: "1.0.0",
      methods,
      storage,
      opcodes,
      deployment,
    };
  }
}
