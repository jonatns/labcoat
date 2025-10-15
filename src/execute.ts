import { encodeArgs, waitForTrace, decodeRevertReason } from "./helpers.js";
import { encodeRunestoneProtostone, ProtoStone, encipher } from "alkanes";
import { Account, alkanes, FormattedUtxo, Provider, Signer } from "oyl-sdk";
import { loadManifest } from "./manifest.js";
import { readFile } from "fs/promises";
import ora from "ora";

export async function executeContract(
  contractName: string,
  methodName: string,
  args: any[],
  options: { feeRate?: number },
  wallet: {
    account: Account;
    signer: Signer;
    provider: Provider;
    utxos: FormattedUtxo[];
  }
) {
  console.log(`🚀 Executing ${contractName}.${methodName} with args:`, args);

  const spinner = ora("Preparing execute...").start();

  const manifest = await loadManifest();
  const contractInfo = manifest[contractName];
  if (!contractInfo) throw new Error(`Contract ${contractName} not found`);

  const abi = JSON.parse(await readFile(contractInfo.abi, "utf8"));
  const method = abi.methods.find(
    (m) => m.name.toLowerCase() === methodName.toLowerCase()
  );
  if (!method) throw new Error(`Method ${methodName} not found in ABI`);

  const [block, tx] = contractInfo.deployment.alkanesId
    .split(":")
    .map((p) => p.trim());
  const encodedArgs = encodeArgs(args);

  const protostone = encodeRunestoneProtostone({
    protostones: [
      ProtoStone.message({
        protocolTag: 1n,
        edicts: [],
        pointer: 0,
        refundPointer: 0,
        calldata: encipher([
          BigInt(block),
          BigInt(tx),
          BigInt(method.opcode),
          ...encodedArgs.map((a) => BigInt(a)),
        ]),
      }),
    ],
  }).encodedRunestone;

  const executionResult = await alkanes.execute({
    protostone,
    alkanesUtxos: [],
    feeRate: options.feeRate,
    ...wallet,
  });

  spinner.stop();

  console.log(`- 🔗 Tx ID: ${executionResult.executeResult.txId}`);

  spinner.start("Waiting for Alkanes traces...");
  const returnTrace = await waitForTrace(
    wallet.provider,
    executionResult.executeResult.txId,
    "return"
  );

  spinner.stop();

  const status = returnTrace?.data?.status ?? "unknown";

  if (status === "revert") {
    console.log(`- 📊 Execute status: ${status}`);
    console.log(
      `- 🪵 Reason: ${decodeRevertReason(
        returnTrace?.data?.response?.data ?? "0x"
      )}`
    );
  } else {
    console.log(`- 📊 Execute status: ${status}`);
  }

  return executionResult;
}
