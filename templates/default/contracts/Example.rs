use alkanes_runtime::{declare_alkane, message::MessageDispatch, runtime::AlkaneResponder};
use alkanes_support::response::CallResponse;
use anyhow::Result;
use metashrew_support::compat::to_arraybuffer_layout;

#[derive(Default)]
pub struct ExampleContract(());

#[derive(MessageDispatch)]
enum ExampleContractMessage {
    #[opcode(0)]
    Initialize,

    #[opcode(1)]
    #[returns(String)]
    Greet { name: String },
}

impl ExampleContract {
    fn initialize(&self) -> Result<CallResponse> {
        let context = self.context()?;
        let response = CallResponse::forward(&context.incoming_alkanes);
        Ok(response)
    }

    fn greet(&self, name: String) -> Result<CallResponse> {
        let context = self.context()?;
        let mut response = CallResponse::forward(&context.incoming_alkanes);

        let message = format!("Hello {}!", name);
        response.data = message.as_bytes().to_vec();

        Ok(response)
    }
}

impl AlkaneResponder for ExampleContract {}

declare_alkane! {
    impl AlkaneResponder for ExampleContract {
        type Message = ExampleContractMessage;
    }
}
