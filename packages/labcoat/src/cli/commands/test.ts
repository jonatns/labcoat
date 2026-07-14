import { Command } from "commander";
import fs from "fs/promises";
import path from "path";
import { exec } from "child_process";
import { promisify } from "util";
import { AlkanesCompiler } from "@/sdk/compiler.js";
import { runContractTests } from "@/runtime/test-runner.js";

const execAsync = promisify(exec);

interface BuildResult {
  wasmPath: string;
  contractName: string;
  tempDir: string;
}

async function buildContract(
  contractName: string,
  sourceCode: string,
  tempDir: string,
  compiler: AlkanesCompiler
): Promise<BuildResult> {
  console.log(`🧱 Building ${contractName} in ${tempDir}`);

  // Use AlkanesCompiler to scaffold the Rust project
  await compiler.scaffoldProject(tempDir, sourceCode);

  // Build with wasm32-wasip1 target for testing
  console.log("🦾 Building contract (wasm32-wasip1)...");

  const { stdout, stderr } = await execAsync(
    `cargo clean && cargo build --target=wasm32-wasip1 --release`,
    { cwd: tempDir }
  );

  if (stderr?.trim()) console.warn("⚠️ Build warnings:", stderr);
  if (stdout?.trim()) console.log(stdout);

  const wasmPath = path.join(
    tempDir,
    "target",
    "wasm32-wasip1",
    "release",
    "alkanes_contract.wasm"
  );

  try {
    await fs.access(wasmPath);
  } catch (error) {
    throw new Error(
      `Compiled WASM not found at ${wasmPath}. Did the build succeed?`
    );
  }

  return { wasmPath, contractName, tempDir };
}

export const testCommand = new Command("test")
  .argument("[file]", "Specific contract file to test")
  .description("Compile the contract and execute WASM-based contract tests")
  .action(async (file) => {
    const projectRoot = process.cwd();

    try {
      let filesToTest: string[] = [];

      if (file) {
        filesToTest = [path.resolve(file)];
      } else {
        const contractsDir = path.join(projectRoot, "contracts");
        try {
          const entries = await fs.readdir(contractsDir, {
            withFileTypes: true,
          });
          filesToTest = entries
            .filter((e) => e.isFile() && e.name.endsWith(".rs"))
            .map((e) => path.join(contractsDir, e.name));
        } catch (error) {
          console.error("❌ No ./contracts directory found");
          process.exitCode = 1;
          return;
        }
      }

      if (!filesToTest.length) {
        console.error("❌ No .rs files found in ./contracts");
        process.exitCode = 1;
        return;
      }

      console.log(`🧪 Testing ${filesToTest.length} contract(s)...`);

      const compiler = new AlkanesCompiler({
        baseDir: path.join(projectRoot, ".labcoat"),
        cleanup: false,
      });

      for (const filePath of filesToTest) {
        const contractName = path.basename(filePath, ".rs");
        const sourceCode = await fs.readFile(filePath, "utf8");

        const baseDir = path.join(projectRoot, ".labcoat");
        const tempDir = path.join(baseDir, `test_${contractName}`);
        await fs.mkdir(tempDir, { recursive: true });

        const { wasmPath } = await buildContract(
          contractName,
          sourceCode,
          tempDir,
          compiler
        );

        // Parse ABI from source code to enable method name lookups
        const abi = await compiler.parseABI(sourceCode);

        console.log(`\n📋 Running tests for ${contractName}...`);
        const results = await runContractTests({
          projectRoot,
          wasmPath,
          abi,
        });

        if (results.failed > 0) {
          process.exitCode = 1;
        }
      }
    } catch (error) {
      console.error(error);
      console.error("❌", (error as Error).message);
      process.exitCode = 1;
    }
  });
