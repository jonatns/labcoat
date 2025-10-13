import { AlkanesABI } from "./types.js";
export declare class AlkanesCompiler {
    private tempDir;
    compile(sourceCode: string): Promise<{
        bytecode: string;
        abi: AlkanesABI;
    } | void>;
    private createProject;
    parseABI(sourceCode: string): Promise<AlkanesABI>;
}
