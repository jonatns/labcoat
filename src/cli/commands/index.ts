import type { Command } from "commander";
import { initCommand } from "./init.js";
import { compileCommand } from "./compile.js";
import { runCommand } from "./run.js";
import { testCommand } from "./test.js";

export async function registerCommands(program: Command) {
  program.addCommand(initCommand);
  program.addCommand(compileCommand);
  program.addCommand(runCommand);
  program.addCommand(testCommand);
}
