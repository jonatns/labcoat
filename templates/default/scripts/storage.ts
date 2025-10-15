import { labcoat } from "@jonatns/labcoat";

export default async function main() {
  const { deploy, execute, simulate } = await labcoat.setup();

  await deploy("Storage");

  await execute("Storage", "Store", [777]);

  const result = await simulate("Storage", "Retrieve");
  console.log("- 📦 Result:", result);
}

main()
  .then(() => process.exit(0))
  .catch((err) => {
    console.error("❌", err);
    process.exit(1);
  });
