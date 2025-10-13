import { labcoat } from "@jonatns/labcoat";

export default async function main() {
  const { deploy } = await labcoat.setup();
  await deploy("Example");
}

main().catch((err) => {
  console.error("❌ Deployment failed:", err);
  process.exit(1);
});
