use alkanes_runtime::declare_alkane;
use alkanes_runtime::message::MessageDispatch;
#[allow(unused_imports)]
use alkanes_runtime::{
    println,
    stdio::{stdout, Write},
};
use alkanes_runtime::{runtime::AlkaneResponder, storage::StoragePointer};
use alkanes_support::response::CallResponse;
use anyhow::Result;
use metashrew_support::compat::to_arraybuffer_layout;
use metashrew_support::index_pointer::KeyValuePointer;

#[derive(Default)]
pub struct StorageContract(());

#[derive(MessageDispatch)]
enum StorageContractMessage {
    #[opcode(0)]
    Initialize,

    #[opcode(1)]
    Store { value: u128 },

    #[opcode(2)]
    #[returns(u128)]
    Retrieve,
}

impl StorageContract {
    pub fn value_pointer(&self) -> StoragePointer {
        StoragePointer::from_keyword("/storage/value")
    }

    fn initialize(&self) -> Result<CallResponse> {
        let context = self.context()?;
        let response = CallResponse::forward(&context.incoming_alkanes);
        Ok(response)
    }

    fn store(&self, value: u128) -> Result<CallResponse> {
        let context = self.context()?;
        let response = CallResponse::forward(&context.incoming_alkanes);

        self.value_pointer().set_value::<u128>(value);

        Ok(response)
    }

    fn retrieve(&self) -> Result<CallResponse> {
        let context = self.context()?;
        let mut response = CallResponse::forward(&context.incoming_alkanes);

        let value = self.value_pointer().get_value::<u128>();
        response.data = (&value.to_le_bytes()).to_vec();

        Ok(response)
    }
}

impl AlkaneResponder for StorageContract {}

declare_alkane! {
    impl AlkaneResponder for StorageContract {
        type Message = StorageContractMessage;
    }
}
