use super::{Command, CommandContext, CommandError};
use crate::ResponseData;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct GetStatus;

#[async_trait]
impl Command for GetStatus {
    async fn handle(&self, context: CommandContext) -> Result<ResponseData, CommandError> {
        let status = context.server_context.vpn().status().await;

        Ok(ResponseData::Status(status))
    }
}
