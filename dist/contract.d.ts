import { ContractConfig } from "./types";
export declare class AlkanesContract {
    private config;
    constructor(config: ContractConfig);
    deploy(params?: any[]): Promise<string>;
    call(methodName: string, params?: any[]): Promise<any>;
    private findMethod;
}
