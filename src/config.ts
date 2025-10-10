import path from "path";
import fs from "fs";
import { pathToFileURL } from "url";
import { AlkaliConfig } from "./types";

export function loadAlkaliConfig(): Promise<AlkaliConfig> {
  const configPathTs = path.resolve("alkali.config.ts");
  const configPathJs = path.resolve("alkali.config.js");

  if (fs.existsSync(configPathTs)) {
    return import(pathToFileURL(configPathTs).href);
  } else if (fs.existsSync(configPathJs)) {
    return import(pathToFileURL(configPathJs).href);
  } else {
    return Promise.resolve({} as AlkaliConfig);
  }
}
