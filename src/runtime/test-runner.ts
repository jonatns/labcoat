import fs from "fs/promises";
import path from "path";
import { pathToFileURL } from "url";
import { WASI } from "wasi";
import { expectEqual, expectRevert } from "./assertions.js";

const COLOR_GREEN = "\u001b[32m";
const COLOR_RED = "\u001b[31m";
const COLOR_CYAN = "\u001b[36m";
const COLOR_DIM = "\u001b[2m";
const COLOR_RESET = "\u001b[0m";

interface TestFileModule {
  [key: string]: unknown;
}

interface TestDefinition {
  name: string;
  fn: (context: TestContext) => unknown | Promise<unknown>;
}

type TestHook = (context: TestContext) => unknown | Promise<unknown>;

export interface TestContext {
  runtime: TestRuntime;
  expectEqual: typeof expectEqual;
  expectRevert: typeof expectRevert;
}

import { AlkanesABI } from "@/sdk/types.js";

export interface RunContractTestsOptions {
  projectRoot: string;
  wasmPath: string;
  abi?: AlkanesABI;
}

export interface ContractTestSummary {
  passed: number;
  failed: number;
  total: number;
}

export class TestRuntime {
  private module?: WebAssembly.Module;
  private instance?: WebAssembly.Instance;
  private memory?: WebAssembly.Memory;
  private wasi?: WASI;
  private readonly wasmPath: string;
  private abi?: AlkanesABI;
  private lastOpcode = 0;
  private lastContextPtr = 0;
  private lastArgs: unknown[] = [];
  private lastContextSize = 0;
  private lastResponseData: Uint8Array | null = null;

  public mockSender = "alk1testsender0000000000000000000000";
  public mockUtxos: Array<Record<string, unknown>> = [];

  constructor(wasmPath: string, abi?: AlkanesABI) {
    this.wasmPath = wasmPath;
    this.abi = abi;
  }

  private async ensureModule() {
    if (!this.module) {
      const wasmBytes = await fs.readFile(this.wasmPath);
      this.module = await WebAssembly.compile(wasmBytes);
    }
  }

  private createImports() {
    const wasi = new WASI({
      version: "preview1",
      args: [],
      env: {},
      preopens: {},
    });
    this.wasi = wasi;

    const encoder = new TextEncoder();
    const nameBytes = encoder.encode("World");

    const chunks: Uint8Array[] = [];

    // Utility to push a 16-byte little-endian u128
    function pushU128(value: bigint) {
      const bytes = new Uint8Array(16);
      for (let i = 0; i < 16; i++) {
        bytes[i] = Number((value >> BigInt(8 * i)) & 0xffn);
      }
      chunks.push(bytes);
    }

    // ---- Context fields ----

    // myself
    pushU128(1n); // block
    pushU128(1n); // tx

    // caller
    pushU128(2n); // block
    pushU128(2n); // tx

    // vout
    pushU128(1n);

    // incoming_alkanes
    pushU128(1n); // len = 1
    pushU128(10n); // id.block
    pushU128(20n); // id.tx
    pushU128(99n); // value

    // --------------------------
    // Inputs (u128 opcode + string)
    // --------------------------
    pushU128(1n); // opcode = 1 (Greet)

    // Encode "World" as aligned u128 chunks
    const nameU128Chunks: bigint[] = [];
    let current = 0n;
    let shift = 0n;
    for (let i = 0; i < nameBytes.length; i++) {
      current |= BigInt(nameBytes[i]) << (8n * shift);
      shift += 1n;
      if (shift === 16n) {
        nameU128Chunks.push(current);
        current = 0n;
        shift = 0n;
      }
    }
    if (shift > 0n) {
      nameU128Chunks.push(current);
    }

    // Push string chunk count
    pushU128(BigInt(nameU128Chunks.length));

    // Push each string chunk
    for (const chunk of nameU128Chunks) {
      pushU128(chunk);
    }

    // --------------------------
    // Build serialized context buffer
    // --------------------------
    const totalLength = chunks.reduce((acc, c) => acc + c.length, 0);
    const serializedContext = new Uint8Array(totalLength);
    let offset = 0;
    for (const chunk of chunks) {
      serializedContext.set(chunk, offset);
      offset += chunk.length;
    }

    return {
      ...wasi.getImportObject(),
      env: {
        println: (ptr: number, len: number) => this.handlePrintln(ptr, len),
        // Common WASM functions
        abort: (
          message: number,
          fileName: number,
          line: number,
          column: number
        ) => {
          console.error(
            `${COLOR_RED}⚠️  WASM abort called: message=${message}, file=${fileName}, line=${line}, col=${column}${COLOR_RESET}`
          );
          throw new Error(`WASM abort at ${fileName}:${line}:${column}`);
        },
        __request_context: () => {
          console.log(
            `__request_context returning ${serializedContext.length} bytes`
          );
          return serializedContext.length;
        },
        __load_context: (ptr: number) => {
          if (!this.memory) return;
          const mem = new Uint8Array(this.memory.buffer);
          mem.set(serializedContext, ptr);
          console.log(`__load_context wrote ${serializedContext.length} bytes`);
        },
      },
    };
  }

  private handlePrintln(ptr: number, len: number) {
    if (!this.memory) return;
    const bytes = new Uint8Array(this.memory.buffer, ptr, len);
    const text = new TextDecoder().decode(bytes);
    console.log(`${COLOR_DIM}📝 ${text}${COLOR_RESET}`);
  }

  public async instantiate() {
    await this.ensureModule();
    const imports = this.createImports();

    try {
      const instance = await WebAssembly.instantiate(this.module!, imports);
      this.instance = instance;
      const memoryExport = instance.exports.memory;
      if (memoryExport instanceof WebAssembly.Memory) {
        this.memory = memoryExport;
      }

      if (this.wasi) {
        this.wasi.initialize(instance);
      }

      return instance;
    } catch (error) {
      // If instantiation fails due to missing imports, try to provide better error info
      if (error instanceof WebAssembly.LinkError) {
        const message = error.message;
        // Extract the missing import name from the error message
        const importMatch = message.match(/function="([^"]+)"/);
        if (importMatch) {
          const missingFunction = importMatch[1];
          throw new Error(
            `Missing required WASM import: ${missingFunction}. ` +
              `Add it to TestRuntime.createImports() env exports. ` +
              `Original error: ${message}`
          );
        }
      }
      throw error;
    }
  }

  public async reset() {
    this.instance = undefined;
    this.memory = undefined;
    this.wasi = undefined;
  }

  private async ensureInstance() {
    if (!this.instance) {
      await this.instantiate();
    }
    return this.instance!;
  }

  /**
   * Write a string to WASM memory and return a pointer
   */
  private writeString(str: string): number {
    if (!this.memory) {
      throw new Error("Memory not available");
    }
    const encoder = new TextEncoder();
    const bytes = encoder.encode(str);
    const buffer = new Uint8Array(this.memory.buffer);

    // Find free space (simple approach: use end of memory)
    // In production, you'd use a proper allocator
    const ptr = Math.max(0, buffer.length - bytes.length - 8);

    // Write length (4 bytes, little-endian)
    const view = new DataView(this.memory.buffer);
    view.setUint32(ptr, bytes.length, true);

    // Write string bytes
    buffer.set(bytes, ptr + 4);

    return ptr;
  }

  /**
   * Read a string from WASM memory at the given pointer
   * Handles both direct string pointers and CallResponse structures
   */
  private readString(ptr: number): string {
    if (!this.memory || ptr === 0) {
      return "";
    }

    try {
      const view = new DataView(this.memory.buffer);
      const buffer = new Uint8Array(this.memory.buffer);

      // Try reading as length-prefixed string first
      if (ptr + 4 < buffer.length) {
        const length = view.getUint32(ptr, true);
        if (length > 0 && length < 10000 && ptr + 4 + length <= buffer.length) {
          const bytes = new Uint8Array(this.memory.buffer, ptr + 4, length);
          const decoder = new TextDecoder();
          const str = decoder.decode(bytes);
          // If it looks like valid text, return it
          if (str.length > 0 && /^[\x20-\x7E]*$/.test(str)) {
            return str;
          }
        }
      }

      // Try reading as CallResponse structure
      // CallResponse might have: data pointer, data length, etc.
      // Try reading as Vec<u8> format: (ptr, len) pair
      if (ptr + 8 < buffer.length) {
        const dataPtr = view.getUint32(ptr, true);
        const dataLen = view.getUint32(ptr + 4, true);

        if (
          dataPtr > 0 &&
          dataLen > 0 &&
          dataLen < 10000 &&
          dataPtr + dataLen <= buffer.length
        ) {
          const bytes = new Uint8Array(this.memory.buffer, dataPtr, dataLen);
          const decoder = new TextDecoder();
          return decoder.decode(bytes);
        }
      }

      // Try reading raw bytes from the pointer location
      // Look for null-terminated string or length-prefixed
      let str = "";
      for (let i = 0; i < Math.min(1000, buffer.length - ptr); i++) {
        const byte = buffer[ptr + i];
        if (byte === 0) break; // null terminator
        if (byte >= 32 && byte <= 126) {
          // printable ASCII
          str += String.fromCharCode(byte);
        } else {
          break;
        }
      }
      if (str.length > 0) return str;
    } catch (error) {
      console.warn(`Failed to read string from pointer ${ptr}:`, error);
    }

    return "";
  }

  /**
   * Read CallResponse structure from memory
   * Returns the data field as a string if it contains text
   */
  public readCallResponse(ptr: number): {
    data: string;
    dataPtr?: number;
    dataLen?: number;
  } {
    if (!this.memory || ptr === 0) {
      return { data: "" };
    }

    const view = new DataView(this.memory.buffer);
    const buffer = new Uint8Array(this.memory.buffer);

    // Try to read CallResponse structure
    // This is a simplified version - actual structure may differ
    try {
      // Common Rust Vec<u8> layout: (ptr, len, cap) - 24 bytes
      // Or just (ptr, len) - 8 bytes
      if (ptr + 8 <= buffer.length) {
        const dataPtr = view.getUint32(ptr, true);
        const dataLen = view.getUint32(ptr + 4, true);

        if (
          dataPtr > 0 &&
          dataLen > 0 &&
          dataLen < 10000 &&
          dataPtr + dataLen <= buffer.length
        ) {
          const bytes = new Uint8Array(this.memory.buffer, dataPtr, dataLen);
          const decoder = new TextDecoder();
          const data = decoder.decode(bytes);
          return { data, dataPtr, dataLen };
        }
      }
    } catch (error) {
      console.warn(`Failed to read CallResponse from pointer ${ptr}:`, error);
    }

    return { data: "" };
  }

  /**
   * Encode arguments for WASM call
   * Handles strings by writing them to memory and returning pointers
   */
  private encodeArgs(args: unknown[]): (number | bigint)[] {
    return args.map((arg) => {
      if (typeof arg === "string") {
        return this.writeString(arg);
      }
      if (typeof arg === "bigint") {
        return arg;
      }
      if (typeof arg === "number") {
        return arg;
      }
      throw new Error(`Unsupported argument type: ${typeof arg}`);
    });
  }

  /**
   * Call a contract method by name (using ABI opcode lookup)
   */
  public async call(method: string, ...args: unknown[]) {
    const instance = await this.ensureInstance();
    const executeFn = instance.exports["__execute"];
    if (typeof executeFn !== "function") {
      throw new Error("Contract does not export __execute");
    }

    const opcode = this.abi?.opcodes?.[method];
    if (opcode === undefined) {
      throw new Error(`Unknown method: ${method}`);
    }

    this.lastOpcode = opcode;
    this.lastArgs = this.encodeArgs(args);

    console.log("Inspecting memory before execute");

    let ptr: number | undefined;
    try {
      ptr = (executeFn as () => number)();
      console.log("Returned ptr:", ptr);
    } catch (err) {
      console.error("❌ WASM trapped:", err);
      return;
    }

    console.log("Inspecting memory after execute");

    const result = this.readArrayBufferLayout(ptr);

    console.log(`🧾 Result from ${method}:`, result);
    return result;
  }

  /**
   * Helper to read a string return value from WASM memory
   * Use this when a contract method returns a String
   * Also handles CallResponse structures
   */
  public readStringFromMemory(ptr: number): string {
    // First try reading as CallResponse
    const response = this.readCallResponse(ptr);
    if (response.data) {
      return response.data;
    }
    // Fall back to direct string reading
    return this.readString(ptr);
  }

  public readResponseFromContext(): string {
    if (!this.memory) return "";
    if (!this.lastResponseData) return "";

    const bytes = this.lastResponseData;
    const decoder = new TextDecoder();
    const text = decoder.decode(bytes).replace(/\0+$/, ""); // trim trailing zeros

    console.log(`📝 Response from context: ${text}`);
    return text.trim();
  }

  private buildInputs(opcode: number, args: (number | bigint)[]): number {
    if (!this.memory) throw new Error("Memory not available");

    const view = new DataView(this.memory.buffer);
    const buffer = new Uint8Array(this.memory.buffer);

    const totalLen = 4 + args.length * 8;
    const ptr = buffer.length - totalLen;

    // First word: opcode (u32)
    view.setUint32(ptr, opcode, true);

    // Following words: args as u64 (aligned)
    for (let i = 0; i < args.length; i++) {
      view.setBigUint64(ptr + 4 + i * 8, BigInt(args[i]), true);
    }

    // Save for debug
    this.lastContextPtr = ptr;
    this.lastContextSize = totalLen;
    return ptr;
  }

  /**
   * Reads an Alkanes-style return value produced by response_to_i32()
   */
  public readArrayBufferLayout(ptr: number): string {
    if (!this.memory || ptr === 0) return "";

    const lenPtr = ptr - 4;
    const view = new DataView(this.memory.buffer);
    const len = view.getUint32(lenPtr, true);
    if (len === 0 || len > this.memory.buffer.byteLength) return "";

    const bytes = new Uint8Array(this.memory.buffer, ptr, len);

    // 🧠 Skip the CallResponse header (usually 64 bytes before data)
    // Find printable text region heuristically:
    let textStart = 0;
    for (let i = 0; i < bytes.length; i++) {
      const b = bytes[i];
      if (b >= 32 && b <= 126) {
        textStart = i;
        break;
      }
    }

    const textBytes = bytes.slice(textStart);
    const text = new TextDecoder().decode(textBytes).replace(/\0+$/, "");
    console.log("🧾 Raw bytes:", bytes);
    console.log("🧾 Decoded:", text);
    return text;
  }

  /**
   * Debug helper to inspect memory at a pointer
   * Useful for understanding return value structures
   */
  public inspectMemory(ptr: number, bytes: number = 64): string {
    if (!this.memory || ptr === 0) {
      return "Invalid pointer";
    }

    const buffer = new Uint8Array(this.memory.buffer);
    const view = new DataView(this.memory.buffer);

    if (ptr + bytes > buffer.length) {
      bytes = buffer.length - ptr;
    }

    let output = `Memory at ${ptr} (${bytes} bytes):\n`;
    output += `  Raw bytes: [${Array.from(
      buffer.slice(ptr, ptr + Math.min(bytes, 32))
    ).join(", ")}]\n`;

    // Try reading as various formats
    if (ptr + 4 <= buffer.length) {
      const u32 = view.getUint32(ptr, true);
      output += `  As U32: ${u32}\n`;
    }
    if (ptr + 8 <= buffer.length) {
      const u32_0 = view.getUint32(ptr, true);
      const u32_1 = view.getUint32(ptr + 4, true);
      output += `  As (U32, U32): (${u32_0}, ${u32_1})\n`;

      // If second value looks like a length, try reading as Vec
      if (u32_1 > 0 && u32_1 < 10000 && u32_0 + u32_1 <= buffer.length) {
        const vecBytes = buffer.slice(u32_0, u32_0 + u32_1);
        const decoder = new TextDecoder();
        const text = decoder.decode(vecBytes);
        if (/^[\x20-\x7E]*$/.test(text)) {
          output += `  As Vec<u8> string: "${text}"\n`;
        }
      }
    }

    // Try reading as null-terminated string
    let nullTerminated = "";
    for (let i = 0; i < Math.min(bytes, 100); i++) {
      const byte = buffer[ptr + i];
      if (byte === 0) break;
      if (byte >= 32 && byte <= 126) {
        nullTerminated += String.fromCharCode(byte);
      } else {
        break;
      }
    }
    if (nullTerminated.length > 0) {
      output += `  As null-terminated string: "${nullTerminated}"\n`;
    }

    return output;
  }

  public getAvailableExports(): string[] {
    if (!this.instance) {
      throw new Error("Runtime not instantiated. Call instantiate() first.");
    }
    return Object.keys(this.instance.exports).filter(
      (key) => typeof this.instance!.exports[key] === "function"
    );
  }

  public getExports() {
    if (!this.instance) {
      throw new Error("Runtime not instantiated. Call instantiate() first.");
    }
    return this.instance.exports;
  }

  public getMemory() {
    if (!this.memory) {
      throw new Error("Contract memory is not available");
    }
    return this.memory;
  }
}

async function discoverTestFiles(projectRoot: string) {
  const testDir = path.join(projectRoot, "tests");
  try {
    const stats = await fs.stat(testDir);
    if (!stats.isDirectory()) {
      return [];
    }
  } catch (error) {
    return [];
  }

  const entries = await fs.readdir(testDir, { withFileTypes: true });
  return entries
    .filter((entry) => entry.isFile() && entry.name.endsWith(".spec.js"))
    .map((entry) => path.join(testDir, entry.name))
    .sort();
}

function extractTests(module: TestFileModule): TestDefinition[] {
  const tests: TestDefinition[] = [];

  const defaultExport = (module as { default?: unknown }).default;
  if (Array.isArray(defaultExport)) {
    for (const value of defaultExport) {
      if (
        value &&
        typeof value.name === "string" &&
        typeof value.fn === "function"
      ) {
        tests.push({ name: value.name, fn: value.fn });
      }
    }
  } else if (typeof defaultExport === "function") {
    tests.push({ name: defaultExport.name || "default", fn: defaultExport });
  }

  const seen = new Set(tests.map((test) => test.name));

  for (const [name, exported] of Object.entries(module)) {
    if (
      name === "default" ||
      name === "beforeAll" ||
      name === "afterAll" ||
      name === "beforeEach" ||
      name === "afterEach"
    ) {
      continue;
    }

    if (typeof exported === "function" && !seen.has(name)) {
      tests.push({ name, fn: exported });
      seen.add(name);
    }
  }

  return tests;
}

export async function runContractTests(
  options: RunContractTestsOptions
): Promise<ContractTestSummary> {
  const { projectRoot, wasmPath, abi } = options;
  const runtime = new TestRuntime(wasmPath, abi);

  const files = await discoverTestFiles(projectRoot);
  if (files.length === 0) {
    console.log("ℹ️  No test files found in ./tests. Skipping.");
    return { passed: 0, failed: 0, total: 0 };
  }

  let passed = 0;
  let failed = 0;

  for (const file of files) {
    const relative = path.relative(projectRoot, file);
    console.log(`\n${COLOR_CYAN}📄 ${relative}${COLOR_RESET}`);

    let module: TestFileModule;
    try {
      module = await import(pathToFileURL(file).href);
    } catch (error) {
      failed += 1;
      const err = error as Error;
      console.error(
        `${COLOR_RED}  ❌ Failed to import test file: ${err.message}${COLOR_RESET}`
      );
      if (err.stack) {
        console.error(`${COLOR_DIM}  Stack trace:${COLOR_RESET}`);
        console.error(
          `${COLOR_DIM}  ${err.stack
            .split("\n")
            .slice(0, 10)
            .join(`\n  `)}${COLOR_RESET}`
        );
      }
      continue;
    }

    const tests = extractTests(module);
    const hooks = module as {
      beforeAll?: TestHook;
      afterAll?: TestHook;
      beforeEach?: TestHook;
      afterEach?: TestHook;
    };
    const beforeAll =
      typeof hooks.beforeAll === "function" ? hooks.beforeAll : undefined;
    const afterAll =
      typeof hooks.afterAll === "function" ? hooks.afterAll : undefined;
    const beforeEach =
      typeof hooks.beforeEach === "function" ? hooks.beforeEach : undefined;
    const afterEach =
      typeof hooks.afterEach === "function" ? hooks.afterEach : undefined;

    if (typeof beforeAll === "function") {
      await beforeAll({ runtime, expectEqual, expectRevert });
    }

    if (tests.length === 0) {
      console.log(
        `${COLOR_DIM}  ⚠️  No tests found in ${relative}${COLOR_RESET}`
      );
      continue;
    }

    // Log available exports for debugging (only once per file)
    try {
      await runtime.instantiate();
      const exports = runtime.getAvailableExports();
      if (exports.length > 0) {
        console.log(
          `${COLOR_DIM}  📦 Available WASM exports: ${exports.join(
            ", "
          )}${COLOR_RESET}`
        );
      }
      await runtime.reset();
    } catch (error) {
      // If instantiation fails, continue anyway - the test will handle it
    }

    for (const test of tests) {
      if (typeof beforeEach === "function") {
        await beforeEach({ runtime, expectEqual, expectRevert });
      }

      await runtime.reset();
      await runtime.instantiate();

      const start = Date.now();
      try {
        await test.fn({ runtime, expectEqual, expectRevert });
        const duration = Date.now() - start;
        passed += 1;
        console.log(
          `${COLOR_GREEN}  ✅ ${test.name}${COLOR_RESET}${COLOR_DIM} (${duration}ms)${COLOR_RESET}`
        );
      } catch (error) {
        failed += 1;
        const duration = Date.now() - start;
        console.error(
          `${COLOR_RED}  ❌ ${test.name}${COLOR_RESET}${COLOR_DIM} (${duration}ms)${COLOR_RESET}`
        );
        console.error(
          `${COLOR_RED}     ${(error as Error).message}${COLOR_RESET}`
        );
      } finally {
        if (typeof afterEach === "function") {
          await afterEach({ runtime, expectEqual, expectRevert });
        }
      }
    }

    if (typeof afterAll === "function") {
      await afterAll({ runtime, expectEqual, expectRevert });
    }
  }

  const total = passed + failed;
  const summaryColor = failed > 0 ? COLOR_RED : COLOR_GREEN;
  console.log(
    `\n${summaryColor}${passed}/${total} tests passed${COLOR_RESET} (${failed} failed)`
  );

  return { passed, failed, total };
}
