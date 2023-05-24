use super::{Command, CommandContext, CommandError};
use crate::{ResponseData, VpnStatus};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Connect {
    server: String,
    cookie: String,
}

impl Connect {
    pub fn new(server: String, cookie: String) -> Self {
        Self { server, cookie }
    }
}

#[async_trait]
impl Command for Connect {
    async fn handle(&self, context: CommandContext) -> Result<ResponseData, CommandError> {
        let vpn = context.server_context.vpn();
        let status = vpn.status().await;

        if status != VpnStatus::Disconnected {
            return Err(format!("VPN is already in state: {:?}", status).into());
        }

        if let Err(err) = vpn.connect(&self.server, &self.cookie).await {
            return Err(err.to_string().into());
        }

        Ok(ResponseData::Empty)
    }
}
