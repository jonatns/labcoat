"use strict";
Object.defineProperty(exports, "__esModule", { value: true });
exports.AlkanesContract = void 0;
class AlkanesContract {
    constructor(config) {
        this.config = config;
    }
    async deploy(params = []) {
        // Implementation for deploying contract
        // 1. Get deploy TX data
        // 2. Sign transaction
        // 3. Broadcast to network
        // 4. Return contract address
        return "contract_address";
    }
    async call(methodName, params = []) {
        const method = this.findMethod(methodName);
        if (!method) {
            throw new Error(`Method ${methodName} not found in ABI`);
        }
        // Implementation for calling contract method
        // 1. Encode params according to ABI
        // 2. Create transaction
        // 3. Return result
    }
    findMethod(name) {
        return this.config.abi.methods.find((m) => m.name === name);
    }
}
exports.AlkanesContract = AlkanesContract;
//# sourceMappingURL=contract.js.map