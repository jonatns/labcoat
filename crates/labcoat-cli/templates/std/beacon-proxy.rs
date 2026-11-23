//! Standard beacon proxy.
//!
//! Keeps its own storage and balances and delegates every call it does not
//! handle to whatever implementation its beacon currently points at. The
//! proxy has no upgrade method of its own: upgrading the beacon upgrades
//! it. The proxy's id never changes.
//!
//! Vendored from alkanes-rs `alkanes-std-beacon-proxy` (MIT). Keep the
//! opcodes as they are: tools and other contracts call them by number.

use alkanes_runtime::declare_alkane;
use alkanes_runtime::message::MessageDispatch;
use alkanes_runtime::{runtime::AlkaneResponder, storage::StoragePointer};
use alkanes_support::{
    cellpack::Cellpack, id::AlkaneId, parcel::AlkaneTransferParcel, response::CallResponse,
};
use anyhow::Result;
use metashrew_support::compat::to_arraybuffer_layout;
use metashrew_support::index_pointer::KeyValuePointer;
use std::sync::Arc;

#[derive(Default)]
pub struct BeaconProxy(());

#[derive(MessageDispatch)]
enum BeaconProxyMessage {
    #[opcode(0x7fff)]
    Initialize { beacon: AlkaneId },

    #[opcode(0x8fff)]
    Forward {},
}

impl BeaconProxy {
    fn forward(&self) -> Result<CallResponse> {
        let context = self.context()?;
        Ok(CallResponse::forward(&context.incoming_alkanes))
    }

    fn beacon_pointer() -> StoragePointer {
        StoragePointer::from_keyword("/beacon")
    }

    /// Ask the beacon (opcode `0x7ffd`) which implementation to run.
    fn implementation(&self) -> Result<AlkaneId> {
        let beacon = Self::beacon_pointer().get().as_ref().clone().try_into()?;
        let response = self.staticcall(
            &Cellpack {
                target: beacon,
                inputs: vec![0x7ffd],
            },
            &AlkaneTransferParcel::default(),
            self.fuel(),
        )?;
        Ok(response.data.try_into()?)
    }

    fn initialize(&self, beacon: AlkaneId) -> Result<CallResponse> {
        self.observe_proxy_initialization()?;
        let context = self.context()?;
        Self::beacon_pointer().set(Arc::new(<AlkaneId as Into<Vec<u8>>>::into(beacon)));
        Ok(CallResponse::forward(&context.incoming_alkanes))
    }
}

impl AlkaneResponder for BeaconProxy {
    fn fallback(&self) -> Result<CallResponse> {
        let context = self.context()?;
        let cellpack = Cellpack {
            target: self.implementation()?,
            inputs: context.inputs.clone(),
        };
        self.delegatecall(&cellpack, &context.incoming_alkanes, self.fuel())
    }
}

declare_alkane! {
    impl AlkaneResponder for BeaconProxy {
        type Message = BeaconProxyMessage;
    }
}
