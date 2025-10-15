import { labcoat } from "@jonatns/labcoat";

export default async function main() {
  const { deploy, simulate } = await labcoat.setup();

  await deploy("Example");

  const result = await simulate("Example", "Greet", ["World"]);
  console.log("📦 Result:", result);
}

main()
  .then(() => process.exit(0))
  .catch((err) => {
    console.error("❌", err);
    process.exit(1);
  });
