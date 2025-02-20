import { AlkanesType, AlkanesPrimitive } from "./types";

export class AlkanesEncoder {
  encode(type: AlkanesType, value: any): Uint8Array {
    if (typeof type === "string") {
      return this.encodePrimitive(type, value);
    }

    if ("array" in type) {
      return this.encodeArray(type.array.type, value, type.array.length);
    }

    if ("vec" in type) {
      return this.encodeVec(type.vec.type, value);
    }

    if ("tuple" in type) {
      return this.encodeTuple(type.tuple, value);
    }

    throw new Error(`Unsupported type: ${JSON.stringify(type)}`);
  }

  private encodePrimitive(type: AlkanesPrimitive, value: any): Uint8Array {
    switch (type) {
      case "u8":
        return new Uint8Array([value]);

      case "u16":
        const u16 = new Uint16Array([value]);
        return new Uint8Array(u16.buffer);

      case "u32":
        const u32 = new Uint32Array([value]);
        return new Uint8Array(u32.buffer);

      case "u64":
      case "u128":
        // BigInt handling
        const bytes: number[] = [];
        let n = BigInt(value);
        while (n > 0n) {
          bytes.push(Number(n & 0xffn));
          n >>= 8n;
        }
        return new Uint8Array(bytes.reverse());

      case "String":
        const encoder = new TextEncoder();
        const strBytes = encoder.encode(value);
        // Prepend length
        const length = new Uint32Array([strBytes.length]);
        return new Uint8Array([...new Uint8Array(length.buffer), ...strBytes]);

      case "bool":
        return new Uint8Array([value ? 1 : 0]);

      case "Vec<u8>":
        if (!(value instanceof Uint8Array)) {
          throw new Error("Expected Uint8Array for Vec<u8>");
        }
        const lenBytes = new Uint32Array([value.length]);
        return new Uint8Array([...new Uint8Array(lenBytes.buffer), ...value]);

      default:
        throw new Error(`Unsupported primitive type: ${type}`);
    }
  }

  private encodeArray(
    type: AlkanesType,
    value: any[],
    length: number
  ): Uint8Array {
    if (!Array.isArray(value) || value.length !== length) {
      throw new Error(`Expected array of length ${length}`);
    }

    const parts = value.map((item) => this.encode(type, item));
    const totalLength = parts.reduce((sum, part) => sum + part.length, 0);

    const result = new Uint8Array(totalLength);
    let offset = 0;
    for (const part of parts) {
      result.set(part, offset);
      offset += part.length;
    }

    return result;
  }

  private encodeVec(type: AlkanesType, value: any[]): Uint8Array {
    if (!Array.isArray(value)) {
      throw new Error("Expected array for Vec");
    }

    const lenBytes = new Uint32Array([value.length]);
    const parts = value.map((item) => this.encode(type, item));
    const totalLength = parts.reduce((sum, part) => sum + part.length, 0);

    const result = new Uint8Array(4 + totalLength);
    result.set(new Uint8Array(lenBytes.buffer));

    let offset = 4;
    for (const part of parts) {
      result.set(part, offset);
      offset += part.length;
    }

    return result;
  }

  private encodeTuple(types: AlkanesType[], values: any[]): Uint8Array {
    if (!Array.isArray(values) || values.length !== types.length) {
      throw new Error(`Expected tuple of length ${types.length}`);
    }

    const parts = types.map((type, i) => this.encode(type, values[i]));
    const totalLength = parts.reduce((sum, part) => sum + part.length, 0);

    const result = new Uint8Array(totalLength);
    let offset = 0;
    for (const part of parts) {
      result.set(part, offset);
      offset += part.length;
    }

    return result;
  }

  decode(type: AlkanesType, data: Uint8Array): any {
    if (typeof type === "string") {
      return this.decodePrimitive(type, data);
    }

    if ("array" in type) {
      return this.decodeArray(type.array.type, data, type.array.length);
    }

    if ("vec" in type) {
      return this.decodeVec(type.vec.type, data);
    }

    if ("tuple" in type) {
      return this.decodeTuple(type.tuple, data);
    }

    throw new Error(`Unsupported type: ${JSON.stringify(type)}`);
  }

  // Implement decode methods similar to encode...
  private decodePrimitive(type: AlkanesPrimitive, data: Uint8Array) {
    // Implementation here
  }

  private decodeArray(type: AlkanesType, data: Uint8Array, length: number) {
    // Implementation here
  }

  private decodeVec(type: AlkanesType, data: Uint8Array) {
    // Implementation here
  }

  private decodeTuple(types: AlkanesType[], data: Uint8Array) {
    // Implementation here
  }
}
