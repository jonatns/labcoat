export async function waitForTrace(provider, txId, vout, eventName) {
    while (true) {
        try {
            const traces = await provider.alkanes.trace({ txid: txId, vout });
            if (Array.isArray(traces)) {
                const entry = traces.find((t) => t.event === eventName);
                if (entry) {
                    return entry;
                }
            }
        }
        catch (err) {
            console.warn(`Trace not ready yet for ${eventName}, retrying...`, err);
        }
        await new Promise((resolve) => setTimeout(resolve, 1000));
    }
}
export function decodeRevertReason(hex) {
    if (!hex || hex === "0x")
        return;
    try {
        const data = hex.slice(10);
        const buf = Buffer.from(data, "hex");
        const text = buf.toString("utf8");
        return text;
    }
    catch {
        return;
    }
}
