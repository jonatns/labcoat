import ora from "ora";
import { invokeLabcoat } from "./rustBinary.js";
import { encodeArgs } from "./helpers.js";
import { resolveContract } from "./manifest.js";
import type { LabcoatWallet } from "./types.js";

export async function executeContract(
  contractName: string,
  methodName: string,
  args: any[],
  options: { feeRate?: number },
  wallet: LabcoatWallet
) {
  console.log(`🚀 Executing ${contractName}.${methodName} with args:`, args);

  const spinner = ora("Preparing execute...").start();

  try {
    const { alkanesId, method } = await resolveContract(contractName, methodName);

    const result = await invokeLabcoat<{
      txid: string;
      status: string;
      revertReason?: string;
      traces?: unknown[];
    }>(
      [
        "call",
        alkanesId,
        String(method.opcode),
        ...encodeArgs(args),
      ],
      { ...wallet.cli, feeRate: options.feeRate }
    );

    spinner.stop();
    console.log(`- 🔗 Tx ID: ${result.txid}`);
    console.log(`- 📊 Execute status: ${result.status}`);
    if (result.status === "revert" && result.revertReason) {
      console.log(`- 🪵 Reason: ${result.revertReason}`);
    }

    // Keep the oyl-era `executeResult.txId` shape for existing scripts.
    return {
      executeResult: { txId: result.txid },
      status: result.status,
      revertReason: result.revertReason,
      traces: result.traces,
    };
  } catch (err) {
    spinner.stop();
    throw err;
  }
}
