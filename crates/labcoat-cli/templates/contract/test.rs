use labcoat_test::{assert_revert, ContractHarness};

#[test]
fn initializes_from_compiled_wasm() -> Result<(), Box<dyn std::error::Error>> {
    let mut contract = ContractHarness::for_contract("{{CONTRACT_NAME}}")?;
    contract.call_method("initialize", &[])?;
    assert_eq!(contract.storage_value(b"/initialized"), Some(&[1][..]));
    assert_revert(contract.call_method("initialize", &[]));
    Ok(())
}
