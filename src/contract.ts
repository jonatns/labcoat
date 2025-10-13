import { AlkanesMethod, ContractConfig } from "./types.js";

export class AlkanesContract {
  private config: ContractConfig;

  constructor(config: ContractConfig) {
    this.config = config;
  }

  async deploy(params: any[] = []): Promise<string> {
    // Implementation for deploying contract
    // 1. Get deploy TX data
    // 2. Sign transaction
    // 3. Broadcast to network
    // 4. Return contract address
    return "contract_address";
  }

  async call(methodName: string, params: any[] = []): Promise<any> {
    const method = this.findMethod(methodName);
    if (!method) {
      throw new Error(`Method ${methodName} not found in ABI`);
    }

    // Implementation for calling contract method
    // 1. Encode params according to ABI
    // 2. Create transaction
    // 3. Return result
  }

  private findMethod(name: string): AlkanesMethod | undefined {
    return this.config.abi.methods.find((m) => m.name === name);
  }
}
