use alkanes_runtime::{
    declare_alkane, message::MessageDispatch, runtime::AlkaneResponder,
    storage::StoragePointer,
};
use alkanes_support::response::CallResponse;
use anyhow::Result;
use metashrew_support::compat::to_arraybuffer_layout;
use metashrew_support::index_pointer::KeyValuePointer;

#[derive(Default)]
pub struct {{CONTRACT_RUST_NAME}}(());

#[derive(MessageDispatch)]
enum {{CONTRACT_RUST_NAME}}Message {
    #[opcode(0)]
    Initialize,
}

impl {{CONTRACT_RUST_NAME}} {
    fn initialize(&self) -> Result<CallResponse> {
        let context = self.context()?;
        let mut initialized = StoragePointer::from_keyword("/initialized");
        initialized.set_value(initialized.get_value::<u8>().saturating_add(1));
        Ok(CallResponse::forward(&context.incoming_alkanes))
    }
}

impl AlkaneResponder for {{CONTRACT_RUST_NAME}} {}

declare_alkane! {
    impl AlkaneResponder for {{CONTRACT_RUST_NAME}} {
        type Message = {{CONTRACT_RUST_NAME}}Message;
    }
}
