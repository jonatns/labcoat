import fs from "fs/promises";
import path from "path";
import { pathToFileURL } from "url";
import { WASI } from "wasi";
import { expect } from "chai";
import type { AlkanesABI } from "@/sdk/types.js";
import { importTypeScriptModule } from "@/sdk/utils/ts-runner.js";

/* ===========================
 * Console colors
 * =========================== */
const COLOR_GREEN = "\u001b[32m";
const COLOR_RED = "\u001b[31m";
const COLOR_CYAN = "\u001b[36m";
const COLOR_DIM = "\u001b[2m";
const COLOR_RESET = "\u001b[0m";

/* ===========================
 * Test module types
 * =========================== */
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
  expectEqual: (a: any, b: any, msg?: string) => Chai.Assertion;
  expectRevert: (fn: () => Promise<any>, msg?: string) => Promise<void>;
}

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

/* ===========================
 * Minimal WASM test runtime
 * - Provides context packing (u128 LE)
 * - Calls __execute and decodes the unified response:
 *   [incoming parcel...] + [raw data bytes]
 * - Returns both raw bytes (`data`) and a convenient `dataText`
 * =========================== */
export class TestRuntime {
  private module?: WebAssembly.Module;
  private instance?: WebAssembly.Instance;
  private memory?: WebAssembly.Memory;
  private wasi?: WASI;
  private readonly wasmPath: string;
  private abi?: AlkanesABI;

  // Bytes returned by __request_context / read by __load_context
  private currentContextBytes: Uint8Array = new Uint8Array(0);

  // Deterministic simulated chain context
  private simulatedContext = {
    myself: { block: 1n, tx: 1n },
    caller: { block: 2n, tx: 2n },
    vout: 0n,
    incoming: [] as Array<{ block: bigint; tx: bigint; value: bigint }>,
  };

  constructor(wasmPath: string, abi?: AlkanesABI) {
    this.wasmPath = wasmPath;
    this.abi = abi;
  }

  /* ---------- lifecycle ---------- */
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

    return {
      ...wasi.getImportObject(),
      env: {
        println: (ptr: number, len: number) => this.handlePrintln(ptr, len),
        abort: (_m: number, _f: number, line: number, column: number) => {
          console.error(
            `${COLOR_RED}⚠️  WASM abort at ${line}:${column}${COLOR_RESET}`
          );
          throw new Error(`WASM abort at ${line}:${column}`);
        },
        __request_context: () => this.currentContextBytes.length,
        __load_context: (ptr: number) => {
          if (!this.memory) return;
          new Uint8Array(this.memory.buffer).set(this.currentContextBytes, ptr);
        },
      },
    };
  }

  public async instantiate() {
    await this.ensureModule();
    const imports = this.createImports();

    const instance = await WebAssembly.instantiate(this.module!, imports);
    this.instance = instance;

    const memoryExport = instance.exports.memory;
    if (memoryExport instanceof WebAssembly.Memory) {
      this.memory = memoryExport;
    }

    this.wasi?.initialize(instance);
    return instance;
  }

  public async reset() {
    this.instance = undefined;
    this.memory = undefined;
    this.wasi = undefined;
  }

  /* ---------- context setters ---------- */
  public setMyself(block: bigint, tx: bigint) {
    this.simulatedContext.myself = { block, tx };
  }
  public setCaller(block: bigint, tx: bigint) {
    this.simulatedContext.caller = { block, tx };
  }
  public setVout(vout: bigint) {
    this.simulatedContext.vout = vout;
  }
  public setIncoming(
    incoming: Array<{ block: bigint; tx: bigint; value: bigint }>
  ) {
    this.simulatedContext.incoming = incoming.slice();
  }
  public addIncoming(x: { block: bigint; tx: bigint; value: bigint }) {
    this.simulatedContext.incoming.push(x);
  }
  public clearIncoming() {
    this.simulatedContext.incoming = [];
  }

  /* ---------- main call ---------- */
  public async call(method: string, ...args: unknown[]) {
    const instance = await (this.instance ? this.instance : this.instantiate());
    const executeFn = instance.exports["__execute"];
    if (typeof executeFn !== "function") {
      throw new Error("Contract does not export __execute");
    }

    const opcode = this.abi?.opcodes?.[method];
    if (opcode === undefined) throw new Error(`Unknown method: ${method}`);

    // Build request context: header + incoming + opcode + args-as-u128-words
    this.currentContextBytes = this.serializeContext(opcode, args);

    let ptr: number;
    try {
      ptr = (executeFn as () => number)();
    } catch (e) {
      console.error("❌ WASM trapped:", e);
      throw e;
    }
    return this.readResponse(ptr);
  }

  /* ---------- decoding ---------- */
  private readResponse(ptr: number) {
    if (!this.memory || ptr === 0)
      return {
        parcel: [] as Array<{ block: bigint; tx: bigint; value: bigint }>,
        data: new Uint8Array(),
        dataText: "",
      };

    const lenPtr = ptr - 4;
    const view = new DataView(this.memory.buffer);
    const totalLen = view.getUint32(lenPtr, true);
    const bytes = new Uint8Array(this.memory.buffer, ptr, totalLen);

    // incoming parcel
    let off = 0;
    const parcel: Array<{ block: bigint; tx: bigint; value: bigint }> = [];
    let nItems: bigint;
    [nItems, off] = this.readU128LE(bytes, off);
    for (let i = 0n; i < nItems; i++) {
      let b, t, v;
      [b, off] = this.readU128LE(bytes, off);
      [t, off] = this.readU128LE(bytes, off);
      [v, off] = this.readU128LE(bytes, off);
      parcel.push({ block: b, tx: t, value: v });
    }

    // raw data payload + convenience text
    const data = bytes.slice(off);
    const dataText = this.decodeUTF8WithoutPrefix(data);

    return { parcel, data, dataText };
  }

  private handlePrintln(ptr: number, len: number) {
    if (!this.memory) return;
    const bytes = new Uint8Array(this.memory.buffer, ptr, len);
    const text = new TextDecoder().decode(bytes);
    console.log(`${COLOR_DIM}📝 ${text}${COLOR_RESET}`);
  }

  private decodeUTF8WithoutPrefix(data: Uint8Array): string {
    let view = data;
    // Drop optional 4-byte 0x00000000 prefix if present
    if (
      view.length >= 4 &&
      view[0] === 0 &&
      view[1] === 0 &&
      view[2] === 0 &&
      view[3] === 0
    ) {
      view = view.slice(4);
    }
    return new TextDecoder().decode(view).replace(/\0+$/, "");
  }

  /* ---------- context & args serialization (u128 LE) ---------- */
  private serializeContext(opcode: number, args: unknown[]): Uint8Array {
    const parts: Uint8Array[] = [];

    // header
    this.pushU128LE(parts, this.simulatedContext.myself.block);
    this.pushU128LE(parts, this.simulatedContext.myself.tx);
    this.pushU128LE(parts, this.simulatedContext.caller.block);
    this.pushU128LE(parts, this.simulatedContext.caller.tx);
    this.pushU128LE(parts, this.simulatedContext.vout);

    // incoming_alkanes
    this.pushU128LE(parts, BigInt(this.simulatedContext.incoming.length));
    for (const p of this.simulatedContext.incoming) {
      this.pushU128LE(parts, p.block);
      this.pushU128LE(parts, p.tx);
      this.pushU128LE(parts, p.value);
    }

    // inputs: opcode + args
    this.pushU128LE(parts, BigInt(opcode));
    for (const a of args) {
      if (typeof a === "string") {
        this.packStringArg(parts, a);
      } else if (typeof a === "bigint") {
        this.pushU128LE(parts, a);
      } else if (typeof a === "number") {
        this.pushU128LE(parts, BigInt(a));
      } else {
        throw new Error(`Unsupported arg type: ${typeof a}`);
      }
    }

    // flatten
    const total = parts.reduce((n, u) => n + u.length, 0);
    const out = new Uint8Array(total);
    let off = 0;
    for (const u of parts) {
      out.set(u, off);
      off += u.length;
    }
    return out;
  }

  private packStringArg(dst: Uint8Array[], s: string) {
    const bytes = new TextEncoder().encode(s);
    // emit ceil(len/16) u128 words (LE), zero-padded; no length prefix
    let acc = 0n,
      shift = 0n,
      inChunk = 0;
    for (let i = 0; i < bytes.length; i++) {
      acc |= BigInt(bytes[i]) << (8n * shift);
      shift += 1n;
      inChunk += 1;
      if (inChunk === 16) {
        this.pushU128LE(dst, acc);
        acc = 0n;
        shift = 0n;
        inChunk = 0;
      }
    }
    if (inChunk > 0) this.pushU128LE(dst, acc);
  }

  private readU128LE(buf: Uint8Array, o: number): [bigint, number] {
    let v = 0n;
    for (let i = 0; i < 16; i++) v |= BigInt(buf[o + i]) << BigInt(8 * i);
    return [v, o + 16];
  }

  private pushU128LE(dst: Uint8Array[], v: bigint) {
    const b = new Uint8Array(16);
    for (let i = 0; i < 16; i++) b[i] = Number((v >> BigInt(8 * i)) & 0xffn);
    dst.push(b);
  }

  /* ---------- utility ---------- */
  public getAvailableExports(): string[] {
    if (!this.instance)
      throw new Error("Runtime not instantiated. Call instantiate() first.");
    return Object.keys(this.instance.exports).filter(
      (key) => typeof this.instance!.exports[key] === "function"
    );
  }

  public getExports() {
    if (!this.instance)
      throw new Error("Runtime not instantiated. Call instantiate() first.");
    return this.instance.exports;
  }

  public getMemory() {
    if (!this.memory) throw new Error("Contract memory is not available");
    return this.memory;
  }
}

/* ===========================
 * Test discovery & execution
 * =========================== */
async function discoverTestFiles(projectRoot: string) {
  const testDir = path.join(projectRoot, "tests");
  try {
    const stats = await fs.stat(testDir);
    if (!stats.isDirectory()) return [];
  } catch {
    return [];
  }

  const entries = await fs.readdir(testDir, { withFileTypes: true });
  return entries
    .filter(
      (e) =>
        e.isFile() &&
        (e.name.endsWith(".spec.js") || e.name.endsWith(".spec.ts"))
    )
    .map((e) => path.join(testDir, e.name))
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
        tests.push({ name: value.name, fn: value.fn as any });
      }
    }
  } else if (typeof defaultExport === "function") {
    tests.push({
      name: (defaultExport as Function).name || "default",
      fn: defaultExport as any,
    });
  }

  const seen = new Set(tests.map((t) => t.name));
  for (const [name, exported] of Object.entries(module)) {
    if (
      ["default", "beforeAll", "afterAll", "beforeEach", "afterEach"].includes(
        name
      )
    )
      continue;
    if (typeof exported === "function" && !seen.has(name)) {
      tests.push({ name, fn: exported as any });
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

  const expectEqual = (a: any, b: any, msg?: string) =>
    expect(a).to.equal(b, msg);
  const expectRevert = async (fn: () => Promise<any>, msg?: string) => {
    try {
      await fn();
      throw new Error("Expected function to revert, but it succeeded");
    } catch (err: any) {
      if (!msg) return;
      const message = err?.message ?? "";
      if (!message.includes(msg)) {
        throw new Error(
          `Expected error message to include "${msg}" but got "${message}"`
        );
      }
    }
  };

  const files = await discoverTestFiles(projectRoot);
  if (files.length === 0) {
    console.log("ℹ️  No test files found in ./tests. Skipping.");
    return { passed: 0, failed: 0, total: 0 };
  }

  let passed = 0,
    failed = 0;

  for (const file of files) {
    const relative = path.relative(projectRoot, file);
    console.log(`\n${COLOR_CYAN}📄 ${relative}${COLOR_RESET}`);

    let module: TestFileModule;
    try {
      module = await importTypeScriptModule(file);
    } catch (error) {
      failed += 1;
      const err = error as Error;
      console.error(
        `${COLOR_RED}  ❌ Failed to import test file: ${err.message}${COLOR_RESET}`
      );
      if (err.stack) {
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

    if (typeof hooks.beforeAll === "function") {
      await hooks.beforeAll({ runtime, expectEqual, expectRevert });
    }

    if (tests.length === 0) {
      console.log(
        `${COLOR_DIM}  ⚠️  No tests found in ${relative}${COLOR_RESET}`
      );
      continue;
    }

    for (const test of tests) {
      if (typeof hooks.beforeEach === "function") {
        await hooks.beforeEach({ runtime, expectEqual, expectRevert });
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
        if (typeof hooks.afterEach === "function") {
          await hooks.afterEach({ runtime, expectEqual, expectRevert });
        }
      }
    }

    if (typeof hooks.afterAll === "function") {
      await hooks.afterAll({ runtime, expectEqual, expectRevert });
    }
  }

  const total = passed + failed;
  const summaryColor = failed > 0 ? COLOR_RED : COLOR_GREEN;
  console.log(
    `\n${summaryColor}${passed}/${total} tests passed${COLOR_RESET} (${failed} failed)`
  );
  return { passed, failed, total };
}
