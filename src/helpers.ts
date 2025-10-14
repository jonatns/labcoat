import { Provider } from "oyl-sdk";

export async function waitForTrace(
  provider: Provider,
  txId: string,
  vout: number,
  eventName = "create"
) {
  while (true) {
    try {
      const result = await provider.alkanes.trace({ txid: txId, vout });

      const entry = result.find(({ event }) => event === eventName);
      if (entry) {
        return entry.data;
      }
    } catch (err) {
      console.warn("Trace not ready yet, retrying...", err);
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
