use crate::{response::ResponseData, server::ServerContext};
use async_trait::async_trait;
use core::fmt::Debug;
use std::{
    fmt::{self, Display},
    sync::Arc,
};

mod connect;
mod disconnect;
mod status;

pub use connect::Connect;
pub use disconnect::Disconnect;
pub use status::Status;

#[derive(Debug)]
pub(crate) struct CommandContext {
    server_context: Arc<ServerContext>,
}

impl From<Arc<ServerContext>> for CommandContext {
    fn from(server_context: Arc<ServerContext>) -> Self {
        Self { server_context }
    }
}

#[derive(Debug)]
pub(crate) struct CommandError {
    message: String,
}

impl From<String> for CommandError {
    fn from(message: String) -> Self {
        Self { message }
    }
}

impl Display for CommandError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "CommandError {:#?}", self.message)
    }
}

#[async_trait]
pub(crate) trait Command: Send + Sync {
    async fn handle(&self, context: CommandContext) -> Result<ResponseData, CommandError>;
}

impl Debug for dyn Command {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Command")
    }
}
