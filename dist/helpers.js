export async function waitForTrace(provider, txId, vout) {
    while (true) {
        try {
            const result = await provider.alkanes.trace({ txid: txId, vout });
            if (Array.isArray(result) && result.length > 0) {
                return result;
            }
        }
        catch (err) {
            console.warn("Trace not ready yet, retrying...", err);
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
