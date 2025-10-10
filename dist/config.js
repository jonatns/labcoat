"use strict";
var __importDefault = (this && this.__importDefault) || function (mod) {
    return (mod && mod.__esModule) ? mod : { "default": mod };
};
Object.defineProperty(exports, "__esModule", { value: true });
exports.loadAlkaliConfig = loadAlkaliConfig;
const path_1 = __importDefault(require("path"));
function loadAlkaliConfig(configPath = "./alkali.config.ts") {
    try {
        const config = require(path_1.default.resolve(configPath));
        return config;
    }
    catch (err) {
        console.warn("No alkali.config.ts found or failed to load.");
        return {};
    }
}
//# sourceMappingURL=config.js.map