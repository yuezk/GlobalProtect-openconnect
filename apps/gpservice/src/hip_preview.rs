use std::{os::unix::process::CommandExt, process::Stdio, sync::Arc, time::Duration};

use anyhow::{Context, bail, ensure};
use gpapi::service::{hip::HipSource, request::PreviewHipReportRequest};
use tokio::{io::AsyncReadExt, process::Command};

const MAX_PREVIEW_BYTES: u64 = 1024 * 1024;
const PREVIEW_TIMEOUT: Duration = Duration::from_secs(60);
const PREVIEW_COOKIE: &str = "user=preview&domain=preview&computer=preview";
const PREVIEW_IP: &str = "192.0.2.1";
const PREVIEW_MD5: &str = "00000000000000000000000000000000";

pub(crate) async fn generate(
  request: PreviewHipReportRequest,
  edited_report: Option<Arc<str>>,
  desktop_uid: Option<u32>,
) -> anyhow::Result<String> {
  ensure!(request.client_version.len() <= 64, "Invalid HIP client version");
  ensure!(
    request.host_id.as_ref().is_none_or(|id| id.len() <= 256),
    "Invalid HIP host ID"
  );

  let source = &request.source;
  let execution = match source {
    HipSource::Disabled => bail!("HIP is disabled"),
    HipSource::Generated | HipSource::Edited { .. } => crate::hip_source::preview_runner(edited_report)?,
    HipSource::UserScript { .. } | HipSource::ApprovedRootScript { .. } => {
      crate::hip_source::resolve(source, desktop_uid, false, None)?
    }
  };
  let script = execution
    .wrapper
    .as_deref()
    .context("HIP preview script is unavailable")?;
  let is_runner = !matches!(source, HipSource::UserScript { .. });
  let mut command = Command::new(script);
  command
    .arg("--client-version")
    .arg(&request.client_version)
    .arg("--client-os")
    .arg(request.client_os.as_str())
    .arg("--os-version")
    .arg(request.client_os.default_os_version());
  if let Some(host_id) = request.host_id {
    command.arg("--host-id").arg(host_id);
  }
  if is_runner {
    command.arg("--preview");
  }
  command
    .arg("--cookie")
    .arg(PREVIEW_COOKIE)
    .arg("--client-ip")
    .arg(PREVIEW_IP)
    .arg("--md5")
    .arg(PREVIEW_MD5)
    .stdin(Stdio::null())
    .stdout(Stdio::piped())
    .stderr(Stdio::null());
  command.as_std_mut().process_group(0);
  configure_identity(&mut command, execution.uid, &request.client_version)?;
  command.kill_on_drop(true);

  let mut child = command.spawn().context("Failed to launch HIP preview")?;
  let mut process_group = ProcessGroupGuard::new(child.id());
  let mut stdout = child
    .stdout
    .take()
    .context("HIP preview has no output stream")?
    .take(MAX_PREVIEW_BYTES + 1);
  let output = tokio::time::timeout(PREVIEW_TIMEOUT, async {
    let mut bytes = Vec::new();
    stdout.read_to_end(&mut bytes).await?;
    ensure!(bytes.len() as u64 <= MAX_PREVIEW_BYTES, "HIP preview exceeds 1 MiB");
    let status = child.wait().await?;
    process_group.disarm();
    ensure!(status.success(), "HIP preview generator failed");
    Ok::<_, anyhow::Error>(String::from_utf8(bytes)?)
  })
  .await;
  match output {
    Ok(Ok(xml)) => {
      if matches!(
        source,
        HipSource::UserScript { .. } | HipSource::ApprovedRootScript { .. }
      ) {
        gphip::validate_edited_report(&xml).context("Custom HIP script did not emit a valid HIP report")?;
      }
      Ok(xml)
    }
    Ok(Err(error)) => {
      terminate_group(&mut child, &mut process_group).await;
      Err(error)
    }
    Err(_) => {
      terminate_group(&mut child, &mut process_group).await;
      anyhow::bail!("HIP preview timed out")
    }
  }
}

fn configure_identity(command: &mut Command, uid: u32, app_version: &str) -> anyhow::Result<()> {
  command
    .as_std_mut()
    .env_clear()
    .env("PATH", "/usr/bin:/bin:/usr/sbin:/sbin")
    .env("LC_ALL", "C")
    .env("APP_VERSION", app_version);
  if uid == 0 {
    command.as_std_mut().env("HOME", "/root");
    return Ok(());
  }
  #[cfg(any(target_os = "linux", target_os = "freebsd", target_os = "openbsd"))]
  {
    use nix::unistd::{Uid, User};
    let user = User::from_uid(Uid::from_raw(uid))?.context("Desktop user is unavailable")?;
    command
      .as_std_mut()
      .env("HOME", &user.dir)
      .env("USER", &user.name)
      .env("LOGNAME", &user.name)
      .current_dir(&user.dir);
    if Uid::effective().as_raw() == uid {
      return Ok(());
    }
    // Match OpenConnect's CSD child: a single primary group, then the desktop UID.
    unsafe {
      command.as_std_mut().pre_exec(move || {
        nix::unistd::setgid(user.gid).map_err(std::io::Error::from)?;
        nix::unistd::setgroups(&[user.gid]).map_err(std::io::Error::from)?;
        nix::unistd::setuid(Uid::from_raw(uid)).map_err(std::io::Error::from)?;
        Ok(())
      });
    }
    Ok(())
  }
  #[cfg(not(any(target_os = "linux", target_os = "freebsd", target_os = "openbsd")))]
  {
    bail!("Custom HIP previews are unavailable on this platform")
  }
}

struct ProcessGroupGuard(Option<u32>);

impl ProcessGroupGuard {
  fn new(pid: Option<u32>) -> Self {
    Self(pid)
  }

  fn kill(&mut self) {
    if let Some(pid) = self.0.take() {
      unsafe { nix::libc::kill(-(pid as i32), nix::libc::SIGKILL) };
    }
  }

  fn disarm(&mut self) {
    self.0 = None;
  }
}

impl Drop for ProcessGroupGuard {
  fn drop(&mut self) {
    self.kill();
  }
}

async fn terminate_group(child: &mut tokio::process::Child, process_group: &mut ProcessGroupGuard) {
  process_group.kill();
  let _ = child.start_kill();
  let _ = child.wait().await;
}

#[cfg(all(test, any(target_os = "linux", target_os = "freebsd", target_os = "openbsd")))]
mod tests {
  use super::*;
  use gpapi::os_profile::ClientOs;
  use std::{fs, os::unix::fs::PermissionsExt};

  fn request(path: &std::path::Path) -> PreviewHipReportRequest {
    PreviewHipReportRequest {
      source: HipSource::UserScript {
        path: path.to_string_lossy().into_owned(),
      },
      client_os: ClientOs::Linux,
      client_version: "6.3.3".into(),
      host_id: Some("host-test".into()),
    }
  }

  fn script(path: &std::path::Path, body: &str) {
    fs::write(path, format!("#!/bin/sh\n{body}\n")).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o700)).unwrap();
  }

  #[tokio::test]
  async fn user_script_preview_receives_placeholders_and_validates_output() {
    let uid = nix::unistd::Uid::effective().as_raw();
    if uid == 0 {
      return;
    }
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("hip.sh");
    script(
      &path,
      r#"
test "$1" = --client-version && test "$2" = 6.3.3 || exit 2
test "$3" = --client-os && test "$4" = Linux || exit 2
test "$5" = --os-version || exit 2
test "$7" = --host-id && test "$8" = host-test || exit 2
test "$9" = --cookie && test "${10}" = 'user=preview&domain=preview&computer=preview' || exit 2
test "${11}" = --client-ip && test "${12}" = 192.0.2.1 || exit 2
test "${13}" = --md5 && test "${14}" = 00000000000000000000000000000000 || exit 2
test "$#" = 14 || exit 2
test "$APP_VERSION" = 6.3.3 || exit 2
printf '%s' '<hip-report><md5-sum/><user-name/><domain/><host-name/><host-id/><ip-address/><ipv6-address/><generate-time/><hip-report-version/><categories><entry name="host-info"><client-version/><os/><os-vendor/><domain/><host-name/><host-id/></entry></categories></hip-report>'
"#,
    );
    let xml = generate(request(&path), None, Some(uid)).await.unwrap();
    assert!(xml.starts_with("<hip-report>"));

    script(&path, "printf '%s' '<not-hip/>'");
    assert!(generate(request(&path), None, Some(uid)).await.is_err());

    script(&path, "head -c 1048577 /dev/zero");
    let err = generate(request(&path), None, Some(uid)).await.unwrap_err();
    assert!(err.to_string().contains("exceeds 1 MiB"));
  }

  #[cfg(target_os = "linux")]
  #[tokio::test]
  async fn cancelling_preview_stops_its_process_group() {
    let uid = nix::unistd::Uid::effective().as_raw();
    if uid == 0 {
      return;
    }
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("hip.sh");
    let pid_file = dir.path().join("pids");
    script(
      &path,
      &format!(
        "sleep 30 & child=$!; echo \"$$ $child\" > \"{}\"; wait",
        pid_file.display()
      ),
    );
    let preview = tokio::spawn(generate(request(&path), None, Some(uid)));
    let pids = tokio::time::timeout(Duration::from_secs(3), async {
      loop {
        if let Ok(pids) = fs::read_to_string(&pid_file) {
          break pids;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
      }
    })
    .await
    .unwrap();
    let child_pid = pids.split_whitespace().nth(1).unwrap().parse::<u32>().unwrap();
    preview.abort();
    assert!(preview.await.unwrap_err().is_cancelled());
    tokio::time::timeout(Duration::from_secs(3), async {
      loop {
        let state = fs::read_to_string(format!("/proc/{child_pid}/stat"))
          .ok()
          .and_then(|stat| stat.rsplit_once(") ").and_then(|(_, fields)| fields.chars().next()));
        if state.is_none_or(|state| state == 'Z') {
          break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
      }
    })
    .await
    .unwrap();
  }
}
