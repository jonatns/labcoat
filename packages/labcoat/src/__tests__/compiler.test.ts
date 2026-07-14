import { AlkanesCompiler } from "@/sdk/compiler.js";

describe("AlkanesCompiler", () => {
  const compiler = new AlkanesCompiler();

  describe("parseABI", () => {
    // NOTE: parseABI reads the #[opcode(n)] attribute grammar (MessageDispatch
    // enums). Two legacy cases here previously asserted an older
    // comment-annotation grammar (/* name(u128) */ before match arms) that
    // parseABI never supported — they were rewritten for the real grammar.
    it("should parse a basic contract", async () => {
      const sourceCode = `
use alkanes_runtime::declare_alkane;
use alkanes_runtime::runtime::AlkaneResponder;
use alkanes_support::response::CallResponse;
use anyhow::Result;

#[derive(Default)]
pub struct SimpleToken(());

#[derive(MessageDispatch)]
enum SimpleTokenMessage {
    #[opcode(0)]
    Initialize { token_units: u128, cap: u128 },

    #[opcode(77)]
    Mint { amount: u128 },

    #[opcode(99)]
    #[returns(String)]
    Name,
}

impl SimpleToken {
    fn initialized_pointer(&self) -> StoragePointer {
        StoragePointer::from_keyword("/initialized")
    }
}`;

      const abi = await compiler.parseABI(sourceCode);

      expect(abi).toMatchObject({
        name: "SimpleToken",
        methods: [
          {
            opcode: 0,
            name: "Initialize",
            inputs: [
              { name: "token_units", type: "u128" },
              { name: "cap", type: "u128" },
            ],
            outputs: [],
          },
          {
            opcode: 77,
            name: "Mint",
            inputs: [{ name: "amount", type: "u128" }],
            outputs: [],
          },
          {
            opcode: 99,
            name: "Name",
            inputs: [],
            outputs: ["String"],
          },
        ],
        storage: [
          {
            key: "/initialized",
            type: "Vec<u8>",
          },
        ],
        opcodes: {
          Initialize: 0,
          Mint: 77,
          Name: 99,
        },
      });
    });

    it("should parse generic field types like Vec<u128>", async () => {
      const sourceCode = `
pub struct ArrayContract(());

enum ArrayContractMessage {
    #[opcode(1)]
    SetArray { values: Vec<u128> },
}`;

      const abi = await compiler.parseABI(sourceCode);
      expect(abi.methods[0]).toMatchObject({
        opcode: 1,
        name: "SetArray",
        inputs: [{ name: "values", type: "Vec<u128>" }],
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

    it("should ignore sources without opcode attributes", async () => {
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
