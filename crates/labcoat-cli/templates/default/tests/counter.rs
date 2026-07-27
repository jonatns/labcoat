use labcoat_test::ContractHarness;

fn returned_count(data: &[u8]) -> Result<u128, Box<dyn std::error::Error>> {
    Ok(u128::from_le_bytes(data.try_into()?))
}

#[test]
fn initializes_and_reads_zero() -> Result<(), Box<dyn std::error::Error>> {
    let mut contract = ContractHarness::for_contract("counter")?;

    let initialized = contract.call_method("initialize", &[])?;
    assert_eq!(returned_count(&initialized.data)?, 0);

    let initial = contract.call_opcode(20, &[])?;
    assert_eq!(returned_count(&initial.data)?, 0);
    assert_eq!(contract.storage_u128(b"/count"), Some(0));
    Ok(())
}

#[test]
fn increments_persisted_count() -> Result<(), Box<dyn std::error::Error>> {
    let mut contract = ContractHarness::for_contract("counter")?;
    contract.call_method("initialize", &[])?;

    let first = contract.call_method("increment", &[])?;
    assert_eq!(returned_count(&first.data)?, 1);
    let second = contract.call_method("increment", &[])?;
    assert_eq!(returned_count(&second.data)?, 2);

    let current = contract.call_method("get_count", &[])?;
    assert_eq!(returned_count(&current.data)?, 2);
    assert_eq!(contract.storage_u128(b"/count"), Some(2));
    Ok(())
}

#[test]
fn rejects_overflow_without_changing_state() -> Result<(), Box<dyn std::error::Error>> {
    let mut contract = ContractHarness::for_contract("counter")?;
    contract.call_method("initialize", &[])?;
    contract.set_storage(b"/count", u128::MAX.to_le_bytes());

    assert!(contract.call_method("increment", &[]).is_err());
    assert_eq!(contract.storage_u128(b"/count"), Some(u128::MAX));
    Ok(())
}
