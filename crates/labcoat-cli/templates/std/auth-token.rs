//! Standard auth token.
//!
//! Deploy this once at reserved number 65517 (`4:65517`). Every standard
//! upgradeable proxy and beacon clones it from there when it initializes,
//! minting the token whose holder may upgrade that contract.
//!
//! Vendored from alkanes-rs `alkanes-std-auth-token` (MIT), with the
//! name and symbol storage inlined so the package needs no extra
//! dependency. Keep the opcodes and behavior as they are: the proxy and
//! beacon contracts call them by number.

use alkanes_runtime::runtime::AlkaneResponder;
use alkanes_runtime::{declare_alkane, message::MessageDispatch, storage::StoragePointer};
use alkanes_support::{parcel::AlkaneTransfer, response::CallResponse};
use anyhow::{anyhow, Result};
use metashrew_support::compat::to_arraybuffer_layout;
use metashrew_support::index_pointer::KeyValuePointer;
use std::sync::Arc;

#[derive(Default)]
pub struct AuthToken(());

#[derive(MessageDispatch)]
enum AuthTokenMessage {
    #[opcode(0)]
    Initialize {
        name: String,
        symbol: String,
        amount: u128,
    },

    #[opcode(1)]
    Authenticate,

    #[opcode(99)]
    #[returns(String)]
    GetName,

    #[opcode(100)]
    #[returns(String)]
    GetSymbol,
}

impl AuthToken {
    fn name_pointer(&self) -> StoragePointer {
        StoragePointer::from_keyword("/name")
    }

    fn symbol_pointer(&self) -> StoragePointer {
        StoragePointer::from_keyword("/symbol")
    }

    fn initialize(&self, name: String, symbol: String, amount: u128) -> Result<CallResponse> {
        self.observe_initialization()?;
        let context = self.context()?;
        self.name_pointer().set(Arc::new(name.into_bytes()));
        self.symbol_pointer().set(Arc::new(symbol.into_bytes()));
        let mut response = CallResponse::forward(&context.incoming_alkanes);
        response.alkanes.0.push(AlkaneTransfer {
            id: context.myself.clone(),
            value: amount,
        });
        Ok(response)
    }

    fn authenticate(&self) -> Result<CallResponse> {
        let context = self.context()?;
        // `forward` returns the supplied unit to the caller.
        let mut response = CallResponse::forward(&context.incoming_alkanes);
        if context.incoming_alkanes.0.len() != 1 {
            return Err(anyhow!(
                "did not authenticate with only the authentication token"
            ));
        }
        let transfer = context.incoming_alkanes.0[0].clone();
        if transfer.id != context.myself {
            return Err(anyhow!("supplied alkane is not authentication token"));
        }
        if transfer.value < 1 {
            return Err(anyhow!(
                "less than 1 unit of authentication token supplied to authenticate"
            ));
        }
        response.data = vec![0x01];
        Ok(response)
    }

    fn get_name(&self) -> Result<CallResponse> {
        let context = self.context()?;
        let mut response = CallResponse::forward(&context.incoming_alkanes);
        response.data = self.name_pointer().get().as_ref().clone();
        Ok(response)
    }

    fn get_symbol(&self) -> Result<CallResponse> {
        let context = self.context()?;
        let mut response = CallResponse::forward(&context.incoming_alkanes);
        response.data = self.symbol_pointer().get().as_ref().clone();
        Ok(response)
    }
}

impl AlkaneResponder for AuthToken {}

declare_alkane! {
    impl AlkaneResponder for AuthToken {
        type Message = AuthTokenMessage;
    }
}
