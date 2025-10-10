import path from "path";

export function loadAlkaliConfig(configPath = "./alkali.config.ts") {
  try {
    const config = require(path.resolve(configPath));
    return config;
  } catch (err) {
    console.warn("No alkali.config.ts found or failed to load.");
    return {};
  }
}
