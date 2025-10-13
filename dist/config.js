import path from "path";
import fs from "fs";
import { pathToFileURL } from "url";
export async function loadLabcoatConfig() {
    const root = process.cwd();
    const configPathTs = path.resolve(root, "labcoat.config.ts");
    const configPathJs = path.resolve(root, "labcoat.config.js");
    try {
        if (fs.existsSync(configPathTs)) {
            await import("ts-node/register");
            const module = await import(pathToFileURL(configPathTs).href);
            return module.default ?? module;
        }
        if (fs.existsSync(configPathJs)) {
            const module = await import(pathToFileURL(configPathJs).href);
            return module.default ?? module;
        }
        console.warn("⚠️  No labcoat.config.ts or labcoat.config.js found");
        return {};
    }
    catch (err) {
        console.error("❌ Failed to load Labcoat config:", err);
        return {};
    }
}
