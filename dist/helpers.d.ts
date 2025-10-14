import { Provider } from "oyl-sdk";
export declare function waitForTrace(provider: Provider, txId: string, vout: number, eventName?: string): Promise<any>;
export declare function decodeRevertReason(hex: string): string | undefined;
