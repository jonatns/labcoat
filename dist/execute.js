import { encodeArgs, waitForTrace, decodeRevertReason } from "./helpers.js";
import { encodeRunestoneProtostone, ProtoStone, encipher } from "alkanes";
import { alkanes } from "oyl-sdk";
import { loadManifest } from "./manifest.js";
import { readFile } from "fs/promises";
export async function executeContract(contractName, methodName, args, account, signer, provider, utxos) {
    console.log(`🚀 Executing ${contractName}.${methodName} with args:`, args);
    const manifest = await loadManifest();
    const contractInfo = manifest[contractName];
    if (!contractInfo)
        throw new Error(`Contract ${contractName} not found`);
    const abi = JSON.parse(await readFile(contractInfo.abi, "utf8"));
    const method = abi.methods.find((m) => m.name.toLowerCase() === methodName.toLowerCase());
    if (!method)
        throw new Error(`Method ${methodName} not found in ABI`);
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
        utxos,
        alkanesUtxos: [],
        feeRate: 2,
        account,
        signer,
        provider,
    });
    console.log(`🔗 Tx ID: ${executionResult.executeResult.txId}`);
    const returnTrace = await waitForTrace(provider, executionResult.executeResult.txId, "return");
    const status = returnTrace?.data?.status ?? "unknown";
    if (status === "revert") {
        console.warn("⚠️ Revert reason:", decodeRevertReason(returnTrace?.data?.response?.data ?? "0x"));
    }
    else {
        console.log(`✅ Execution status: ${status.toUpperCase()}`);
    }
    return executionResult;
}
