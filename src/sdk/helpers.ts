import { promisify } from "node:util";
import { gzip } from "zlib";
import { Provider } from "@oyl/sdk";

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

export async function waitForTrace(
  provider: Provider,
  txId: string,
  eventName: string
): Promise<any> {
  while (true) {
    try {
      const traces = await provider.alkanes.trace({ txid: txId, vout: 4 });

      if (Array.isArray(traces)) {
        const entry = traces.find((t) => t.event === eventName);
        if (entry) {
          return entry;
        }
      }
    } catch (err) {
      console.warn(`Trace not ready yet for ${eventName}, retrying...`, err);
    }

    await new Promise((resolve) => setTimeout(resolve, 1000));
  }
}

export function decodeRevertReason(hex: string): string | undefined {
  if (!hex || hex === "0x") return;
  try {
    const data = hex.slice(10);
    const buf = Buffer.from(data, "hex");
    const text = buf.toString("utf8");
    return text;
  } catch {
    return;
  }
}

function encodeArg(arg: string | number | bigint): string {
  if (typeof arg === "string" && arg.startsWith("0x")) {
    const cleanHex = arg.slice(2).padStart(32, "0").toLowerCase();
    return "0x" + cleanHex.match(/../g)!.reverse().join("");
  }

  if (typeof arg === "string") {
    const buf = Buffer.from(arg, "utf8");
    return "0x" + Buffer.from(buf).reverse().toString("hex");
  }

  if (typeof arg === "number" || typeof arg === "bigint") {
    const big = BigInt(arg);
    // Convert number → hex, pad to even length
    let hex = big.toString(16);
    if (hex.length % 2 !== 0) hex = "0" + hex;

    // Split into bytes and reverse for little-endian
    const reversed = hex.match(/../g)!.reverse().join("");

    // Pad to 16 bytes (u128)
    return "0x" + reversed.padEnd(32, "0");
  }

  throw new Error(
    `Unsupported argument type: ${typeof arg}. Must be string, number, or bigint.`
  );
}

export function encodeArgs(args: unknown[]): string[] {
  return args.map(encodeArg);
}

export function decodeAlkanesResult(result: any): string | number | bigint {
  if (!result.parsed) return result.execution?.data ?? null;

  const { string, be } = result.parsed;

  if (string && /^[\x20-\x7E\s]*$/.test(string)) {
    return string;
  }

  if (be) {
    const n = BigInt(be);
    return n <= Number.MAX_SAFE_INTEGER ? Number(n) : n;
  }

  return result.execution?.data ?? null;
}
