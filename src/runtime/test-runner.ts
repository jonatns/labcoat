import fs from "fs/promises";
import path from "path";
import { pathToFileURL } from "url";
import { WASI } from "wasi";
import { expect } from "chai";

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
  expectEqual: (a: any, b: any, msg?: string) => Chai.Assertion;
  expectRevert: (fn: () => Promise<any>, msg?: string) => Promise<void>;
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
  private currentContextBytes: Uint8Array = new Uint8Array(0);
  private simulatedContext: {
    myself: { block: bigint; tx: bigint };
    caller: { block: bigint; tx: bigint };
    vout: bigint;
    incoming: Array<{ block: bigint; tx: bigint; value: bigint }>;
  } = {
    myself: { block: 1n, tx: 1n },
    caller: { block: 2n, tx: 2n },
    vout: 0n,
    incoming: [],
  };
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

    // const encoder = new TextEncoder();
    // const nameBytes = encoder.encode("World");

    // const chunks: Uint8Array[] = [];

    // // Utility to push a 16-byte little-endian u128
    // function pushU128(value: bigint) {
    //   const bytes = new Uint8Array(16);
    //   for (let i = 0; i < 16; i++) {
    //     bytes[i] = Number((value >> BigInt(8 * i)) & 0xffn);
    //   }
    //   chunks.push(bytes);
    // }

    // // ---- Context fields ----
    // // myself
    // pushU128(1n); // block
    // pushU128(1n); // tx

    // // caller
    // pushU128(2n); // block
    // pushU128(2n); // tx

    // // vout
    // pushU128(1n);

    // // incoming_alkanes
    // pushU128(1n); // len = 1
    // pushU128(10n); // id.block
    // pushU128(20n); // id.tx
    // pushU128(99n); // value

    // // Inputs: opcode + strlen(bytes) + packed bytes as u128 chunks
    // pushU128(1n); // opcode = 1 (Greet)

    // // pack "World" into a single 16-byte little-endian u128 (no length)
    // let acc = 0n;
    // let shift = 0n;
    // for (let i = 0; i < nameBytes.length; i++) {
    //   acc |= BigInt(nameBytes[i]) << (8n * shift);
    //   shift += 1n;
    // }

    // pushU128(acc);

    // // Merge all chunks into one buffer
    // let totalLength = chunks.reduce((a, c) => a + c.length, 0);
    // const serializedContext = new Uint8Array(totalLength);
    // let offset = 0;
    // for (const chunk of chunks) {
    //   serializedContext.set(chunk, offset);
    //   offset += chunk.length;
    // }

    return {
      ...wasi.getImportObject(),
      env: {
        println: (ptr: number, len: number) => this.handlePrintln(ptr, len),
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
        __request_context: () => this.currentContextBytes.length,
        __load_context: (ptr: number) => {
          if (!this.memory) return;
          new Uint8Array(this.memory.buffer).set(this.currentContextBytes, ptr);
        },
      },
    };
  }

  // ---- helpers: u128 <-> bytes (little-endian 16 bytes) ----
  private u128ToBytesLE(v: bigint): Uint8Array {
    const out = new Uint8Array(16);
    let x = v;
    for (let i = 0; i < 16; i++) {
      out[i] = Number(x & 0xffn);
      x >>= 8n;
    }
    return out;
  }

  private packStringArg = (dst: Uint8Array[], s: string) => {
    const bytes = new TextEncoder().encode(s);

    // emit ceil(len/16) u128 words, LE, zero-padded; NO length/word count prefix
    let acc = 0n;
    let shift = 0n;
    let inChunk = 0;

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
    if (inChunk > 0) {
      this.pushU128LE(dst, acc); // zero-padded tail
    }
  };

  private readU128LE(buf: Uint8Array, o: number): [bigint, number] {
    let v = 0n;
    for (let i = 0; i < 16; i++) v |= BigInt(buf[o + i]) << BigInt(8 * i);
    return [v, o + 16];
  }

  // Pack an arbitrary byte array as: [u128 byte_len] + ceil(len/16) × u128 words
  private packBytesAsU128Words(bytes: Uint8Array): Uint8Array[] {
    const chunks: Uint8Array[] = [];
    chunks.push(this.u128ToBytesLE(BigInt(bytes.length))); // exact byte length
    let acc = 0n,
      shift = 0n;
    for (let i = 0; i < bytes.length; i++) {
      acc |= BigInt(bytes[i]) << (8n * shift);
      shift += 1n;
      if (shift === 16n) {
        chunks.push(this.u128ToBytesLE(acc));
        acc = 0n;
        shift = 0n;
      }
    }
    if (shift > 0n) {
      chunks.push(this.u128ToBytesLE(acc)); // final partial word
    }
    return chunks;
  }

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

    const total = parts.reduce((n, u) => n + u.length, 0);
    const out = new Uint8Array(total);
    let off = 0;
    for (const u of parts) {
      out.set(u, off);
      off += u.length;
    }
    return out;
  }

  private pushU128LE = (dst: Uint8Array[], v: bigint) => {
    const b = new Uint8Array(16);
    for (let i = 0; i < 16; i++) b[i] = Number((v >> BigInt(8 * i)) & 0xffn);
    dst.push(b);
  };
  // Build the whole request context buffer deterministically
  private buildRequestContext(
    opcode: bigint,
    argBytes: Uint8Array
  ): Uint8Array {
    const parts: Uint8Array[] = [];

    // fixed header
    parts.push(this.u128ToBytesLE(this.simulatedContext.myself.block));
    parts.push(this.u128ToBytesLE(this.simulatedContext.myself.tx));
    parts.push(this.u128ToBytesLE(this.simulatedContext.caller.block));
    parts.push(this.u128ToBytesLE(this.simulatedContext.caller.tx));
    parts.push(this.u128ToBytesLE(this.simulatedContext.vout));

    // incoming_alkanes
    const incoming = this.simulatedContext.incoming;
    parts.push(this.u128ToBytesLE(BigInt(incoming.length)));
    for (const t of incoming) {
      parts.push(this.u128ToBytesLE(t.block));
      parts.push(this.u128ToBytesLE(t.tx));
      parts.push(this.u128ToBytesLE(t.value));
    }

    // inputs: opcode + arg byte array as u128 words
    parts.push(this.u128ToBytesLE(opcode));
    parts.push(...this.packBytesAsU128Words(argBytes));

    // flatten
    const total = parts.reduce((n, p) => n + p.length, 0);
    const out = new Uint8Array(total);
    let off = 0;
    for (const p of parts) {
      out.set(p, off);
      off += p.length;
    }
    return out;
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
      if (this.wasi) this.wasi.initialize(instance);
      return instance;
    } catch (error) {
      if (error instanceof WebAssembly.LinkError) {
        const message = error.message;
        const importMatch = message.match(/function="([^"]+)"/);
        if (importMatch) {
          const missingFunction = importMatch[1];
          throw new Error(
            `Missing required WASM import: ${missingFunction}. Add it to TestRuntime.createImports() env exports. Original error: ${message}`
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

  private async ensureInstance() {
    if (!this.instance) {
      await this.instantiate();
    }
    return this.instance!;
  }

  private writeString(str: string): number {
    if (!this.memory) throw new Error("Memory not available");
    const encoder = new TextEncoder();
    const bytes = encoder.encode(str);
    const buffer = new Uint8Array(this.memory.buffer);

    const ptr = Math.max(0, buffer.length - bytes.length - 8);

    const view = new DataView(this.memory.buffer);
    view.setUint32(ptr, bytes.length, true);
    buffer.set(bytes, ptr + 4);

    return ptr;
  }

  private readString(ptr: number): string {
    if (!this.memory || ptr === 0) return "";

    try {
      const view = new DataView(this.memory.buffer);
      const buffer = new Uint8Array(this.memory.buffer);

      if (ptr + 4 < buffer.length) {
        const length = view.getUint32(ptr, true);
        if (length > 0 && length < 10000 && ptr + 4 + length <= buffer.length) {
          const bytes = new Uint8Array(this.memory.buffer, ptr + 4, length);
          const decoder = new TextDecoder();
          const str = decoder.decode(bytes);
          if (str.length > 0 && /^[\x20-\x7E]*$/.test(str)) return str;
        }
      }

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

      let str = "";
      for (let i = 0; i < Math.min(1000, buffer.length - ptr); i++) {
        const byte = buffer[ptr + i];
        if (byte === 0) break;
        if (byte >= 32 && byte <= 126) str += String.fromCharCode(byte);
        else break;
      }
      if (str.length > 0) return str;
    } catch (error) {
      console.warn(`Failed to read string from pointer ${ptr}:`, error);
    }

    return "";
  }

  public readCallResponse(ptr: number): {
    data: string;
    dataPtr?: number;
    dataLen?: number;
  } {
    if (!this.memory || ptr === 0) return { data: "" };

    const view = new DataView(this.memory.buffer);
    const buffer = new Uint8Array(this.memory.buffer);

    try {
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

  public async call(method: string, ...args: unknown[]) {
    const instance = await this.ensureInstance();
    const executeFn = instance.exports["__execute"];
    if (typeof executeFn !== "function")
      throw new Error("Contract does not export __execute");

    const opcode = this.abi?.opcodes?.[method];
    if (opcode === undefined) throw new Error(`Unknown method: ${method}`);

    this.currentContextBytes = this.serializeContext(opcode, args);

    let ptr: number;
    try {
      ptr = (executeFn as () => number)();
    } catch (e) {
      console.error("❌ WASM trapped:", e);
      throw e;
    }
    return this.readResponse(ptr, method);
  }

  private decodeResponse(ptr: number): {
    parcel: Array<{ block: bigint; tx: bigint; value: bigint }>;
    data: Uint8Array;
    dataText: string;
  } {
    if (!this.memory || ptr === 0) {
      return { parcel: [], data: new Uint8Array(0), dataText: "" };
    }

    const lenPtr = ptr - 4;
    const view = new DataView(this.memory.buffer);
    const totalLen = view.getUint32(lenPtr, true);
    const bytes = new Uint8Array(this.memory.buffer, ptr, totalLen);

    let off = 0;
    // parcel_len
    const [parcelLenBI, off1] = this.readU128LE(bytes, off);
    off = off1;

    const parcel: Array<{ block: bigint; tx: bigint; value: bigint }> = [];
    for (let i = 0n; i < parcelLenBI; i++) {
      const [b, o1] = this.readU128LE(bytes, off);
      const [t, o2] = this.readU128LE(bytes, o1);
      const [v, o3] = this.readU128LE(bytes, o2);
      off = o3;
      parcel.push({ block: b, tx: t, value: v });
    }

    // The rest is **exactly** the data payload
    const data = bytes.slice(off);
    const dataText = new TextDecoder().decode(data);

    return { parcel, data, dataText };
  }

  public readStringFromMemory(ptr: number): string {
    const response = this.readCallResponse(ptr);
    if (response.data) return response.data;
    return this.readString(ptr);
  }

  private methodReturnsString(method: string): boolean {
    const m = this.abi?.methods?.find((x) => x.name === method);
    const ret = m?.outputs?.[0] ?? "";
    // match exactly `String` (allow whitespace)
    return ret.replace(/\s+/g, "") === "String";
  }

  private decodeUTF8WithoutPrefix(data: Uint8Array): string {
    let view = data;
    // Drop optional 4-byte prefix (often a vec length or placeholder)
    if (
      view.length >= 4 &&
      view[0] === 0 &&
      view[1] === 0 &&
      view[2] === 0 &&
      view[3] === 0
    ) {
      view = view.slice(4);
    }
    // Decode and trim trailing NULs
    return new TextDecoder().decode(view).replace(/\0+$/, "");
  }

  private readResponse(ptr: number, method: string) {
    if (!this.memory || ptr === 0)
      return { parcel: [], data: new Uint8Array(), dataText: "" };

    const lenPtr = ptr - 4;
    const view = new DataView(this.memory.buffer);
    const totalLen = view.getUint32(lenPtr, true);
    const bytes = new Uint8Array(this.memory.buffer, ptr, totalLen);

    // --- incoming parcel ---
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

    // The rest is the data payload
    const data = bytes.slice(off);
    const dataText = this.decodeUTF8WithoutPrefix(data);

    return { parcel, data, dataText };
  }

  public readResponseFromContext(): string {
    if (!this.memory) return "";
    if (!this.lastResponseData) return "";

    const bytes = this.lastResponseData;
    const decoder = new TextDecoder();
    const text = decoder.decode(bytes).replace(/\0+$/, "");
    console.log(`📝 Response from context: ${text}`);
    return text.trim();
  }

  private buildInputs(opcode: number, args: (number | bigint)[]): number {
    if (!this.memory) throw new Error("Memory not available");

    const view = new DataView(this.memory.buffer);
    const buffer = new Uint8Array(this.memory.buffer);

    const totalLen = 4 + args.length * 8;
    const ptr = buffer.length - totalLen;

    view.setUint32(ptr, opcode, true);
    for (let i = 0; i < args.length; i++) {
      view.setBigUint64(ptr + 4 + i * 8, BigInt(args[i]), true);
    }

    return ptr;
  }

  /**
   * Reads an Alkanes-style return value produced by response_to_i32()
   * Layout inside the returned slice (ptr..ptr+len):
   *   incoming_alkanes:
   *     - parcel_len: u128
   *     - for each of parcel_len: id.block u128, id.tx u128, value u128
   *   data: UTF-8 bytes (the message you set in CallResponse.data)
   */
  public readArrayBufferLayout(ptr: number): string {
    if (!this.memory || ptr === 0) return "";

    const lenPtr = ptr - 4;
    const view = new DataView(this.memory.buffer);
    const totalLen = view.getUint32(lenPtr, true);
    if (totalLen === 0 || totalLen > this.memory.buffer.byteLength) return "";

    const bytes = new Uint8Array(this.memory.buffer, ptr, totalLen);

    // --- Skip the incoming_alkanes parcel ---
    if (bytes.length < 16) return "";
    let off = 0;

    // parcel_len (u128)
    let parcelLen: bigint;
    [parcelLen, off] = this.readU128LE(bytes, off);

    // expected header bytes to skip: 16 (len) + parcelLen * 48 (three u128s)
    const headerBytes = 16 + Number(parcelLen) * 48;
    if (headerBytes > bytes.length) {
      // Defensive fallback: decode whole slice
      const fallback = new TextDecoder().decode(bytes).replace(/\0+$/, "");
      console.log("🧾 Raw bytes:", bytes);
      console.log("🧾 Fallback decoded:", fallback);
      return fallback;
    }

    off = headerBytes;

    let textBytes = bytes.slice(off);

    if (
      textBytes.length >= 4 &&
      textBytes[0] === 0 &&
      textBytes[1] === 0 &&
      textBytes[2] === 0 &&
      textBytes[3] === 0
    ) {
      textBytes = textBytes.slice(4);
    }

    const text = new TextDecoder().decode(textBytes).replace(/\0+$/, "");
    console.log("🧾 Raw bytes:", bytes);
    console.log("🧾 Skipped parcel bytes:", off);
    console.log("🧾 Decoded:", text);
    return text;
  }

  public inspectMemory(ptr: number, bytes: number = 64): string {
    if (!this.memory || ptr === 0) return "Invalid pointer";

    const buffer = new Uint8Array(this.memory.buffer);
    const view = new DataView(this.memory.buffer);

    if (ptr + bytes > buffer.length) bytes = buffer.length - ptr;

    let output = `Memory at ${ptr} (${bytes} bytes):\n`;
    output += `  Raw bytes: [${Array.from(
      buffer.slice(ptr, ptr + Math.min(bytes, 32))
    ).join(", ")}]\n`;

    if (ptr + 4 <= buffer.length) {
      const u32 = view.getUint32(ptr, true);
      output += `  As U32: ${u32}\n`;
    }
    if (ptr + 8 <= buffer.length) {
      const u32_0 = view.getUint32(ptr, true);
      const u32_1 = view.getUint32(ptr + 4, true);
      output += `  As (U32, U32): (${u32_0}, ${u32_1})\n`;
      if (u32_1 > 0 && u32_1 < 10000 && u32_0 + u32_1 <= buffer.length) {
        const vecBytes = buffer.slice(u32_0, u32_0 + u32_1);
        const decoder = new TextDecoder();
        const text = decoder.decode(vecBytes);
        if (/^[\x20-\x7E]*$/.test(text)) {
          output += `  As Vec<u8> string: "${text}"\n`;
        }
      }
    }

    let nullTerminated = "";
    for (let i = 0; i < Math.min(bytes, 100); i++) {
      const byte = buffer[ptr + i];
      if (byte === 0) break;
      if (byte >= 32 && byte <= 126)
        nullTerminated += String.fromCharCode(byte);
      else break;
    }
    if (nullTerminated.length > 0) {
      output += `  As null-terminated string: "${nullTerminated}"\n`;
    }

    return output;
  }

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
    tests.push({
      name: defaultExport.name || "default",
      fn: defaultExport as (context: TestContext) => unknown,
    });
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
      tests.push({ name, fn: exported as (context: TestContext) => unknown });
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
    // try {
    //   await runtime.instantiate();
    //   const exports = runtime.getAvailableExports();
    //   if (exports.length > 0) {
    //     console.log(
    //       `${COLOR_DIM}  📦 Available WASM exports: ${exports.join(
    //         ", "
    //       )}${COLOR_RESET}`
    //     );
    //   }
    //   await runtime.reset();
    // } catch (error) {
    //   // If instantiation fails, continue anyway - the test will handle it
    // }

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
