use anyhow::{Context, ensure};
use gpapi::{
  process::user_process::UserProcess,
  script_tun::{Bootstrap, ScriptTunRequest},
  windows::PipeClientIdentity,
};
use openconnect::{Vpn, VpnBuilder};
use std::{os::windows::io::AsRawHandle, sync::Arc, time::Duration};
use tokio::net::windows::named_pipe::NamedPipeServer;
use windows_sys::Win32::System::Pipes::GetNamedPipeClientProcessId;
pub(super) type ControlStream = NamedPipeServer;

pub(crate) struct PreparedScriptTun {
  control: Option<NamedPipeServer>,
  worker: UserProcess,
  pub(super) bootstrap: Bootstrap,
}
pub(crate) fn builder(
  server: &str,
  cookie: &str,
  request: &ScriptTunRequest,
  owner: Arc<PipeClientIdentity>,
) -> anyhow::Result<(VpnBuilder, PreparedScriptTun)> {
  request.validate()?;
  let attempt = uuid::Uuid::new_v4();
  let control_name = format!(r"\\.\pipe\GPConnect.ScriptTun.{attempt}.Control");
  let packet_name = format!(r"\\.\pipe\GPConnect.ScriptTun.{attempt}.Packets");
  let control = owner.create_control_pipe(&control_name)?;
  let mut packets = owner.create_packet_pipe(&packet_name)?;
  let worker = UserProcess::launch(
    owner.primary_token(),
    &request.program,
    &request.args,
    &[
      ("OPENCONNECT_SCRIPT_TUN_CONTROL_PIPE", &control_name),
      ("VPN_PIPE", &packet_name),
    ],
  )
  .context("Could not launch the user-session packet worker")?;
  packets.connect(worker.id(), Duration::from_secs(10))?;
  // PacketPipe creates only raw overlapped Win32 I/O and never registers IOCP.
  let builder = unsafe { Vpn::builder(server, cookie).script_tun_pipe(packets.into_handle()) };
  Ok((
    builder,
    PreparedScriptTun {
      control: Some(control),
      worker,
      bootstrap: request.bootstrap.clone(),
    },
  ))
}
impl PreparedScriptTun {
  pub(super) async fn connect(&mut self) -> anyhow::Result<ControlStream> {
    ensure!(!self.worker.has_exited()?, "The packet worker exited during startup");
    let control = self.control.take().context("Worker control pipe is unavailable")?;
    tokio::time::timeout(Duration::from_secs(10), control.connect())
      .await
      .context("Packet control connection timed out")??;
    let mut pid = 0;
    if unsafe { GetNamedPipeClientProcessId(control.as_raw_handle(), &mut pid) } == 0 {
      return Err(std::io::Error::last_os_error().into());
    }
    ensure!(pid == self.worker.id(), "Unexpected packet control client");
    Ok(control)
  }
  pub(super) fn stop_worker(&mut self) -> anyhow::Result<()> {
    Ok(self.worker.stop()?)
  }
}
