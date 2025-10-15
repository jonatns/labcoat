import { labcoat } from "@jonatns/labcoat";

export default async function main() {
  const { simulate } = await labcoat.setup();
  await simulate("Example", "Greet", ["World"]);
}

main().catch((err) => {
  console.error("❌ Deployment failed:", err);
  process.exit(1);
});
