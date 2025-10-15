import { AlkanesABI } from "./types.js";
export declare class AlkanesCompiler {
    private tempDir;
    compile(contractName: string, sourceCode: string): Promise<{
        wasmBuffer: Buffer;
        abi: AlkanesABI;
    } | void>;
    private createProject;
    parseABI(sourceCode: string): Promise<AlkanesABI>;
}
