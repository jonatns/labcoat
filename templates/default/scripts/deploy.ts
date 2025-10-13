import { Oyl } from "oyl-sdk";
import fs from "fs/promises";
import config from "../labcoat.config";

export default async function main() {
  console.log("🚀 Deploying Example contract...");

  const bytecode = await fs.readFile("./build/Example.wasm");
  const abi = JSON.parse(await fs.readFile("./build/Example.abi.json", "utf8"));

  const oyl = new Oyl({ network: config.network });
  const privateKey = config.accounts[0];
  const signer = oyl.Wallet.fromPrivateKey(privateKey);

  const contract = oyl.Contract.fromCompiled({
    bytecode: bytecode.toString("base64"),
    abi,
  });

  const tx = await contract.deploy({
    from: signer.address,
    signer,
  });

  console.log("✅ Contract deployed!");
  console.log(`📜 Address: ${tx.contractAddress}`);
  console.log(`🔗 TxID: ${tx.txid}`);
}

main().catch((err) => {
  console.error("❌ Deployment failed:", err);
  process.exit(1);
});
