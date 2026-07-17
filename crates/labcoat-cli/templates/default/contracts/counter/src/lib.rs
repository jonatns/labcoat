use alkanes_runtime::{
    declare_alkane, message::MessageDispatch, runtime::AlkaneResponder, storage::StoragePointer,
};
use alkanes_support::response::CallResponse;
use anyhow::{anyhow, Result};
use metashrew_support::{
    compat::to_arraybuffer_layout,
    index_pointer::KeyValuePointer,
};

#[derive(Default)]
pub struct Counter(());

#[derive(MessageDispatch)]
enum CounterMessage {
    #[opcode(0)]
    Initialize,

    #[opcode(1)]
    #[returns(u128)]
    Increment,

    #[opcode(2)]
    #[returns(u128)]
    GetCount,
}

impl Counter {
    fn count_pointer(&self) -> StoragePointer {
        StoragePointer::from_keyword("/count")
    }

    fn response_with_count(&self, count: u128) -> Result<CallResponse> {
        let context = self.context()?;
        let mut response = CallResponse::forward(&context.incoming_alkanes);
        response.data = count.to_le_bytes().to_vec();
        Ok(response)
    }

    fn initialize(&self) -> Result<CallResponse> {
        self.observe_initialization()?;
        self.count_pointer().set_value::<u128>(0);
        self.response_with_count(0)
    }

    fn increment(&self) -> Result<CallResponse> {
        let mut pointer = self.count_pointer();
        let count = pointer
            .get_value::<u128>()
            .checked_add(1)
            .ok_or_else(|| anyhow!("counter overflow"))?;
        pointer.set_value::<u128>(count);
        self.response_with_count(count)
    }

    fn get_count(&self) -> Result<CallResponse> {
        self.response_with_count(self.count_pointer().get_value::<u128>())
    }
}

impl AlkaneResponder for Counter {}

declare_alkane! {
    impl AlkaneResponder for Counter {
        type Message = CounterMessage;
    }
}
