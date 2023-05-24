use super::{Command, CommandContext, CommandError};
use crate::ResponseData;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Disconnect;

#[async_trait]
impl Command for Disconnect {
    async fn handle(&self, context: CommandContext) -> Result<ResponseData, CommandError> {
        context.server_context.vpn().disconnect().await;
        Ok(ResponseData::Empty)
    }
}
