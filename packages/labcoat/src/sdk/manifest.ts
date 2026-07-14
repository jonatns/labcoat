import fs from "fs/promises";
import { readFileSync, existsSync } from "fs";

const MANIFEST_PATH = "./deployments/manifest.json";
const LOCKFILE_PATH = "./labcoat.lock";

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

/**
 * Look up a deployed contract + ABI method. Prefers labcoat.lock (written
 * by the Rust core, per-network); falls back to the legacy manifest.
 */
export async function resolveContract(
  contractName: string,
  methodName: string
): Promise<{ alkanesId: string; method: { opcode: number; name: string } }> {
  let alkanesId: string | undefined;

  if (existsSync(LOCKFILE_PATH)) {
    try {
      const lock = JSON.parse(readFileSync(LOCKFILE_PATH, "utf8"));
      for (const network of Object.values<any>(lock.networks ?? {})) {
        if (network[contractName]?.alkanesId) {
          alkanesId = network[contractName].alkanesId;
          break;
        }
      }
    } catch {
      // fall through to the legacy manifest
    }
  }

  const manifest = await loadManifest();
  const contractInfo = manifest[contractName];
  if (!alkanesId) {
    alkanesId = contractInfo?.deployment?.alkanesId;
  }
  if (!alkanesId) {
    throw new Error(
      `Contract ${contractName} not found in labcoat.lock or deployments/manifest.json — deploy it first`
    );
  }

  // ABI lives next to the build artifacts.
  const abiPath = contractInfo?.abi ?? `./build/${contractName}.abi.json`;
  const abi = JSON.parse(await fs.readFile(abiPath, "utf8"));
  const method = abi.methods.find(
    (m: any) => m.name.toLowerCase() === methodName.toLowerCase()
  );
  if (!method) throw new Error(`Method ${methodName} not found in ABI`);

  return { alkanesId, method };
}
