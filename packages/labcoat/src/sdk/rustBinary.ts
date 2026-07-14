import { execFile } from "child_process";
import { existsSync } from "fs";
import path from "path";

/**
 * Locates and invokes the `labcoat` Rust CLI, which carries all wallet,
 * deploy, execute, simulate, and trace logic (built on the pinned
 * alkanes-rs develop commit — no oyl-sdk anywhere).
 *
 * Discovery order:
 *  1. LABCOAT_CORE_BIN env override
 *  2. target/{release,debug}/labcoat walking up from cwd (monorepo dev)
 *  3. `labcoat` on PATH
 */
export function findLabcoatBinary(): string {
  const override = process.env.LABCOAT_CORE_BIN;
  if (override && existsSync(override)) return override;

  let dir = process.cwd();
  for (let i = 0; i < 6; i++) {
    for (const profile of ["release", "debug"]) {
      const candidate = path.join(dir, "target", profile, "labcoat");
      if (existsSync(candidate)) return candidate;
    }
    const parent = path.dirname(dir);
    if (parent === dir) break;
    dir = parent;
  }

  return "labcoat"; // rely on PATH
}

export interface Envelope<T = any> {
  ok: boolean;
  command: string;
  schema: string;
  result?: T;
  error?: { code: string; message: string; hint?: string };
}

export class LabcoatCliError extends Error {
  code: string;
  hint?: string;
  constructor(code: string, message: string, hint?: string) {
    super(`[${code}] ${message}${hint ? `\nhint: ${hint}` : ""}`);
    this.code = code;
    this.hint = hint;
  }
}

export interface InvokeOptions {
  network: string;
  rpcUrl: string;
  walletFile?: string;
  feeRate?: number;
  /** written to the child's stdin (mnemonics only — never argv) */
  stdin?: string;
  /** extra environment (e.g. LABCOAT_WALLET_PASSPHRASE) */
  env?: Record<string, string>;
}

/**
 * Run one CLI command and return the parsed envelope result.
 * Secrets policy: passphrase via env, mnemonic via stdin, nothing on argv.
 */
export async function invokeLabcoat<T = any>(
  args: string[],
  options: InvokeOptions
): Promise<T> {
  const bin = findLabcoatBinary();
  const fullArgs = [
    ...args,
    "--json",
    "--network",
    options.network,
    "--rpc-url",
    options.rpcUrl,
  ];
  if (options.walletFile) fullArgs.push("--wallet-file", options.walletFile);
  if (options.feeRate != null) fullArgs.push("--fee-rate", String(options.feeRate));

  const stdout = await new Promise<string>((resolve, reject) => {
    const child = execFile(
      bin,
      fullArgs,
      {
        env: { ...process.env, ...options.env },
        maxBuffer: 64 * 1024 * 1024,
      },
      (err, stdout, stderr) => {
        if (err && !stdout) {
          reject(
            new LabcoatCliError(
              "BINARY_CRASH",
              `labcoat CLI failed: ${err.message}\n${stderr}`,
              "set LABCOAT_CORE_BIN to a built labcoat binary (cargo build -p labcoat-cli)"
            )
          );
          return;
        }
        resolve(stdout);
      }
    );
    if (options.stdin != null) {
      child.stdin?.write(options.stdin);
    }
    child.stdin?.end();
  });

  let envelope: Envelope<T>;
  try {
    envelope = JSON.parse(stdout);
  } catch {
    throw new LabcoatCliError(
      "BINARY_CRASH",
      `labcoat CLI emitted unparseable output: ${stdout.slice(0, 400)}`,
      "re-run with RUST_LOG=debug to see stderr diagnostics"
    );
  }

  if (!envelope.ok || envelope.error) {
    const e = envelope.error ?? { code: "TOOLKIT_ERROR", message: "unknown error" };
    throw new LabcoatCliError(e.code, e.message, e.hint);
  }
  return envelope.result as T;
}
