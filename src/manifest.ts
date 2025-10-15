import fs from "fs/promises";
const MANIFEST_PATH = "./deployments/manifest.json";

export async function loadManifest(): Promise<Record<string, any>> {
  try {
    const data = await fs.readFile(MANIFEST_PATH, "utf8");
    return JSON.parse(data);
  } catch {
    return {};
  }
}

export async function saveManifest(manifest: Record<string, any>) {
  await fs.mkdir("./deployments", { recursive: true });
  await fs.writeFile(MANIFEST_PATH, JSON.stringify(manifest, null, 2));
}
