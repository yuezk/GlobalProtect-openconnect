use std::{io, sync::Arc};

use anyhow::{Context, ensure};
use gpapi::{
  hip::HipSource,
  os_profile::{ClientOs, HostIdentity, OsProfile, OsProfileBuilder},
};
use gphip::{ReportContext, ReportInput};
use openconnect::HipRequest;
#[cfg(any(target_os = "linux", target_os = "freebsd", target_os = "openbsd"))]
use openconnect::HipScript;
#[cfg(unix)]
#[path = "hip_source/unix.rs"]
mod scripts;
#[cfg(windows)]
#[path = "hip_source/windows.rs"]
mod scripts;
#[cfg(any(target_os = "linux", target_os = "freebsd", target_os = "openbsd"))]
use scripts::script_environment;

pub(crate) struct HipExecution {
  pub source: openconnect::HipSource,
  approval: HipApproval,
}

impl HipExecution {
  pub(crate) fn into_parts(self) -> (openconnect::HipSource, HipApproval) {
    (self.source, self.approval)
  }
}

pub(crate) struct HipApproval {
  identity: Option<(String, u32)>,
}

impl HipApproval {
  pub(crate) fn approval_is_valid(&self) -> bool {
    #[cfg(any(target_os = "linux", target_os = "freebsd", target_os = "openbsd"))]
    if let Some((id, uid)) = &self.identity {
      return gpservice::hip_approval::resolve(id, *uid).is_ok();
    }
    self.identity.is_none()
  }
}

pub(crate) fn resolve(
  source: &HipSource,
  desktop_uid: Option<u32>,
  restricted_desktop: bool,
  edited_report: Option<Arc<str>>,
  profile: Option<OsProfile>,
) -> anyhow::Result<HipExecution> {
  let approval;
  let source = match source {
    HipSource::Disabled => {
      approval = None;
      openconnect::HipSource::Disabled
    }
    HipSource::Generated | HipSource::Edited { .. } => {
      let profile = profile.context("HIP generator identity is unavailable")?;
      approval = None;
      let xml = match source {
        HipSource::Edited { .. } => {
          let xml = edited_report.context("Edited HIP report was not uploaded for this connection")?;
          gphip::validate_edited_report(&xml)?;
          Some(xml)
        }
        _ => None,
      };
      openconnect::HipSource::Generator(Arc::new(move |request, control| {
        generate(&profile, xml.as_deref(), request, &|| control.check(), false).map_err(generation_error)
      }))
    }
    HipSource::UserScript { path } => {
      approval = None;
      ensure_custom_available(restricted_desktop)?;
      scripts::user_script(path, desktop_uid)?
    }
    HipSource::ApprovedRootScript { approval_id } => {
      ensure_custom_available(restricted_desktop)?;
      #[cfg(any(target_os = "linux", target_os = "freebsd", target_os = "openbsd"))]
      {
        let owner_uid = desktop_uid.context("Root HIP scripts require a verified desktop user")?;
        let approved = gpservice::hip_approval::resolve(approval_id, owner_uid)?;
        let id = approval_id.clone();
        let path = approved.path;
        let expected = path.clone();
        let script = HipScript::new(path.to_string_lossy().into_owned(), Some(0))?
          .with_environment(script_environment(0)?)?
          .with_validator(move |control| {
            control.check()?;
            let current = gpservice::hip_approval::resolve_with_control(&id, owner_uid, &|| control.check())
              .map_err(generation_error)?;
            if current.path != expected {
              return Err(io::Error::other("Approved HIP executable changed"));
            }
            control.check()
          });
        approval = Some((approval_id.clone(), owner_uid));
        openconnect::HipSource::Script(script)
      }
      #[cfg(not(any(target_os = "linux", target_os = "freebsd", target_os = "openbsd")))]
      {
        let _ = (approval_id, desktop_uid);
        anyhow::bail!("Root HIP script approvals are unavailable on this platform")
      }
    }
  };
  Ok(HipExecution {
    source,
    approval: HipApproval { identity: approval },
  })
}

fn ensure_custom_available(restricted_desktop: bool) -> anyhow::Result<()> {
  ensure!(
    !cfg!(target_os = "macos") && !restricted_desktop,
    "Custom HIP scripts are unavailable on this platform"
  );
  Ok(())
}

pub(crate) fn generate(
  profile: &OsProfile,
  edited: Option<&str>,
  request: &HipRequest,
  control: &dyn gpapi::collection::CollectionControl,
  preview: bool,
) -> anyhow::Result<String> {
  control.check()?;
  let context = if preview {
    ReportContext::Preview
  } else {
    ReportContext::Connected {
      cookie: request.cookie.clone(),
      client_ip: request.client_ip.clone(),
      client_ipv6: request.client_ipv6.clone(),
      md5: request.md5.clone(),
    }
  };
  let input = ReportInput {
    profile: profile.clone(),
    context,
  };
  match edited {
    Some(xml) => gphip::refresh_edited_report_with_control(xml, &input, control),
    None => gphip::generate_report_with_control(&input, control),
  }
}

pub(crate) fn profile(
  identity: &HostIdentity,
  os: ClientOs,
  version: Option<String>,
  os_version: Option<String>,
  host_id: Option<String>,
  hostname: Option<String>,
  control: &dyn gpapi::collection::CollectionControl,
) -> io::Result<OsProfile> {
  let identity = gpapi::os_profile::desktop::identity_from_parts(
    hostname.as_deref().unwrap_or(identity.computer()).into(),
    host_id.as_deref().unwrap_or(identity.host_id()).into(),
    identity.serialno().into(),
    identity.mac_addr().into(),
  );
  let mut builder = OsProfileBuilder::new(os).host_identity(identity);
  if let Some(hostname) = hostname {
    builder = builder.computer_name_override(hostname);
  }
  if let Some(version) = version {
    builder = builder.client_version(version);
  }
  if let Some(os_version) = os_version {
    builder = builder.os_version(os_version);
  }
  gpapi::os_profile::desktop::collect_profile_with_control(builder, host_id.as_deref(), control)
}

fn generation_error(error: anyhow::Error) -> io::Error {
  match error.downcast::<io::Error>() {
    Ok(error) => error,
    Err(error) => io::Error::other(error),
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use gpapi::os_profile::ClientOs;

  fn fixture_profile() -> OsProfile {
    profile(
      &gpapi::os_profile::desktop::identity_from_parts(
        "device".into(),
        "host".into(),
        "serial".into(),
        "00:11:22:33:44:55".into(),
      ),
      ClientOs::Linux,
      Some("6.3.3".into()),
      Some("negotiated-version".into()),
      None,
      None,
      &gpapi::collection::CollectionBudget::new(std::time::Duration::from_secs(60)),
    )
    .unwrap()
  }

  #[test]
  fn callback_configuration_needs_no_files_or_root_identity() {
    let generated = resolve(&HipSource::Generated, None, true, None, Some(fixture_profile())).unwrap();
    assert!(matches!(generated.source, openconnect::HipSource::Generator(_)));
    let disabled = resolve(&HipSource::Disabled, None, true, None, Some(fixture_profile())).unwrap();
    assert!(matches!(disabled.source, openconnect::HipSource::Disabled));
  }

  #[test]
  fn profile_retains_negotiated_identity_and_os_version() {
    let identity = gpapi::os_profile::desktop::identity_from_parts(
      "original".into(),
      "original-id".into(),
      "serial".into(),
      "00:11:22:33:44:55".into(),
    );
    let profile = profile(
      &identity,
      ClientOs::Mac,
      Some("6.3.3".into()),
      Some("negotiated-os".into()),
      Some("negotiated-host".into()),
      Some("negotiated-name".into()),
      &gpapi::collection::CollectionBudget::new(std::time::Duration::from_secs(60)),
    )
    .unwrap();
    assert_eq!(profile.os_version(), "negotiated-os");
    assert_eq!(profile.host_identity().host_id(), "negotiated-host");
    assert_eq!(profile.host_identity().computer(), "negotiated-name");
    assert_eq!(profile.host_identity().serialno(), "serial");
  }

  #[cfg(windows)]
  #[test]
  fn native_windows_profile_retains_negotiated_host_id() {
    let identity = gpapi::os_profile::HostIdentity::from_parts(
      "device".into(),
      "hardware-seed".into(),
      "serial".into(),
      "00:11:22:33:44:55".into(),
    );
    let profile = profile(
      &identity,
      ClientOs::Windows,
      None,
      None,
      Some("negotiated-host".into()),
      None,
      &gpapi::collection::CollectionBudget::new(std::time::Duration::from_secs(60)),
    )
    .unwrap();
    assert_eq!(profile.host_id(), "negotiated-host");
    assert_eq!(profile.host_identity().host_id(), "negotiated-host");
  }

  #[test]
  fn callback_errors_preserve_cancellation() {
    let error = generation_error(io::Error::new(io::ErrorKind::Interrupted, "cancelled").into());
    assert_eq!(error.kind(), io::ErrorKind::Interrupted);
  }

  #[test]
  fn custom_source_cannot_run_on_macos_or_choose_root_without_approval() {
    let source = HipSource::UserScript {
      path: "/tmp/hip.sh".into(),
    };
    assert!(resolve(&source, None, false, None, None).is_err());
    assert!(resolve(&source, Some(0), false, None, None).is_err());
    assert!(resolve(&source, Some(1000), true, None, Some(fixture_profile())).is_err());
    #[cfg(target_os = "macos")]
    assert!(resolve(&source, Some(1000), false, None, None).is_err());
  }
}
