import { Command } from "commander";
import fs from "fs/promises";
import { statSync } from "fs";
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
  sourcePathOrCode: string,
  tempDir: string,
  compiler: AlkanesCompiler
): Promise<BuildResult> {
  console.log(`🧱 Building ${contractName} in ${tempDir}`);

  await compiler.init();
  await compiler.scaffoldProject(tempDir, sourcePathOrCode);

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
  } catch {
    throw new Error(
      `Compiled WASM not found at ${wasmPath}. Did the build succeed?`
    );
  }

  return { wasmPath, contractName, tempDir };
}

export const testCommand = new Command("test")
  .argument("[target]", "Specific contract file or directory to test")
  .description("Compile the contract(s) and execute WASM-based contract tests")
  .action(async (target) => {
    const projectRoot = process.cwd();

    try {
      const contractsDir = path.join(projectRoot, "contracts");
      let contractsToTest: string[] = [];

      if (target) {
        const resolved = path.resolve(target);
        const stat = await fs.stat(resolved);
        if (stat.isDirectory()) {
          contractsToTest = [resolved];
        } else if (stat.isFile() && resolved.endsWith(".rs")) {
          contractsToTest = [resolved];
        } else {
          console.error("❌ Target must be a .rs file or a directory");
          process.exit(1);
        }
      } else {
        const entries = await fs.readdir(contractsDir, { withFileTypes: true });
        contractsToTest = entries
          .filter(
            (e) => (e.isFile() && e.name.endsWith(".rs")) || e.isDirectory()
          )
          .map((e) => path.join(contractsDir, e.name));
      }

      if (!contractsToTest.length) {
        console.error("❌ No contracts found in ./contracts");
        process.exit(1);
      }

      console.log(`🧪 Testing ${contractsToTest.length} contract(s)...`);

      const compiler = new AlkanesCompiler({
        baseDir: path.join(projectRoot, ".labcoat"),
        cleanup: false,
      });

      for (const contractPath of contractsToTest) {
        const stat = statSync(contractPath);
        const name = stat.isFile()
          ? path.basename(contractPath, ".rs")
          : path.basename(contractPath);

        const baseDir = path.join(projectRoot, ".labcoat");
        const tempDir = path.join(baseDir, `test_${name}`);
        await fs.mkdir(tempDir, { recursive: true });

        let result: BuildResult;
        let abiSource: string;

        if (stat.isFile()) {
          const sourceCode = await fs.readFile(contractPath, "utf8");
          result = await buildContract(name, sourceCode, tempDir, compiler);
          abiSource = sourceCode;
        } else if (stat.isDirectory()) {
          result = await buildContract(name, contractPath, tempDir, compiler);
          const libPath = path.join(contractPath, "lib.rs");
          try {
            abiSource = await fs.readFile(libPath, "utf8");
          } catch {
            throw new Error(`❌ No lib.rs found in ${contractPath}/src`);
          }
        } else {
          throw new Error(`Unsupported contract path: ${contractPath}`);
        }

        const abi = await compiler.parseABI(abiSource);

        console.log(`\n📋 Running tests for ${name}...`);
        const results = await runContractTests({
          projectRoot,
          wasmPath: result.wasmPath,
          abi,
        });

        if (results.failed > 0) {
          process.exitCode = 1;
        }
      }
    } catch (error) {
      console.error("❌", (error as Error).message);
      process.exitCode = 1;
    }
  });
