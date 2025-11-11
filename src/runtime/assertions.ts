import { isDeepStrictEqual } from "node:util";

export class AssertionError extends Error {
  constructor(message: string) {
    super(message);
    this.name = "AssertionError";
  }
}

export function expectEqual<T>(actual: T, expected: T, message?: string) {
  if (isDeepStrictEqual(actual, expected)) {
    return;
  }

  const defaultMessage = `Expected ${formatValue(actual)} to equal ${formatValue(
    expected
  )}`;
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

  throw new AssertionError("Expected function to throw but it completed successfully");
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
