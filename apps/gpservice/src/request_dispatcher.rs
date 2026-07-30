use std::{
  fs::{self, File, OpenOptions, Permissions},
  io::{BufReader, Read, Write},
  os::unix::fs::{OpenOptionsExt, PermissionsExt},
  path::Path,
  sync::{Arc, atomic::AtomicBool},
};

use common::binary_paths;
use gpapi::{
  service::{
    request::{UpdateGuiRequest, WsRequest},
    transport::{ServiceErrorCode, ServiceResult},
  },
  utils::{checksum::verify_checksum, redact::Redaction},
};
use log::{info, warn};
use tar::Archive;
use tempfile::NamedTempFile;
use tokio::sync::{Mutex, mpsc};
use xz2::read::XzDecoder;

const MAX_GUI_ARCHIVE_SIZE: u64 = 256 * 1024 * 1024;
const MAX_GUI_ARCHIVE_CONTENT_SIZE: u64 = 512 * 1024 * 1024;
const MAX_GUI_BINARY_SIZE: u64 = 256 * 1024 * 1024;

pub struct RequestDispatcher {
  ws_req_tx: mpsc::Sender<WsRequest>,
  gui_restart_requested: Arc<AtomicBool>,
  redaction: Arc<Redaction>,
  install_lock: Mutex<()>,
  gui_management_enabled: bool,
  brokered_scripts_dir: Option<std::path::PathBuf>,
}

impl RequestDispatcher {
  pub fn new(
    ws_req_tx: mpsc::Sender<WsRequest>,
    gui_restart_requested: Arc<AtomicBool>,
    redaction: Arc<Redaction>,
    gui_management_enabled: bool,
    brokered_scripts_dir: Option<std::path::PathBuf>,
  ) -> Self {
    Self {
      ws_req_tx,
      gui_restart_requested,
      redaction,
      install_lock: Mutex::new(()),
      gui_management_enabled,
      brokered_scripts_dir,
    }
  }

  pub async fn dispatch(&self, request: WsRequest) -> ServiceResult {
    match request {
      WsRequest::RestartGui => {
        if !self.gui_management_enabled {
          return ServiceResult::rejected(ServiceErrorCode::InvalidRequest, "GUI restart is unavailable on macOS");
        }
        info!("GUI restart requested");
        self
          .gui_restart_requested
          .store(true, std::sync::atomic::Ordering::SeqCst);
        ServiceResult::Accepted
      }
      WsRequest::UpdateGui(request) => {
        if !self.gui_management_enabled {
          return ServiceResult::rejected(ServiceErrorCode::InvalidRequest, "GUI update is unavailable on macOS");
        }
        self.update_gui(request).await
      }
      WsRequest::GetVpncScriptMetadata => {
        #[cfg(any(target_os = "linux", target_os = "freebsd", target_os = "openbsd"))]
        {
          match gpservice::vpnc_script::metadata() {
            Ok(metadata) => ServiceResult::VpncScriptMetadata(metadata),
            Err(err) => {
              warn!("Failed to read VPNC script metadata: {err}");
              ServiceResult::rejected(ServiceErrorCode::Internal, "Failed to read VPNC script metadata")
            }
          }
        }
        #[cfg(not(any(target_os = "linux", target_os = "freebsd", target_os = "openbsd")))]
        {
          ServiceResult::VpncScriptMetadata(None)
        }
      }
      request => {
        if let WsRequest::Connect(ref connect) = request {
          if let Err(message) = self.validate_connect_paths(connect) {
            return ServiceResult::rejected(ServiceErrorCode::InvalidRequest, message);
          }
          if let Err(err) = self
            .redaction
            .add_values(&[connect.gateway().server(), connect.args().cookie()])
          {
            warn!("Failed to update log redaction: {err}");
            return ServiceResult::rejected(ServiceErrorCode::Internal, "Request could not be accepted");
          }
        }

        match self.ws_req_tx.try_send(request) {
          Ok(()) => ServiceResult::Accepted,
          Err(mpsc::error::TrySendError::Full(_)) => {
            ServiceResult::rejected(ServiceErrorCode::Busy, "Service request queue is full")
          }
          Err(mpsc::error::TrySendError::Closed(_)) => {
            ServiceResult::rejected(ServiceErrorCode::Internal, "VPN task is unavailable")
          }
        }
      }
    }
  }

  fn validate_connect_paths(&self, request: &gpapi::service::request::ConnectRequest) -> Result<(), &'static str> {
    let Some(scripts) = &self.brokered_scripts_dir else {
      return Ok(());
    };
    let expected_script = scripts.join("vpnc-script").to_string_lossy().into_owned();
    let expected_wrapper = scripts.join("hipreport.sh").to_string_lossy().into_owned();
    if request.args().vpnc_script().as_deref() != Some(expected_script.as_str()) {
      return Err("macOS VPN script must be the bundled script");
    }
    if request.args().csd_wrapper().as_deref() != Some(expected_wrapper.as_str()) {
      return Err("macOS HIP wrapper must be the bundled wrapper");
    }
    if request.args().certificate().is_some() || request.args().sslkey().is_some() {
      return Err("macOS client identity must be provided as protected data, not root-readable paths");
    }
    Ok(())
  }

  async fn update_gui(&self, request: UpdateGuiRequest) -> ServiceResult {
    let _install = self.install_lock.lock().await;
    let path = request.path;
    let checksum = request.checksum;
    let installed = tokio::task::spawn_blocking(move || install_gui(&path, &checksum)).await;
    match installed {
      Ok(Ok(())) => ServiceResult::Accepted,
      Ok(Err(GuiInstallError::Checksum(err))) => {
        warn!("Failed to verify GUI update checksum: {err}");
        ServiceResult::rejected(ServiceErrorCode::ChecksumFailed, "GUI update checksum failed")
      }
      Ok(Err(GuiInstallError::Install(err))) => {
        warn!("Failed to install GUI update: {err}");
        ServiceResult::rejected(ServiceErrorCode::InstallFailed, "GUI update installation failed")
      }
      Err(err) => {
        warn!("GUI update task failed: {err}");
        ServiceResult::rejected(ServiceErrorCode::InstallFailed, "GUI update installation failed")
      }
    }
  }
}

#[derive(Debug)]
enum GuiInstallError {
  Checksum(anyhow::Error),
  Install(anyhow::Error),
}

fn install_gui(src: &str, checksum: &str) -> Result<(), GuiInstallError> {
  let target = binary_paths::gpgui();
  let Some(dir) = target.parent() else {
    return Err(GuiInstallError::Install(anyhow::anyhow!(
      "Failed to get parent directory of GUI binary"
    )));
  };
  fs::create_dir_all(dir).map_err(|err| GuiInstallError::Install(err.into()))?;

  let staged_archive = stage_gui_archive(Path::new(src), dir)?;
  let archive_path = staged_archive.path().to_string_lossy();
  verify_checksum(&archive_path, checksum).map_err(GuiInstallError::Checksum)?;

  let tar = XzDecoder::new(BufReader::new(
    staged_archive
      .reopen()
      .map_err(|err| GuiInstallError::Install(err.into()))?,
  ))
  .take(MAX_GUI_ARCHIVE_CONTENT_SIZE);
  let mut archive = Archive::new(tar);
  let file = extract_gui_binary(&mut archive, dir)?;
  file
    .as_file()
    .set_permissions(Permissions::from_mode(0o755))
    .map_err(|err| GuiInstallError::Install(err.into()))?;
  file
    .as_file()
    .sync_all()
    .map_err(|err| GuiInstallError::Install(err.into()))?;
  file
    .persist(&target)
    .map_err(|err| GuiInstallError::Install(err.into()))?;
  File::open(dir)
    .and_then(|directory| directory.sync_all())
    .map_err(|err| GuiInstallError::Install(err.into()))?;
  Ok(())
}

fn stage_gui_archive(src: &Path, dir: &Path) -> Result<NamedTempFile, GuiInstallError> {
  let mut source = OpenOptions::new()
    .read(true)
    .custom_flags(nix::libc::O_NONBLOCK | nix::libc::O_NOFOLLOW)
    .open(src)
    .map_err(|err| GuiInstallError::Install(err.into()))?;
  let metadata = source.metadata().map_err(|err| GuiInstallError::Install(err.into()))?;
  let source_size = metadata.len();
  if !metadata.file_type().is_file() || source_size == 0 || source_size > MAX_GUI_ARCHIVE_SIZE {
    return Err(GuiInstallError::Install(anyhow::anyhow!(
      "GUI update archive must be a non-empty regular file no larger than {MAX_GUI_ARCHIVE_SIZE} bytes"
    )));
  }

  let mut staged = NamedTempFile::new_in(dir).map_err(|err| GuiInstallError::Install(err.into()))?;
  let copied = std::io::copy(&mut (&mut source).take(MAX_GUI_ARCHIVE_SIZE + 1), &mut staged)
    .map_err(|err| GuiInstallError::Install(err.into()))?;
  if copied != source_size {
    return Err(GuiInstallError::Install(anyhow::anyhow!(
      "GUI update archive changed while it was being read"
    )));
  }
  staged.flush().map_err(|err| GuiInstallError::Install(err.into()))?;
  staged
    .as_file()
    .sync_all()
    .map_err(|err| GuiInstallError::Install(err.into()))?;
  Ok(staged)
}

fn extract_gui_binary<R: Read>(archive: &mut Archive<R>, dir: &Path) -> Result<NamedTempFile, GuiInstallError> {
  for entry in archive.entries().map_err(|err| GuiInstallError::Install(err.into()))? {
    let mut entry = entry.map_err(|err| GuiInstallError::Install(err.into()))?;
    let is_gui = entry
      .path()
      .map_err(|err| GuiInstallError::Install(err.into()))?
      .file_name()
      .is_some_and(|name| name == "gpgui");
    if is_gui && entry.header().entry_type().is_file() {
      let declared_size = entry.size();
      if declared_size == 0 || declared_size > MAX_GUI_BINARY_SIZE {
        return Err(GuiInstallError::Install(anyhow::anyhow!(
          "GUI binary must be non-empty and no larger than {MAX_GUI_BINARY_SIZE} bytes"
        )));
      }
      let mut file = NamedTempFile::new_in(dir).map_err(|err| GuiInstallError::Install(err.into()))?;
      let copied = std::io::copy(&mut entry, &mut file).map_err(|err| GuiInstallError::Install(err.into()))?;
      if copied != declared_size {
        return Err(GuiInstallError::Install(anyhow::anyhow!(
          "GUI binary size does not match its archive header"
        )));
      }
      file.flush().map_err(|err| GuiInstallError::Install(err.into()))?;
      return Ok(file);
    }
  }
  Err(GuiInstallError::Install(anyhow::anyhow!(
    "GUI update archive does not contain a non-empty regular gpgui file"
  )))
}

#[cfg(test)]
mod tests {
  use std::io::Cursor;

  use gpapi::{
    gateway::Gateway,
    service::{
      request::ConnectRequest,
      transport::{ServiceErrorCode, ServiceResult},
    },
  };
  use tar::{Builder, EntryType, Header};

  use super::*;

  fn archive_with_entry(entry_type: EntryType, contents: &[u8]) -> Vec<u8> {
    let mut data = Vec::new();
    {
      let mut builder = Builder::new(&mut data);
      let mut header = Header::new_gnu();
      header.set_entry_type(entry_type);
      header.set_size(contents.len() as u64);
      header.set_mode(0o755);
      header.set_cksum();
      builder.append_data(&mut header, "gpgui", contents).unwrap();
      builder.finish().unwrap();
    }
    data
  }

  #[test]
  fn rejects_non_file_and_empty_gui_entries() {
    let output = tempfile::tempdir().unwrap();
    for (entry_type, contents) in [(EntryType::Directory, &[][..]), (EntryType::Regular, &[][..])] {
      let data = archive_with_entry(entry_type, contents);
      let mut archive = Archive::new(Cursor::new(data));
      assert!(extract_gui_binary(&mut archive, output.path()).is_err());
    }
  }

  #[test]
  fn extracts_non_empty_regular_gui_entry() {
    let output = tempfile::tempdir().unwrap();
    let data = archive_with_entry(EntryType::Regular, b"gui binary");
    let mut archive = Archive::new(Cursor::new(data));
    let extracted = extract_gui_binary(&mut archive, output.path()).unwrap();
    let contents = std::fs::read(extracted.path()).unwrap();
    assert_eq!(contents, b"gui binary");
  }

  #[test]
  fn rejects_invalid_archive_sources() {
    let output = tempfile::tempdir().unwrap();
    assert!(stage_gui_archive(output.path(), output.path()).is_err());

    let oversized = output.path().join("oversized.tar.xz");
    File::create(&oversized)
      .unwrap()
      .set_len(MAX_GUI_ARCHIVE_SIZE + 1)
      .unwrap();
    assert!(stage_gui_archive(&oversized, output.path()).is_err());
  }

  #[test]
  fn rejects_oversized_gui_entry() {
    let output = tempfile::tempdir().unwrap();
    let mut header = Header::new_gnu();
    header.set_entry_type(EntryType::Regular);
    header.set_size(MAX_GUI_BINARY_SIZE + 1);
    header.set_mode(0o755);
    header.set_path("gpgui").unwrap();
    header.set_cksum();
    let mut data = header.as_bytes().to_vec();
    data.resize(512 * 3, 0);

    let mut archive = Archive::new(Cursor::new(data));
    assert!(extract_gui_binary(&mut archive, output.path()).is_err());
  }

  #[tokio::test]
  async fn brokered_mode_rejects_gui_management_and_root_readable_identity_paths() {
    let (request_tx, _request_rx) = mpsc::channel(1);
    let dispatcher = RequestDispatcher::new(
      request_tx,
      Arc::new(AtomicBool::new(false)),
      Arc::new(Redaction::new()),
      false,
      Some(std::path::PathBuf::from("/app/Contents/Resources/Scripts")),
    );
    let ServiceResult::Rejected(rejection) = dispatcher.dispatch(WsRequest::RestartGui).await else {
      panic!("brokered mode accepted GUI restart");
    };
    assert_eq!(rejection.code(), ServiceErrorCode::InvalidRequest);

    let gateway = Gateway::new("Gateway".into(), "vpn.example.com".into());
    let info = gpapi::service::vpn_state::ConnectInfo::new("portal.example.com".into(), gateway.clone(), vec![gateway]);
    let request = ConnectRequest::new(info, "cookie".into())
      .with_vpnc_script(Some("/app/Contents/Resources/Scripts/vpnc-script".to_string()))
      .with_csd_wrapper(Some("/app/Contents/Resources/Scripts/hipreport.sh".to_string()))
      .with_certificate(Some("/Users/example/client.pem".to_string()));
    let ServiceResult::Rejected(rejection) = dispatcher.dispatch(WsRequest::Connect(Box::new(request))).await else {
      panic!("brokered mode accepted a root-readable identity path");
    };
    assert_eq!(rejection.code(), ServiceErrorCode::InvalidRequest);
  }

  #[test]
  fn brokered_mode_accepts_only_resource_script_paths() {
    let (request_tx, _request_rx) = mpsc::channel(1);
    let dispatcher = RequestDispatcher::new(
      request_tx,
      Arc::new(AtomicBool::new(false)),
      Arc::new(Redaction::new()),
      false,
      Some(std::path::PathBuf::from("/app/Contents/Resources/Scripts")),
    );
    let gateway = Gateway::new("Gateway".into(), "vpn.example.com".into());
    let info = gpapi::service::vpn_state::ConnectInfo::new("portal.example.com".into(), gateway.clone(), vec![gateway]);
    let bundled = ConnectRequest::new(info.clone(), "cookie".into())
      .with_vpnc_script(Some("/app/Contents/Resources/Scripts/vpnc-script".to_string()))
      .with_csd_wrapper(Some("/app/Contents/Resources/Scripts/hipreport.sh".to_string()));
    assert_eq!(dispatcher.validate_connect_paths(&bundled), Ok(()));

    let old_layout = ConnectRequest::new(info, "cookie".into())
      .with_vpnc_script(Some("/app/Contents/Helpers/vpnc-script".to_string()))
      .with_csd_wrapper(Some("/app/Contents/Helpers/hipreport.sh".to_string()));
    assert_eq!(
      dispatcher.validate_connect_paths(&old_layout),
      Err("macOS VPN script must be the bundled script")
    );
  }
}
