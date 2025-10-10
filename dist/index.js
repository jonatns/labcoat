"use strict";
var __createBinding = (this && this.__createBinding) || (Object.create ? (function(o, m, k, k2) {
    if (k2 === undefined) k2 = k;
    var desc = Object.getOwnPropertyDescriptor(m, k);
    if (!desc || ("get" in desc ? !m.__esModule : desc.writable || desc.configurable)) {
      desc = { enumerable: true, get: function() { return m[k]; } };
    }
    Object.defineProperty(o, k2, desc);
}) : (function(o, m, k, k2) {
    if (k2 === undefined) k2 = k;
    o[k2] = m[k];
}));
var __exportStar = (this && this.__exportStar) || function(m, exports) {
    for (var p in m) if (p !== "default" && !Object.prototype.hasOwnProperty.call(exports, p)) __createBinding(exports, m, p);
};
Object.defineProperty(exports, "__esModule", { value: true });
exports.loadAlkaliConfig = exports.AlkanesCompiler = exports.AlkanesContract = void 0;
var contract_1 = require("./contract");
Object.defineProperty(exports, "AlkanesContract", { enumerable: true, get: function () { return contract_1.AlkanesContract; } });
var compiler_1 = require("./compiler");
Object.defineProperty(exports, "AlkanesCompiler", { enumerable: true, get: function () { return compiler_1.AlkanesCompiler; } });
var config_1 = require("./config");
Object.defineProperty(exports, "loadAlkaliConfig", { enumerable: true, get: function () { return config_1.loadAlkaliConfig; } });
__exportStar(require("./types"), exports);
//# sourceMappingURL=index.js.map