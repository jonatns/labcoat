import { isDeepStrictEqual } from "node:util";

export class AssertionError extends Error {
  constructor(message: string) {
    super(message);
    this.name = "AssertionError";
  }
}

export function expectEqual<T>(actual: T, expected: T, message?: string) {
  const unwrap = (v: unknown) => {
    if (v instanceof String) return v.valueOf(); // unbox String objects
    if (
      typeof v === "object" &&
      v &&
      "toString" in v &&
      typeof v.toString === "function"
    ) {
      const str = v.toString();
      if (typeof str === "string" && str !== "[object Object]") {
        return str; // handle weird String-like wrappers
      }
    }
    return v;
  };

  const a = unwrap(actual);
  const e = unwrap(expected);

  // Explicit manual check for string equality
  if (typeof a === "string" && typeof e === "string") {
    if (a === e) return; // ✅ primitive equality check only
  }

  // Then check numbers, bigints, booleans, etc.
  if (Object.is(a, e)) return;

  // If both are objects, do JSON compare instead of isDeepStrictEqual
  try {
    if (JSON.stringify(a) === JSON.stringify(e)) return;
  } catch (_) {
    // ignore JSON stringify errors
  }

  const defaultMessage = `Expected ${formatValue(a)} to equal ${formatValue(
    e
  )}`;
  console.log("a", a, typeof a, Object.prototype.toString.call(a));
  console.log("e", e, typeof e, Object.prototype.toString.call(e));
  throw new AssertionError(message ?? defaultMessage);
}

export async function expectRevert(
  fn: () => unknown | Promise<unknown>,
  expectedMessage?: string
) {
  try {
    await fn();
  } catch (error) {
    if (!expectedMessage) {
      return;
    }

    const actual = (error as Error).message ?? "";
    if (actual.includes(expectedMessage)) {
      return;
    }

    throw new AssertionError(
      `Expected error message to include \"${expectedMessage}\" but received \"${actual}\"`
    );
  }

  throw new AssertionError(
    "Expected function to throw but it completed successfully"
  );
}

function formatValue(value: unknown) {
  if (typeof value === "bigint") {
    return `${value}n`;
  }

  if (typeof value === "string") {
    return `"${value}"`;
  }

  try {
    return JSON.stringify(value);
  } catch {
    return String(value);
  }
}
