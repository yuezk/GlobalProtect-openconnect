use crate::session::tunnel::NegotiatedNetwork;
use serde::{Deserialize, Serialize};
use std::{io, path::PathBuf};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use uuid::Uuid;
const MAX_MESSAGE: usize = 60 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
pub struct Identity {
  pub service_instance: Uuid,
  #[serde(with = "crate::session::tunnel::decimal")]
  #[specta(type = String)]
  pub attempt: u64,
  pub launch_id: Uuid,
}
#[derive(Clone, Default, Serialize, Deserialize, specta::Type)]
#[serde(transparent)]
pub struct Bootstrap(String);
impl std::fmt::Debug for Bootstrap {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    f.write_str("[redacted bootstrap]")
  }
}
impl Bootstrap {
  pub fn new(bytes: &[u8]) -> anyhow::Result<Self> {
    anyhow::ensure!(bytes.len() <= 16 * 1024, "Script-tun bootstrap too large");
    use base64::Engine;
    Ok(Self(base64::engine::general_purpose::STANDARD.encode(bytes)))
  }
  pub fn decode(&self) -> anyhow::Result<Vec<u8>> {
    anyhow::ensure!(self.0.len() <= 21848, "Script-tun bootstrap too large");
    use base64::Engine;
    let bytes = base64::engine::general_purpose::STANDARD.decode(&self.0)?;
    anyhow::ensure!(bytes.len() <= 16 * 1024, "Script-tun bootstrap too large");
    Ok(bytes)
  }
}
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
pub struct ScriptTunRequest {
  pub launch_id: Uuid,
  pub program: PathBuf,
  pub args: Vec<String>,
  pub bootstrap: Bootstrap,
}
impl ScriptTunRequest {
  pub fn validate(&self) -> anyhow::Result<()> {
    let program = self
      .program
      .to_str()
      .ok_or_else(|| anyhow::anyhow!("Executable path must be UTF-8"))?;
    anyhow::ensure!(
      self.program.is_absolute() && !program.contains('\0'),
      "Script-tun requires an absolute executable path"
    );
    anyhow::ensure!(
      self.args.len() <= 64
        && self.args.iter().all(|a| !a.contains('\0'))
        && program.len() + self.args.iter().map(String::len).sum::<usize>() <= 16 * 1024,
      "Invalid script-tun arguments"
    );
    self.bootstrap.decode()?;
    Ok(())
  }
}
#[derive(Debug, Clone, Default, Serialize, Deserialize, specta::Type)]
#[serde(tag = "type", content = "value", rename_all = "camelCase")]
pub enum PacketDelivery {
  #[default]
  System,
  ScriptTun(ScriptTunRequest),
}
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub enum PacketDeliveryKind {
  #[default]
  System,
  ScriptTun,
}
impl PacketDelivery {
  pub fn kind(&self) -> PacketDeliveryKind {
    match self {
      Self::System => PacketDeliveryKind::System,
      Self::ScriptTun(_) => PacketDeliveryKind::ScriptTun,
    }
  }
}
#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type", deny_unknown_fields)]
pub enum Message {
  Configure {
    identity: Identity,
    network: NegotiatedNetwork,
    bootstrap: Bootstrap,
  },
  Ready,
  Activate,
  Activated,
  Suspend,
  Suspended,
  Stop,
  Error {
    message: String,
  },
}
pub async fn send(stream: &mut (impl AsyncWrite + Unpin), message: &Message) -> io::Result<()> {
  let bytes = serde_json::to_vec(message)?;
  if bytes.len() > MAX_MESSAGE {
    return Err(io::Error::other("Script-tun control message too large"));
  }
  stream.write_u32(bytes.len() as u32).await?;
  stream.write_all(&bytes).await
}
pub async fn receive(stream: &mut (impl AsyncRead + Unpin)) -> io::Result<Message> {
  let length = stream.read_u32().await? as usize;
  if length > MAX_MESSAGE {
    return Err(io::Error::other("Script-tun control message too large"));
  }
  let mut bytes = vec![0; length];
  stream.read_exact(&mut bytes).await?;
  serde_json::from_slice(&bytes).map_err(io::Error::from)
}

#[cfg(test)]
mod tests {
  use super::*;

  #[tokio::test]
  async fn rejects_oversized_unknown_and_truncated_frames() {
    let oversized = (MAX_MESSAGE as u32 + 1).to_be_bytes();
    assert!(receive(&mut oversized.as_slice()).await.is_err());
    for payload in [br#"{"type":"UnknownCommand"}"#.as_slice(), b"not JSON"] {
      let mut frame = (payload.len() as u32).to_be_bytes().to_vec();
      frame.extend_from_slice(payload);
      assert!(receive(&mut frame.as_slice()).await.is_err());
    }
    let truncated = [0, 0, 0, 10, b'{'];
    assert!(receive(&mut truncated.as_slice()).await.is_err());
  }
}
