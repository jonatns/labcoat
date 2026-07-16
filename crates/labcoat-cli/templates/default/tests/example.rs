use labcoat_test::{ContractHarness, Value};

#[test]
fn greets_from_compiled_wasm() -> Result<(), Box<dyn std::error::Error>> {
    let mut contract = ContractHarness::for_contract("example")?;
    let result = contract.call_method("greet", &[Value::String("World".into())])?;
    assert_eq!(result.data_text(), "Hello World!");
    Ok(())
}
