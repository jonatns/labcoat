"use strict";
Object.defineProperty(exports, "__esModule", { value: true });
const compiler_1 = require("../compiler");
describe("AlkanesCompiler", () => {
    const compiler = new compiler_1.AlkanesCompiler();
    describe("parseABI", () => {
        it("should parse a basic contract", async () => {
            const sourceCode = `
use alkanes_runtime::declare_alkane;
use alkanes_runtime::runtime::AlkaneResponder;
use alkanes_support::response::CallResponse;
use anyhow::Result;

#[derive(Default)]
pub struct SimpleToken(());

impl AlkaneResponder for SimpleToken {
    fn execute(&self) -> Result<CallResponse> {
        let context = self.context()?;
        let mut inputs = context.inputs.clone();
        let mut response = CallResponse::forward(&context.incoming_alkanes);

        match shift_or_err(&mut inputs)? {
            /* initialize(u128, u128) */
            0 => {
                let mut pointer = StoragePointer::from_keyword("/initialized");
                Ok(response)
            },
            /* mint(u128) */
            77 => {
                let amount = shift_or_err(&mut inputs)?;
                Ok(response)
            },
            /* name() */
            99 => {
                response.data = self.name().into_bytes().to_vec();
                Ok(response)
            },
            _ => Err(anyhow!("unrecognized opcode"))
        }
    }
}`;
            const abi = await compiler.parseABI(sourceCode);
            expect(abi).toMatchObject({
                name: "SimpleToken",
                methods: [
                    {
                        opcode: 0,
                        name: "initialize",
                        inputs: [
                            { name: "param0", type: "u128" },
                            { name: "param1", type: "u128" },
                        ],
                        outputs: [],
                    },
                    {
                        opcode: 77,
                        name: "mint",
                        inputs: [{ name: "param0", type: "u128" }],
                        outputs: [],
                    },
                    {
                        opcode: 99,
                        name: "name",
                        inputs: [],
                        outputs: [],
                    },
                ],
                storage: [
                    {
                        key: "/initialized",
                        type: "Vec<u8>",
                    },
                ],
                opcodes: {
                    initialize: 0,
                    mint: 77,
                    name: 99,
                },
            });
        });
        it("should parse array parameters", async () => {
            const sourceCode = `
// ... contract setup ...
match shift_or_err(&mut inputs)? {
    /* setArray(u128[2]) */
    1 => {
        Ok(response)
    }
}`;
            const abi = await compiler.parseABI(sourceCode);
            expect(abi.methods[0]).toMatchObject({
                opcode: 1,
                name: "setArray",
                inputs: [
                    {
                        name: "param0",
                        type: {
                            array: {
                                type: "u128",
                                length: 2,
                            },
                        },
                    },
                ],
            });
        });
        it("should handle multiple storage pointers", async () => {
            const sourceCode = `
// ... contract setup ...
let initialized = StoragePointer::from_keyword("/initialized");
let total_supply = StoragePointer::from_keyword("/total-supply");
let owner = StoragePointer::from_keyword("/owner");`;
            const abi = await compiler.parseABI(sourceCode);
            expect(abi.storage).toEqual([
                { key: "/initialized", type: "Vec<u8>" },
                { key: "/total-supply", type: "Vec<u8>" },
                { key: "/owner", type: "Vec<u8>" },
            ]);
        });
        it("should handle missing method comments gracefully", async () => {
            const sourceCode = `
match shift_or_err(&mut inputs)? {
    0 => { Ok(response) },
    1 => { Ok(response) }
}`;
            const abi = await compiler.parseABI(sourceCode);
            expect(abi.methods).toEqual([]);
        });
    });
});
//# sourceMappingURL=compiler.test.js.map