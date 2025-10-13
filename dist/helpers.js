export async function waitForTrace(provider, txId, vout, eventName = "create") {
    while (true) {
        try {
            const result = await provider.alkanes.trace({ txid: txId, vout });
            const entry = result.find(({ event }) => event === eventName);
            if (entry) {
                return {
                    block: Number(entry.data.block),
                    tx: Number(entry.data.tx),
                };
            }
        }
        catch (err) {
            console.warn("Trace not ready yet, retrying...", err);
        }
        await new Promise((resolve) => setTimeout(resolve, 1000));
    }
}
