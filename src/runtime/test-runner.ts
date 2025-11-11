import fs from "fs/promises";
import path from "path";
import { pathToFileURL } from "url";
import { WASI } from "wasi";
import { expectEqual, expectRevert } from "./assertions.js";

const COLOR_GREEN = "\u001b[32m";
const COLOR_RED = "\u001b[31m";
const COLOR_CYAN = "\u001b[36m";
const COLOR_DIM = "\u001b[2m";
const COLOR_RESET = "\u001b[0m";

interface TestFileModule {
  [key: string]: unknown;
}

interface TestDefinition {
  name: string;
  fn: (context: TestContext) => unknown | Promise<unknown>;
}

type TestHook = (context: TestContext) => unknown | Promise<unknown>;

export interface TestContext {
  runtime: TestRuntime;
  expectEqual: typeof expectEqual;
  expectRevert: typeof expectRevert;
}

export interface RunContractTestsOptions {
  projectRoot: string;
  wasmPath: string;
}

export interface ContractTestSummary {
  passed: number;
  failed: number;
  total: number;
}

export class TestRuntime {
  private module?: WebAssembly.Module;
  private instance?: WebAssembly.Instance;
  private memory?: WebAssembly.Memory;
  private wasi?: WASI;
  private readonly wasmPath: string;

  public mockSender = "alk1testsender0000000000000000000000";
  public mockUtxos: Array<Record<string, unknown>> = [];

  constructor(wasmPath: string) {
    this.wasmPath = wasmPath;
  }

  private async ensureModule() {
    if (!this.module) {
      const wasmBytes = await fs.readFile(this.wasmPath);
      this.module = await WebAssembly.compile(wasmBytes);
    }
  }

  private createImports() {
    const wasi = new WASI({ args: [], env: {}, preopens: {} });
    this.wasi = wasi;

    return {
      ...wasi.getImportObject(),
      env: {
        println: (ptr: number, len: number) => this.handlePrintln(ptr, len),
      },
    };
  }

  private handlePrintln(ptr: number, len: number) {
    if (!this.memory) return;
    const bytes = new Uint8Array(this.memory.buffer, ptr, len);
    const text = new TextDecoder().decode(bytes);
    console.log(`${COLOR_DIM}📝 ${text}${COLOR_RESET}`);
  }

  public async instantiate() {
    await this.ensureModule();
    const imports = this.createImports();
    const instance = await WebAssembly.instantiate(this.module!, imports);
    this.instance = instance;
    const memoryExport = instance.exports.memory;
    if (memoryExport instanceof WebAssembly.Memory) {
      this.memory = memoryExport;
    }

    if (this.wasi) {
      this.wasi.initialize(instance);
    }

    return instance;
  }

  public async reset() {
    this.instance = undefined;
    this.memory = undefined;
    this.wasi = undefined;
  }

  private async ensureInstance() {
    if (!this.instance) {
      await this.instantiate();
    }
    return this.instance!;
  }

  public async call(method: string, ...args: (number | bigint)[]) {
    const instance = await this.ensureInstance();
    const exportFn = instance.exports[method];
    if (typeof exportFn !== "function") {
      throw new Error(`Exported function \"${method}\" not found on contract`);
    }
    const wasmFn = exportFn as (...fnArgs: (number | bigint)[]) => unknown;
    return wasmFn(...args);
  }

  public getExports() {
    if (!this.instance) {
      throw new Error("Runtime not instantiated. Call instantiate() first.");
    }
    return this.instance.exports;
  }

  public getMemory() {
    if (!this.memory) {
      throw new Error("Contract memory is not available");
    }
    return this.memory;
  }
}

async function discoverTestFiles(projectRoot: string) {
  const testDir = path.join(projectRoot, "tests");
  try {
    const stats = await fs.stat(testDir);
    if (!stats.isDirectory()) {
      return [];
    }
  } catch (error) {
    return [];
  }

  const entries = await fs.readdir(testDir, { withFileTypes: true });
  return entries
    .filter((entry) => entry.isFile() && entry.name.endsWith(".spec.js"))
    .map((entry) => path.join(testDir, entry.name))
    .sort();
}

function extractTests(module: TestFileModule): TestDefinition[] {
  const tests: TestDefinition[] = [];

  const defaultExport = (module as { default?: unknown }).default;
  if (Array.isArray(defaultExport)) {
    for (const value of defaultExport) {
      if (value && typeof value.name === "string" && typeof value.fn === "function") {
        tests.push({ name: value.name, fn: value.fn });
      }
    }
  } else if (typeof defaultExport === "function") {
    tests.push({ name: defaultExport.name || "default", fn: defaultExport });
  }

  const seen = new Set(tests.map((test) => test.name));

  for (const [name, exported] of Object.entries(module)) {
    if (
      name === "default" ||
      name === "beforeAll" ||
      name === "afterAll" ||
      name === "beforeEach" ||
      name === "afterEach"
    ) {
      continue;
    }

    if (typeof exported === "function" && !seen.has(name)) {
      tests.push({ name, fn: exported });
      seen.add(name);
    }
  }

  return tests;
}

export async function runContractTests(
  options: RunContractTestsOptions
): Promise<ContractTestSummary> {
  const { projectRoot, wasmPath } = options;
  const runtime = new TestRuntime(wasmPath);

  const files = await discoverTestFiles(projectRoot);
  if (files.length === 0) {
    console.log("ℹ️  No test files found in ./tests. Skipping.");
    return { passed: 0, failed: 0, total: 0 };
  }

  let passed = 0;
  let failed = 0;

  for (const file of files) {
    const relative = path.relative(projectRoot, file);
    console.log(`\n${COLOR_CYAN}📄 ${relative}${COLOR_RESET}`);

    let module: TestFileModule;
    try {
      module = await import(pathToFileURL(file).href);
    } catch (error) {
      failed += 1;
      console.error(`${COLOR_RED}  ❌ Failed to import test file: ${(error as Error).message}${COLOR_RESET}`);
      continue;
    }

    const tests = extractTests(module);
    const hooks = module as {
      beforeAll?: TestHook;
      afterAll?: TestHook;
      beforeEach?: TestHook;
      afterEach?: TestHook;
    };
    const beforeAll = typeof hooks.beforeAll === "function" ? hooks.beforeAll : undefined;
    const afterAll = typeof hooks.afterAll === "function" ? hooks.afterAll : undefined;
    const beforeEach = typeof hooks.beforeEach === "function" ? hooks.beforeEach : undefined;
    const afterEach = typeof hooks.afterEach === "function" ? hooks.afterEach : undefined;

    if (typeof beforeAll === "function") {
      await beforeAll({ runtime, expectEqual, expectRevert });
    }

    if (tests.length === 0) {
      console.log(`${COLOR_DIM}  ⚠️  No tests found in ${relative}${COLOR_RESET}`);
      continue;
    }

    for (const test of tests) {
      if (typeof beforeEach === "function") {
        await beforeEach({ runtime, expectEqual, expectRevert });
      }

      await runtime.reset();
      await runtime.instantiate();

      const start = Date.now();
      try {
        await test.fn({ runtime, expectEqual, expectRevert });
        const duration = Date.now() - start;
        passed += 1;
        console.log(
          `${COLOR_GREEN}  ✅ ${test.name}${COLOR_RESET}${COLOR_DIM} (${duration}ms)${COLOR_RESET}`
        );
      } catch (error) {
        failed += 1;
        const duration = Date.now() - start;
        console.error(
          `${COLOR_RED}  ❌ ${test.name}${COLOR_RESET}${COLOR_DIM} (${duration}ms)${COLOR_RESET}`
        );
        console.error(`${COLOR_RED}     ${(error as Error).message}${COLOR_RESET}`);
      } finally {
        if (typeof afterEach === "function") {
          await afterEach({ runtime, expectEqual, expectRevert });
        }
      }
    }

    if (typeof afterAll === "function") {
      await afterAll({ runtime, expectEqual, expectRevert });
    }
  }

  const total = passed + failed;
  const summaryColor = failed > 0 ? COLOR_RED : COLOR_GREEN;
  console.log(
    `\n${summaryColor}${passed}/${total} tests passed${COLOR_RESET} (${failed} failed)`
  );

  return { passed, failed, total };
}
