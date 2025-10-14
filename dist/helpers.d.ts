import { Provider } from "oyl-sdk";
export declare function waitForTrace(provider: Provider, txId: string, vout: number): Promise<any[]>;
export declare function decodeRevertReason(hex: string): string | undefined;
