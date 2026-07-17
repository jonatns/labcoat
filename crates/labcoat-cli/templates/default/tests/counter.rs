use labcoat_test::ContractHarness;

fn returned_count(data: &[u8]) -> Result<u128, Box<dyn std::error::Error>> {
    Ok(u128::from_le_bytes(data.try_into()?))
}

#[test]
fn increments_persisted_count() -> Result<(), Box<dyn std::error::Error>> {
    let mut contract = ContractHarness::for_contract("counter")?;

    let initialized = contract.call_method("initialize", &[])?;
    assert_eq!(returned_count(&initialized.data)?, 0);

    let initial = contract.call_method("get_count", &[])?;
    assert_eq!(returned_count(&initial.data)?, 0);

    contract.call_method("increment", &[])?;
    contract.call_method("increment", &[])?;

    let current = contract.call_method("get_count", &[])?;
    assert_eq!(returned_count(&current.data)?, 2);
    assert_eq!(contract.storage_u128(b"/count"), Some(2));
    Ok(())
}
