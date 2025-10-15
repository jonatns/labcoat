import { Provider } from "oyl-sdk";

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
  if (typeof arg === "string") {
    const buf = Buffer.from(arg, "utf8");
    return "0x" + Buffer.from(buf).reverse().toString("hex"); // reverse for little-endian
  }
  throw new Error(
    `Unsupported argument type: ${typeof arg}. Only string arguments are currently supported.`
  );
}

export function encodeArgs(args: unknown[]): string[] {
  return args.map(encodeArg);
}
