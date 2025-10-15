import { InputType } from "node:zlib";
import { Provider } from "oyl-sdk";
export declare function gzipWasm(wasmBuffer: InputType): Promise<Buffer<ArrayBufferLike>>;
export declare function waitForTrace(provider: Provider, txId: string, eventName: string): Promise<any>;
export declare function decodeRevertReason(hex: string): string | undefined;
export declare function encodeArgs(args: unknown[]): string[];
export declare function decodeAlkanesResult(result: any): string | number | bigint;
