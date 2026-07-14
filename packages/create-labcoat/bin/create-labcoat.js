#!/usr/bin/env node
// create-labcoat — scaffold a new Alkanes contract project.
// Usage: npm create labcoat [dir] [--template default]
import { cpSync, existsSync, readdirSync } from "fs";
import path from "path";
import { fileURLToPath } from "url";

const here = path.dirname(fileURLToPath(import.meta.url));
const templatesDir = path.join(here, "..", "templates");

const args = process.argv.slice(2);
const targetArg = args.find((a) => !a.startsWith("-")) ?? "my-labcoat-project";
const tplIdx = args.indexOf("--template");
const template = tplIdx >= 0 ? args[tplIdx + 1] : "default";

const source = path.join(templatesDir, template);
if (!existsSync(source)) {
  const available = readdirSync(templatesDir).join(", ");
  console.error(`Unknown template "${template}". Available: ${available}`);
  process.exit(1);
}

const target = path.resolve(targetArg);
if (existsSync(target) && readdirSync(target).length > 0) {
  console.error(`Refusing to scaffold into non-empty directory: ${target}`);
  process.exit(1);
}

cpSync(source, target, { recursive: true });
console.log(`✅ Created ${target} from the "${template}" template.

Next steps:
  cd ${path.relative(process.cwd(), target) || "."}
  labcoat up            # boot the local devnet
  labcoat wallet init
  npm run compile
  labcoat deploy build/Example.wasm

Agent-ready: AGENTS.md + SKILL.md are included; \`labcoat docs --llm\`
prints the full reference and \`labcoat mcp serve\` exposes MCP tools.`);
