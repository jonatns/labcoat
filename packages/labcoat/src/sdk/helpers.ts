import { promisify } from "node:util";
import { gzip } from "zlib";

const gzipAsync = promisify(gzip);

/**
 * Compresses a WebAssembly buffer using gzip.
 * Works in both Node and Bun environments.
 */
export async function gzipWasm(
  wasmBuffer: Buffer | Uint8Array
): Promise<Buffer> {
  try {
    const result = await gzipAsync(wasmBuffer);
    return Buffer.from(result);
  } catch (error) {
    console.error("❌ Failed to gzip wasm:", error);
    throw error;
  }
}

/**
 * Encode one call argument as the decimal u128 string the Rust core takes.
 * Same semantics as the oyl-era encodeArg: numbers pass through, 0x-hex
 * parses as an integer, and short strings become little-endian byte
 * integers (so string args produce identical cellpack values as before).
 */
export function encodeArg(arg: string | number | bigint): string {
  if (typeof arg === "number" || typeof arg === "bigint") {
    const big = BigInt(arg);
    if (big < 0n) throw new Error(`Negative args are not supported: ${arg}`);
    return big.toString(10);
  }

  if (typeof arg === "string" && arg.startsWith("0x")) {
    return BigInt(arg).toString(10);
  }

  if (typeof arg === "string") {
    const bytes = Buffer.from(arg, "utf8");
    if (bytes.length === 0 || bytes.length > 16) {
      throw new Error(`String arg must be 1..=16 bytes: "${arg}"`);
    }
    let value = 0n;
    for (let i = 0; i < bytes.length; i++) {
      value |= BigInt(bytes[i]) << BigInt(8 * i); // little-endian
    }
    return value.toString(10);
  }

  throw new Error(
    `Unsupported argument type: ${typeof arg}. Must be string, number, or bigint.`
  );
}

export function encodeArgs(args: unknown[]): string[] {
  return args.map((a) => encodeArg(a as string | number | bigint));
}

/**
 * Decode a revert reason payload (0x + 4-byte selector + UTF-8 message).
 * Retained for compatibility; the Rust core decodes reasons itself.
 */
export function decodeRevertReason(hex: string): string | undefined {
  if (!hex || hex === "0x") return;
  try {
    const data = hex.slice(10);
    const buf = Buffer.from(data, "hex");
    return buf.toString("utf8");
  } catch {
    return;
  }
}
