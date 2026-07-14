import ora from "ora";
import { invokeLabcoat } from "./rustBinary.js";
import { encodeArgs } from "./helpers.js";
import { resolveContract } from "./manifest.js";
import type { LabcoatWallet } from "./types.js";

export async function simulateContract(
  contractName: string,
  methodName: string,
  args: any[] = [],
  wallet: LabcoatWallet
) {
  console.log(`🧪 Simulating ${contractName}.${methodName} with args:`, args);

  const spinner = ora("⏳ Running simulation...").start();

  try {
    const { alkanesId, method } = await resolveContract(contractName, methodName);

    const result = await invokeLabcoat<{
      status: string;
      gasUsed: number;
      error?: string;
      data: string;
      decoded: { string?: string; uint?: string };
    }>(
      ["simulate", alkanesId, String(method.opcode), ...encodeArgs(args)],
      wallet.cli
    );

    spinner.stop();
    console.log("- ✅ Simulation complete");

    // Same decoding priority as the old decodeAlkanesResult: printable
    // string first, then integer (number when safe, else bigint), else raw.
    if (result.decoded.string) return result.decoded.string;
    if (result.decoded.uint != null) {
      const n = BigInt(result.decoded.uint);
      return n <= BigInt(Number.MAX_SAFE_INTEGER) ? Number(n) : n;
    }
    return result.data ?? null;
  } catch (err) {
    spinner.stop();
    throw err;
  }
}
