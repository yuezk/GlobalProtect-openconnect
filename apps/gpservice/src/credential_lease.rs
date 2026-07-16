use std::sync::Arc;

use tokio::{
  io::{AsyncReadExt, AsyncWriteExt},
  net::UnixStream,
};

use crate::session_registry::SessionRegistry;

pub(crate) async fn serve(mut stream: UnixStream, registry: Arc<SessionRegistry>) -> anyhow::Result<()> {
  let credential = registry.issue(env!("CARGO_PKG_VERSION"))?;
  let session_id = credential.session_id();
  let result = async {
    stream.write_all(&credential.encode_frame()?).await?;

    let mut buffer = [0_u8; 1];
    loop {
      match stream.read(&mut buffer).await {
        Ok(0) => break,
        Ok(_) => continue,
        Err(err) => return Err(err.into()),
      }
    }
    Ok(())
  }
  .await;
  registry.revoke(session_id);
  result
}
