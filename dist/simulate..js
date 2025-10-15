import fs from "fs/promises";
import { decodeAlkanesResult, encodeArgs } from "./helpers.js";
import { loadManifest } from "./manifest.js";
export async function simulateContract(provider, contractName, methodName, args = []) {
    console.log(`🧪 Simulating ${contractName}.${methodName} with args:`, args);
    const manifest = await loadManifest();
    const contractInfo = manifest[contractName];
    if (!contractInfo)
        throw new Error(`Contract ${contractName} not found`);
    const abi = JSON.parse(await fs.readFile(contractInfo.abi, "utf8"));
    const method = abi.methods.find((m) => m.name.toLowerCase() === methodName.toLowerCase());
    if (!method)
        throw new Error(`Method ${methodName} not found in ABI`);
    const [block, tx] = contractInfo.deployment.alkanesId
        .split(":")
        .map((p) => p.trim());
    const encodedArgs = encodeArgs(args);
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
    return decodeAlkanesResult(result);
}
