import { exec } from "child_process";
import { promisify } from "util";
import fs from "fs/promises";
import path from "path";
import {
  AlkanesABI,
  AlkanesMethod,
  AlkanesInput,
  StorageKey,
  LabcoatConfig,
} from "./types.js";
import { cargoTemplate } from "./cargo-template.js";
import { gzipWasm } from "./helpers.js";
import { loadManifest, saveManifest } from "./manifest.js";
import { nanoid } from "nanoid";
import { loadConfig } from "./config.js";

const execAsync = promisify(exec);

export class AlkanesCompiler {
  private baseDir: string;
  private cleanupAfter: boolean;
  private config: LabcoatConfig;

  constructor(options?: { baseDir?: string; cleanup?: boolean }) {
    this.baseDir = options?.baseDir ?? path.join(process.cwd(), ".labcoat");
    this.cleanupAfter = options?.cleanup ?? true;
  }

  public async init() {
    this.config = await loadConfig();
  }

  private async getTempDir() {
    const id = nanoid(10);
    const dir = path.join(this.baseDir, `build_${id}`);
    await fs.mkdir(dir, { recursive: true });
    return dir;
  }

  async compile(
    contractName: string,
    sourcePathOrCode: string
  ): Promise<{ wasmBuffer: Buffer; abi: AlkanesABI }> {
    if (!this.config || Object.keys(this.config).length === 0) {
      await this.init();
    }

    const tempDir = await this.getTempDir();

    try {
      console.log(`🧱 Building in ${tempDir}`);

      const stat = await fs.stat(sourcePathOrCode).catch(() => null);
      if (stat?.isDirectory()) {
        // Multi-file contract folder
        await this.copyProject(sourcePathOrCode, tempDir);
      } else {
        // Single-file fallback
        await this.createProject(tempDir, sourcePathOrCode);
      }

      const { stdout, stderr } = await execAsync(
        `cargo clean && cargo build --target=wasm32-unknown-unknown --release`,
        { cwd: tempDir }
      );

      if (stderr?.trim()) console.warn("⚠️ Build warnings:", stderr);
      if (stdout?.trim()) console.log(stdout);

      const wasmPath = path.join(
        tempDir,
        "target",
        "wasm32-unknown-unknown",
        "release",
        "alkanes_contract.wasm"
      );

      const wasmBuffer = await fs.readFile(wasmPath);
      const abi = await this.parseABI(sourcePathOrCode);

      const buildDir = path.join(process.cwd(), "build");
      await fs.mkdir(buildDir, { recursive: true });

      const abiPath = path.join(buildDir, `${contractName}.abi.json`);
      const wasmOutPath = path.join(buildDir, `${contractName}.wasm.gz`);

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
    } finally {
      if (this.cleanupAfter) {
        await fs.rm(tempDir, { recursive: true, force: true }).catch(() => {});
      }
    }
  }

  private async createProject(tempDir: string, sourceCode: string) {
    await fs.mkdir(tempDir, { recursive: true });
    await fs.mkdir(path.join(tempDir, "src"), { recursive: true });

    const deps = this.config?.compiler?.dependencies ?? {};

    const cargoToml =
      cargoTemplate +
      `${Object.entries(deps)
        .map(([name, version]) => `${name} = "${version}"`)
        .join("\n")}`;

    await fs.writeFile(path.join(tempDir, "Cargo.toml"), cargoToml);
    await fs.writeFile(path.join(tempDir, "src", "lib.rs"), sourceCode);
  }

  private async copyProject(contractDir: string, tempDir: string) {
    const srcDir = path.join(tempDir, "src");
    await fs.mkdir(srcDir, { recursive: true });

    // copy every .rs file recursively
    const entries = await fs.readdir(contractDir, { withFileTypes: true });
    for (const entry of entries) {
      const srcPath = path.join(contractDir, entry.name);
      const destPath = path.join(srcDir, entry.name);

      if (entry.isDirectory()) {
        await this.copyProject(srcPath, path.join(tempDir, "src", entry.name));
      } else if (entry.isFile() && entry.name.endsWith(".rs")) {
        await fs.copyFile(srcPath, destPath);
      }
    }

    const deps = this.config?.compiler?.dependencies ?? {};

    const cargoToml =
      cargoTemplate +
      `${Object.entries(deps)
        .map(([name, version]) => `${name} = "${version}"`)
        .join("\n")}`;

    await fs.writeFile(path.join(tempDir, "Cargo.toml"), cargoToml);
  }

  public async scaffoldProject(tempDir: string, sourcePathOrCode: string) {
    const stat = await fs.stat(sourcePathOrCode).catch(() => null);
    if (stat?.isDirectory()) {
      await this.copyProject(sourcePathOrCode, tempDir);
    } else {
      await this.createProject(tempDir, sourcePathOrCode);
    }
  }

  public async parseABI(sourcePathOrCode: string): Promise<AlkanesABI> {
    let sourceCode = sourcePathOrCode;

    const stat = await fs.stat(sourcePathOrCode).catch(() => null);
    if (stat?.isDirectory()) {
      const libPath = path.join(sourcePathOrCode, "lib.rs");
      try {
        sourceCode = await fs.readFile(libPath, "utf8");
      } catch {
        throw new Error(`Missing lib.rs in ${sourcePathOrCode}`);
      }
    }

    const methods: AlkanesMethod[] = [];
    const opcodes: Record<string, number> = {};

    const messageRegex =
      /#\[opcode\((\d+)\)\](?:\s*#\[returns\(([^)]+)\)\])?\s*([A-Za-z_][A-Za-z0-9_]*)\s*(?:\{([^}]*)\})?/gm;

    let match: RegExpExecArray | null;
    while ((match = messageRegex.exec(sourceCode)) !== null) {
      const [, opcodeStr, returnsType, variantName, inputBlock] = match;
      const opcodeNum = parseInt(opcodeStr, 10);
      const outputs = returnsType ? [returnsType.trim()] : [];

      const inputs: AlkanesInput[] = [];
      if (inputBlock && inputBlock.trim().length > 0) {
        const fieldRegex = /(\w+)\s*:\s*([\w<>]+)/g;
        let fieldMatch: RegExpExecArray | null;
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

    const structRegex = /pub\s+struct\s+(\w+)/g;
    const structNames: string[] = [];
    let structMatch: RegExpExecArray | null;
    while ((structMatch = structRegex.exec(sourceCode)) !== null) {
      structNames.push(structMatch[1]);
    }

    const name = structNames.length > 0 ? structNames[0] : "UnknownContract";

    const storage: StorageKey[] = [];
    const storageRegex = /StoragePointer::from_keyword\("([^"]+)"\)/g;
    let storageMatch: RegExpExecArray | null;
    while ((storageMatch = storageRegex.exec(sourceCode)) !== null) {
      storage.push({ key: storageMatch[1], type: "Vec<u8>" });
    }

    return { name, version: "1.0.0", methods, storage, opcodes };
  }
}
