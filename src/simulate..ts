import fs from "fs/promises";
import { decodeAlkanesResult, encodeArgs } from "./helpers.js";
import { loadManifest } from "./manifest.js";
import ora from "ora";

export async function simulateContract(
  provider: any,
  contractName: string,
  methodName: string,
  args: any[] = []
) {
  console.log(`🧪 Simulating ${contractName}.${methodName} with args:`, args);

  const spinner = ora("Preparing simulation...").start();

  const manifest = await loadManifest();
  const contractInfo = manifest[contractName];
  if (!contractInfo) throw new Error(`Contract ${contractName} not found`);

  const abi = JSON.parse(await fs.readFile(contractInfo.abi, "utf8"));
  const method = abi.methods.find(
    (m: any) => m.name.toLowerCase() === methodName.toLowerCase()
  );
  if (!method) throw new Error(`Method ${methodName} not found in ABI`);

  const [block, tx] = contractInfo.deployment.alkanesId
    .split(":")
    .map((p: string) => p.trim());
  const encodedArgs = encodeArgs(args);

  spinner.text = "⏳ Running simulation...";
  const request = {
    alkanes: [],
    transaction: "0x",
    block: "0x",
    height: "20000",
    txindex: 0,
    target: { block, tx },
    inputs: [method.opcode.toString(), ...encodedArgs],
    pointer: 0,
    refundPointer: 0,
    vout: 0,
  };

  const result = await provider.alkanes.simulate(request);
  spinner.stop();

  console.log("- ✅ Simulation complete");

  return decodeAlkanesResult(result);
}
