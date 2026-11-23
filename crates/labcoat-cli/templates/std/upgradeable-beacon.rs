//! Standard upgradeable beacon.
//!
//! Holds one implementation id that any number of beacon proxies follow.
//! Initialization mints an auth token to the deployer; whoever holds it may
//! point the beacon at a new implementation, which upgrades every proxy
//! that follows it in one transaction. The beacon's id never changes.
//!
//! Vendored from alkanes-rs `alkanes-std-upgradeable-beacon` (MIT). Keep
//! the opcodes as they are: beacon proxies read `0x7ffd` by number.

use alkanes_runtime::auth::AuthenticatedResponder;
use alkanes_runtime::declare_alkane;
use alkanes_runtime::message::MessageDispatch;
use alkanes_runtime::{runtime::AlkaneResponder, storage::StoragePointer};
use alkanes_support::{id::AlkaneId, response::CallResponse};
use anyhow::Result;
use metashrew_support::compat::to_arraybuffer_layout;
use metashrew_support::index_pointer::KeyValuePointer;
use std::sync::Arc;

#[derive(Default)]
pub struct UpgradeableBeacon(());

#[derive(MessageDispatch)]
enum UpgradeableBeaconMessage {
    #[opcode(0x7fff)]
    Initialize {
        implementation: AlkaneId,
        auth_token_units: u128,
    },

    #[opcode(0x7ffd)]
    Implementation {},

    #[opcode(0x7ffe)]
    UpgradeTo { implementation: AlkaneId },

    #[opcode(0x8fff)]
    Forward {},
}

impl UpgradeableBeacon {
    fn forward(&self) -> Result<CallResponse> {
        let context = self.context()?;
        Ok(CallResponse::forward(&context.incoming_alkanes))
    }

    fn implementation_pointer() -> StoragePointer {
        StoragePointer::from_keyword("/implementation")
    }

    fn current_implementation() -> Result<AlkaneId> {
        Ok(Self::implementation_pointer()
            .get()
            .as_ref()
            .clone()
            .try_into()?)
    }

    fn set_implementation(implementation: AlkaneId) {
        Self::implementation_pointer()
            .set(Arc::new(<AlkaneId as Into<Vec<u8>>>::into(implementation)));
    }

    fn initialize(&self, implementation: AlkaneId, auth_token_units: u128) -> Result<CallResponse> {
        self.observe_initialization()?;
        let context = self.context()?;
        Self::set_implementation(implementation);
        let mut response = CallResponse::forward(&context.incoming_alkanes);
        response
            .alkanes
            .0
            .push(self.deploy_auth_token(auth_token_units)?);
        Ok(response)
    }

    fn implementation(&self) -> Result<CallResponse> {
        let context = self.context()?;
        let mut response = CallResponse::forward(&context.incoming_alkanes);
        response.data = Self::current_implementation()?.into();
        Ok(response)
    }

    fn upgrade_to(&self, implementation: AlkaneId) -> Result<CallResponse> {
        let context = self.context()?;
        self.only_owner()?;
        Self::set_implementation(implementation);
        Ok(CallResponse::forward(&context.incoming_alkanes))
    }
}

impl AuthenticatedResponder for UpgradeableBeacon {}
impl AlkaneResponder for UpgradeableBeacon {}

declare_alkane! {
    impl AlkaneResponder for UpgradeableBeacon {
        type Message = UpgradeableBeaconMessage;
    }
}
