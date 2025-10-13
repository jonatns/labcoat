import { Provider } from "oyl-sdk";
export declare function waitForTrace(provider: Provider, txId: string, vout: number, eventName?: string): Promise<{
    block: number;
    tx: number;
}>;
