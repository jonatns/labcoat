import { AlkanesType } from "./types";
export declare class AlkanesEncoder {
    encode(type: AlkanesType, value: any): Uint8Array;
    private encodePrimitive;
    private encodeArray;
    private encodeVec;
    private encodeTuple;
    decode(type: AlkanesType, data: Uint8Array): any;
    private decodePrimitive;
    private decodeArray;
    private decodeVec;
    private decodeTuple;
}
