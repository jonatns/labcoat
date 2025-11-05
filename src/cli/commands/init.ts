import { Command } from "commander";
import fs from "fs/promises";
import path from "path";
import os from "os";
import { fileURLToPath } from "url";
import AdmZip from "adm-zip"; // make sure this is installed

const __filename = fileURLToPath(import.meta.url);
const __dirname = path.dirname(__filename);

export const initCommand = new Command("init")
  .description("Initialize a new Labcoat project")
  .option("-t, --template <name>", "Template to use", "default")
  .action(async (options) => {
    console.log("🔥 Initializing Labcoat project...");

    const templateName = options.template;
    const repoOwner = "jonatns";
    const repoName = "labcoat-templates";
    const zipUrl = `https://codeload.github.com/${repoOwner}/${repoName}/zip/refs/heads/main`;

    // Temporary directory for extracting
    const tempDir = await fs.mkdtemp(path.join(os.tmpdir(), "labcoat-"));
    const zipPath = path.join(tempDir, "templates.zip");

    console.log(
      `📦 Downloading "${templateName}" template from ${repoName}...`
    );

    try {
      const res = await fetch(zipUrl);
      if (!res.ok) throw new Error(`HTTP ${res.status} - ${res.statusText}`);

      const buffer = Buffer.from(await res.arrayBuffer());
      await fs.writeFile(zipPath, buffer);
    } catch (err) {
      console.error("❌ Failed to download templates from GitHub:", err);
      process.exit(1);
    }

    // Extract ZIP
    const zip = new AdmZip(zipPath);
    zip.extractAllTo(tempDir, true);

    // Inside ZIP: labcoat-templates-main/<templateName>/
    const extractedRoot = path.join(tempDir, `${repoName}-main`);
    const templatePath = path.join(extractedRoot, templateName);

    try {
      await fs.access(templatePath);
    } catch {
      console.error(`❌ Template "${templateName}" not found in ${repoName}`);
      console.error(
        `   Available templates can be viewed at: https://github.com/${repoOwner}/${repoName}`
      );
      process.exit(1);
    }

    // Copy into current working directory
    await fs.cp(templatePath, process.cwd(), { recursive: true });

    console.log("✅ Project initialized successfully");
    console.log("\nNext steps:");
    console.log("  1. npm install");
    console.log("  2. npx labcoat compile contracts/Example.rs");
    console.log("  3. npx labcoat run scripts/example.ts");
  });
