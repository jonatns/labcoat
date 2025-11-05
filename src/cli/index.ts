import { Command } from "commander";
import { registerCommands } from "./commands/index.js";

const program = new Command();

program
  .name("labcoat")
  .description("Smart contract development toolkit for Bitcoin Alkanes")
  .version("0.1.0");

await registerCommands(program);

program.parse();
