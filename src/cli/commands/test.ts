import { Command } from "commander";
import fs from "fs/promises";
import path from "path";
import { spawn } from "child_process";
import { runContractTests } from "@/runtime/test-runner.js";

interface BuildResult {
  wasmPath: string;
  contractName: string;
}

async function readContractName(projectDir: string) {
  const cargoTomlPath = path.join(projectDir, "Cargo.toml");
  const manifest = await fs.readFile(cargoTomlPath, "utf8");
  const match = manifest.match(/name\s*=\s*"([^"]+)"/);
  if (!match) {
    throw new Error("Unable to determine contract name from Cargo.toml");
  }
  return match[1];
}

async function buildContract(projectDir: string): Promise<BuildResult> {
  const cargoTomlPath = path.join(projectDir, "Cargo.toml");
  try {
    await fs.access(cargoTomlPath);
  } catch (error) {
    throw new Error(
      "No Cargo.toml found. Make sure you're running labcoat test from the project root."
    );
  }

  console.log("🦾 Building contract (wasm32-wasi)...");

  await new Promise<void>((resolve, reject) => {
    const child = spawn(
      "cargo",
      ["build", "--target", "wasm32-wasi", "--release"],
      { stdio: "inherit", cwd: projectDir, shell: process.platform === "win32" }
    );

    child.on("error", (err) => {
      reject(
        new Error(
          `Failed to run cargo build. Please ensure Rust is available in your PATH.\n${err.message}`
        )
      );
    });

    child.on("exit", (code) => {
      if (code === 0) {
        resolve();
      } else {
        reject(new Error(`cargo build exited with code ${code}`));
      }
    });
  });

  const contractName = await readContractName(projectDir);
  const wasmFileName = `${contractName.replace(/-/g, "_")}.wasm`;
  const wasmPath = path.join(
    projectDir,
    "target",
    "wasm32-wasi",
    "release",
    wasmFileName
  );

  try {
    await fs.access(wasmPath);
  } catch (error) {
    throw new Error(
      `Compiled WASM not found at ${wasmPath}. Did the build succeed?`
    );
  }

  return { wasmPath, contractName };
}

export const testCommand = new Command("test")
  .description("Compile the contract and execute WASM-based contract tests")
  .action(async () => {
    const projectDir = process.cwd();

    try {
      const { wasmPath } = await buildContract(projectDir);
      const results = await runContractTests({
        projectRoot: projectDir,
        wasmPath,
      });

      if (results.failed > 0) {
        process.exitCode = 1;
      }
    } catch (error) {
      console.error("❌", (error as Error).message);
      process.exitCode = 1;
    }
  });
