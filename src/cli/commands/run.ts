import { Command } from "commander";
import path from "path";
import { spawn } from "child_process";
import { runTypeScriptFile } from "../../utils/ts-runner.js";

export const runCommand = new Command("run")
  .argument("<script>", "Path to a .ts or .js script to execute")
  .description("Run a custom Labcoat script (.ts or .js)")
  .action(async (script) => {
    const scriptPath = path.resolve(script);
    const isTs = scriptPath.endsWith(".ts");

    console.log(`🧩 Running script: ${scriptPath}`);

    if (isTs) {
      await runTypeScriptFile(scriptPath);
    } else {
      const child = spawn("node", [scriptPath], {
        stdio: "inherit",
        shell: true,
      });
      child.on("exit", (code) => process.exit(code ?? 0));
    }
  });
