//! Standard upgradeable proxy.
//!
//! Keeps its own storage and balances and delegates every call it does not
//! handle to the implementation it points at. Initialization mints an auth
//! token to the deployer; whoever holds it may point the proxy at a new
//! implementation. The proxy's id never changes.
//!
//! Vendored from alkanes-rs `alkanes-std-upgradeable` (MIT). Keep the
//! opcodes as they are: tools and other contracts call them by number.

use alkanes_runtime::auth::AuthenticatedResponder;
use alkanes_runtime::declare_alkane;
use alkanes_runtime::message::MessageDispatch;
use alkanes_runtime::{runtime::AlkaneResponder, storage::StoragePointer};
use alkanes_support::{cellpack::Cellpack, id::AlkaneId, response::CallResponse};
use anyhow::Result;
use metashrew_support::compat::to_arraybuffer_layout;
use metashrew_support::index_pointer::KeyValuePointer;
use std::sync::Arc;

#[derive(Default)]
pub struct Upgradeable(());

#[derive(MessageDispatch)]
enum UpgradeableMessage {
    #[opcode(0x7fff)]
    Initialize {
        implementation: AlkaneId,
        auth_token_units: u128,
    },

    #[opcode(0x7ffe)]
    Upgrade { implementation: AlkaneId },

    #[opcode(0x8fff)]
    Forward {},
}

impl Upgradeable {
    fn forward(&self) -> Result<CallResponse> {
        let context = self.context()?;
        Ok(CallResponse::forward(&context.incoming_alkanes))
    }

    fn implementation_pointer() -> StoragePointer {
        StoragePointer::from_keyword("/implementation")
    }

    fn implementation() -> Result<AlkaneId> {
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
        self.observe_proxy_initialization()?;
        let context = self.context()?;
        Self::set_implementation(implementation);
        let mut response = CallResponse::forward(&context.incoming_alkanes);
        response
            .alkanes
            .0
            .push(self.deploy_auth_token(auth_token_units)?);
        Ok(response)
    }

    fn upgrade(&self, implementation: AlkaneId) -> Result<CallResponse> {
        let context = self.context()?;
        self.only_owner()?;
        Self::set_implementation(implementation);
        Ok(CallResponse::forward(&context.incoming_alkanes))
    }
}

impl AuthenticatedResponder for Upgradeable {}

impl AlkaneResponder for Upgradeable {
    fn fallback(&self) -> Result<CallResponse> {
        let context = self.context()?;
        let cellpack = Cellpack {
            target: Self::implementation()?,
            inputs: context.inputs.clone(),
        };
        self.delegatecall(&cellpack, &context.incoming_alkanes, self.fuel())
    }
}

declare_alkane! {
    impl AlkaneResponder for Upgradeable {
        type Message = UpgradeableMessage;
    }
}
