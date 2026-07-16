use alkanes_runtime::{declare_alkane, message::MessageDispatch, runtime::AlkaneResponder};
use alkanes_support::response::CallResponse;
use anyhow::Result;
use metashrew_support::compat::to_arraybuffer_layout;

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
        Ok(CallResponse::forward(&context.incoming_alkanes))
    }
}

impl AlkaneResponder for {{CONTRACT_RUST_NAME}} {}

declare_alkane! {
    impl AlkaneResponder for {{CONTRACT_RUST_NAME}} {
        type Message = {{CONTRACT_RUST_NAME}}Message;
    }
}
