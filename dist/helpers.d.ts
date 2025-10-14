import { Provider } from "oyl-sdk";
export declare function waitForTrace(provider: Provider, txId: string, eventName: string): Promise<any>;
export declare function decodeRevertReason(hex: string): string | undefined;
