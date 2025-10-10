import { AlkaliConfig, AlkanesABI } from "./types";
export declare class AlkanesCompiler {
    private config;
    private tempDir;
    constructor(config?: AlkaliConfig);
    compile(sourceCode: string): Promise<{
        bytecode: string;
        abi: AlkanesABI;
    } | void>;
    private createProject;
    parseABI(sourceCode: string): Promise<AlkanesABI>;
}
