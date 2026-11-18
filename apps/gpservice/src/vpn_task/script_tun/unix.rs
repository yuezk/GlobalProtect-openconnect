use anyhow::{Context, ensure};
use gpapi::script_tun::{Bootstrap, ScriptTunRequest};
use openconnect::{Vpn, VpnBuilder};
pub(super) type ControlStream = tokio::net::UnixStream;
pub(crate) struct PreparedScriptTun {
  pub(super) control: Option<std::os::unix::net::UnixStream>,
  pub bootstrap: Bootstrap,
}

pub(crate) fn builder(
  server: &str,
  cookie: &str,
  request: &ScriptTunRequest,
  uid: u32,
) -> anyhow::Result<(VpnBuilder, PreparedScriptTun)> {
  request.validate()?;
  ensure!(uid != 0, "Script-tun worker requires a desktop user");
  let (control, child) = std::os::unix::net::UnixStream::pair()?;
  let builder = Vpn::builder(server, cookie).script_tun_program(
    request.program.to_str().context("Program is not UTF-8")?,
    &request.args,
    uid,
    child.into(),
  )?;
  Ok((
    builder,
    PreparedScriptTun {
      control: Some(control),
      bootstrap: request.bootstrap.clone(),
    },
  ))
}

impl PreparedScriptTun {
  pub(super) async fn connect(&mut self) -> anyhow::Result<ControlStream> {
    let control = self.control.take().context("Worker control pipe is unavailable")?;
    control.set_nonblocking(true)?;
    Ok(tokio::net::UnixStream::from_std(control)?)
  }
  // The native OpenConnect Unix launcher owns/reaps its child on disconnect.
  pub(super) fn stop_worker(&mut self) -> anyhow::Result<()> {
    Ok(())
  }
}
