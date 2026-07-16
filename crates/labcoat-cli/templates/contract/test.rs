use labcoat_test::ContractHarness;

#[test]
fn initializes_from_compiled_wasm() -> Result<(), Box<dyn std::error::Error>> {
    let mut contract = ContractHarness::for_contract("{{CONTRACT_NAME}}")?;
    contract.call_method("initialize", &[])?;
    Ok(())
}
