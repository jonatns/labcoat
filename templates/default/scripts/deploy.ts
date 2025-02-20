import { AlkanesContract } from "alkali";

async function main() {
  const contract = await AlkanesContract.fromFile("contracts/Example.rs");
  const deployed = await contract.deploy();
  console.log(`Contract deployed to: ${deployed.address}`);
}

main().catch(console.error);
