use crate::{HipControl, HipRequest, HipScript, HipSource, Vpn};
use std::{
  ffi::{CString, c_char, c_int, c_void},
  io,
  marker::PhantomData,
  panic::{AssertUnwindSafe, catch_unwind},
};
#[link(name = "vpn")]
unsafe extern "C" {
  fn vpn_collect_hip_report(
    script: *const HipScriptRaw,
    request: *const HipRequestRaw,
    control: *const HipControlRaw,
    generate: Option<HipGenerateFn>,
    data: *mut c_void,
    output: *mut c_char,
    capacity: usize,
    written: *mut usize,
  ) -> c_int;
}
#[repr(C)]
pub(crate) struct HipRequestRaw {
  cookie: *const c_char,
  client_ip: *const c_char,
  client_ipv6: *const c_char,
  md5: *const c_char,
  client_version: *const c_char,
  client_os: *const c_char,
  os_version: *const c_char,
  host_id: *const c_char,
  local_hostname: *const c_char,
}
#[repr(C)]
pub(crate) struct HipControlRaw {
  pub data: *mut c_void,
  pub check: unsafe extern "C" fn(*mut c_void) -> c_int,
}
pub(crate) type HipGenerateFn =
  extern "C" fn(*mut c_void, *const HipRequestRaw, *const HipControlRaw, *mut c_char, usize, *mut usize) -> c_int;
#[repr(C)]
#[derive(Debug)]
pub(crate) struct HipScriptRaw {
  pub path: *const c_char,
  pub uid: u32,
  pub uid_present: c_int,
  pub validation_data: *mut c_void,
  pub validate: Option<extern "C" fn(*mut c_void, *const HipControlRaw) -> c_int>,
  pub environment: *const *const c_char,
  pub cwd: *const c_char,
}
impl Default for HipScriptRaw {
  fn default() -> Self {
    Self {
      path: std::ptr::null(),
      uid: 0,
      uid_present: 0,
      validation_data: std::ptr::null_mut(),
      validate: None,
      environment: std::ptr::null(),
      cwd: std::ptr::null(),
    }
  }
}
impl HipScriptRaw {
  pub fn from_script(script: &HipScript) -> Self {
    Self {
      path: script.path.as_ptr(),
      uid: script.user.unwrap_or(0),
      uid_present: script.user.is_some().into(),
      validation_data: script as *const _ as *mut _,
      validate: script.validator.as_ref().map(|_| validate_hip_script as _),
      environment: std::ptr::null(),
      cwd: script.cwd.as_ref().map_or(std::ptr::null(), |value| value.as_ptr()),
    }
  }
}
pub(crate) fn script_environment(script: &HipScript) -> Vec<*const c_char> {
  match &script.environment {
    Some(values) => values
      .iter()
      .map(|value| value.as_ptr())
      .chain(std::iter::once(std::ptr::null()))
      .collect(),
    None => Vec::new(),
  }
}
fn error_code(error: io::Error) -> c_int {
  let errno = error.raw_os_error().unwrap_or_else(|| match error.kind() {
    io::ErrorKind::Interrupted => libc::EINTR,
    io::ErrorKind::TimedOut => libc::ETIMEDOUT,
    io::ErrorKind::InvalidInput | io::ErrorKind::InvalidData => libc::EINVAL,
    io::ErrorKind::PermissionDenied => libc::EPERM,
    io::ErrorKind::NotFound => libc::ENOENT,
    _ => libc::EIO,
  });
  -errno
}

unsafe fn request_string(pointer: *const c_char) -> io::Result<Option<String>> {
  if pointer.is_null() {
    return Ok(None);
  }
  unsafe { std::ffi::CStr::from_ptr(pointer) }
    .to_str()
    .map(|s| Some(s.to_owned()))
    .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "Invalid HIP request encoding"))
}
unsafe fn read_hip_request(raw: &HipRequestRaw) -> io::Result<HipRequest> {
  unsafe {
    Ok(HipRequest {
      cookie: request_string(raw.cookie)?.unwrap_or_default(),
      client_ip: request_string(raw.client_ip)?,
      client_ipv6: request_string(raw.client_ipv6)?,
      md5: request_string(raw.md5)?.unwrap_or_default(),
      client_version: request_string(raw.client_version)?.unwrap_or_default(),
      client_os: request_string(raw.client_os)?.unwrap_or_default(),
      os_version: request_string(raw.os_version)?.unwrap_or_default(),
      host_id: request_string(raw.host_id)?,
      local_hostname: request_string(raw.local_hostname)?,
    })
  }
}
pub(crate) extern "C" fn generate_hip_report(
  data: *mut c_void,
  request: *const HipRequestRaw,
  control: *const HipControlRaw,
  output: *mut c_char,
  capacity: usize,
  written: *mut usize,
) -> c_int {
  let result = catch_unwind(AssertUnwindSafe(|| -> io::Result<()> {
    let vpn = unsafe { &*(data as *const Vpn) };
    let HipSource::Generator(generate) = &vpn.hip_source else {
      return Err(io::Error::new(io::ErrorKind::InvalidInput, "No HIP generator"));
    };
    let request = unsafe { read_hip_request(&*request)? };
    let control = HipControl {
      raw: unsafe { &*control },
      _thread: PhantomData,
    };
    control.check()?;
    let report = generate(&request, &control)?;
    control.check()?;
    if report.len() > capacity {
      return Err(io::Error::from_raw_os_error(libc::E2BIG));
    }
    unsafe {
      std::ptr::copy_nonoverlapping(report.as_ptr(), output.cast(), report.len());
      *written = report.len();
    }
    Ok(())
  }));
  match result {
    Ok(Ok(())) => 0,
    Ok(Err(error)) => error_code(error),
    Err(_) => -libc::EIO,
  }
}
extern "C" fn validate_hip_script(data: *mut c_void, control: *const HipControlRaw) -> c_int {
  let result = catch_unwind(AssertUnwindSafe(|| {
    let script = unsafe { &*(data as *const HipScript) };
    let control = HipControl {
      raw: unsafe { &*control },
      _thread: PhantomData,
    };
    control.check()?;
    if let Some(validate) = &script.validator {
      validate(&control)?;
    }
    control.check()
  }));
  match result {
    Ok(Ok(())) => 0,
    Ok(Err(error)) => error_code(error),
    Err(_) => -libc::EIO,
  }
}
pub(crate) fn preview_hip_script(
  script: &HipScript,
  request: &HipRequest,
  check: &dyn Fn() -> io::Result<()>,
) -> io::Result<String> {
  let mut raw = HipScriptRaw::from_script(script);
  let environment = script_environment(script);
  if !environment.is_empty() {
    raw.environment = environment.as_ptr();
  }
  collect_hip_report(&raw, None, std::ptr::null_mut(), request, check)
}

fn collect_hip_report(
  script_raw: &HipScriptRaw,
  generate: Option<HipGenerateFn>,
  data: *mut c_void,
  request: &HipRequest,
  check: &dyn Fn() -> io::Result<()>,
) -> io::Result<String> {
  let values = [
    Some(request.cookie.as_str()),
    request.client_ip.as_deref(),
    request.client_ipv6.as_deref(),
    Some(request.md5.as_str()),
    Some(request.client_version.as_str()),
    Some(request.client_os.as_str()),
    Some(request.os_version.as_str()),
    request.host_id.as_deref(),
    request.local_hostname.as_deref(),
  ]
  .into_iter()
  .map(|value| value.map(CString::new).transpose())
  .collect::<Result<Vec<_>, _>>()
  .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "HIP request contains NUL"))?;
  let ptr = |i: usize| values[i].as_ref().map_or(std::ptr::null(), |value| value.as_ptr());
  let raw = HipRequestRaw {
    cookie: ptr(0),
    client_ip: ptr(1),
    client_ipv6: ptr(2),
    md5: ptr(3),
    client_version: ptr(4),
    client_os: ptr(5),
    os_version: ptr(6),
    host_id: ptr(7),
    local_hostname: ptr(8),
  };
  let control = HipControlRaw {
    data: &check as *const _ as *mut _,
    check: preview_check,
  };
  let mut output = vec![0u8; 1024 * 1024 + 1];
  let mut written = 0;
  let result = unsafe {
    vpn_collect_hip_report(
      script_raw,
      &raw,
      &control,
      generate,
      data,
      output.as_mut_ptr().cast(),
      1024 * 1024,
      &mut written,
    )
  };
  if result < 0 {
    return Err(io::Error::from_raw_os_error(-result));
  }
  output.truncate(written);
  String::from_utf8(output).map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "HIP output is not UTF-8"))
}
unsafe extern "C" fn preview_check(data: *mut c_void) -> c_int {
  let check = unsafe { &*(data as *const &dyn Fn() -> io::Result<()>) };
  match catch_unwind(AssertUnwindSafe(check)) {
    Ok(Ok(())) => 0,
    Ok(Err(error)) => error_code(error),
    Err(_) => -libc::EIO,
  }
}

#[cfg(test)]
mod hip_tests {
  use super::*;
  use std::{
    fs,
    os::unix::fs::PermissionsExt,
    sync::atomic::{AtomicUsize, Ordering},
    time::{Duration, Instant},
  };
  fn provider(generate: impl Fn(&HipRequest, &HipControl<'_>) -> io::Result<String> + Send + Sync + 'static) -> Vpn {
    Vpn::builder("vpn.example", "cookie")
      .script_path("/bin/sh".to_owned())
      .hip_source(HipSource::Generator(std::sync::Arc::new(generate)))
      .build()
      .unwrap()
  }
  fn collect(vpn: &Vpn, script: &HipScriptRaw, check: &dyn Fn() -> io::Result<()>) -> io::Result<String> {
    collect_hip_report(
      script,
      Some(generate_hip_report),
      vpn as *const _ as *mut _,
      &HipRequest::default(),
      check,
    )
  }
  struct ScriptFixture(std::path::PathBuf);
  impl ScriptFixture {
    fn new(contents: &str) -> Self {
      static NEXT: AtomicUsize = AtomicUsize::new(0);
      let directory = std::env::temp_dir().join(format!(
        "gp-hip-script-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
      ));
      fs::create_dir(&directory).unwrap();
      let path = directory.join("script");
      fs::write(&path, format!("#!/bin/sh\n{contents}\n")).unwrap();
      fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).unwrap();
      Self(directory)
    }
    fn script(&self, user: Option<u32>) -> HipScript {
      HipScript::new(self.0.join("script").to_str().unwrap().to_owned(), user).unwrap()
    }
  }
  impl Drop for ScriptFixture {
    fn drop(&mut self) {
      let _ = fs::remove_dir_all(&self.0);
    }
  }

  #[test]
  fn callback_outputs_are_bounded_and_validated_by_c() {
    let raw = HipScriptRaw::default();
    for (report, errno) in [
      (String::new(), libc::EINVAL),
      ("x\0x".to_owned(), libc::EINVAL),
      ("x".repeat(1024 * 1024 + 1), libc::E2BIG),
    ] {
      let vpn = provider(move |_, _| Ok(report.clone()));
      assert_eq!(collect(&vpn, &raw, &|| Ok(())).unwrap_err().raw_os_error(), Some(errno));
    }
    let vpn = provider(|_, _| Ok("x".repeat(1024 * 1024)));
    assert_eq!(collect(&vpn, &raw, &|| Ok(())).unwrap().len(), 1024 * 1024);
  }
  #[test]
  fn callback_panics_and_control_errors_do_not_cross_ffi() {
    let vpn = provider(|_, _| panic!("test generator panic"));
    assert_eq!(
      collect(&vpn, &Default::default(), &|| Ok(()))
        .unwrap_err()
        .raw_os_error(),
      Some(libc::EIO)
    );
    let vpn = provider(|_, _| Err(io::Error::from(io::ErrorKind::TimedOut)));
    assert_eq!(
      collect(&vpn, &Default::default(), &|| Ok(()))
        .unwrap_err()
        .raw_os_error(),
      Some(libc::ETIMEDOUT)
    );
    let vpn = provider(|_, _| panic!("Canceled provider must not run"));
    assert_eq!(
      collect(&vpn, &Default::default(), &|| Err(io::Error::from(
        io::ErrorKind::Interrupted
      )))
      .unwrap_err()
      .raw_os_error(),
      Some(libc::EINTR)
    );
  }
  #[test]
  fn explicit_script_wins_and_uid_presence_is_preserved() {
    let fixture = ScriptFixture::new("printf '<script/>'");
    let script = fixture.script(None);
    let raw = HipScriptRaw::from_script(&script);
    assert_eq!(raw.uid_present, 0);
    let vpn = provider(|_, _| panic!("Script must win"));
    assert_eq!(collect(&vpn, &raw, &|| Ok(())).unwrap(), "<script/>");
    let script = fixture.script(Some(unsafe { libc::getuid() }));
    assert_eq!(HipScriptRaw::from_script(&script).uid_present, 1);
    assert_eq!(script.preview(&HipRequest::default(), &|| Ok(())).unwrap(), "<script/>");
  }
  #[test]
  fn validator_refusal_prevents_execution_and_errors_propagate() {
    let fixture = ScriptFixture::new("touch executed; printf '<script/>'");
    let script = fixture
      .script(None)
      .with_validator(|_| Err(io::Error::from(io::ErrorKind::PermissionDenied)));
    assert_eq!(
      script
        .preview(&HipRequest::default(), &|| Ok(()))
        .unwrap_err()
        .raw_os_error(),
      Some(libc::EPERM)
    );
    assert!(!fixture.0.join("executed").exists());
    let script = fixture.script(None).with_validator(|_| panic!("test validator panic"));
    assert_eq!(
      script
        .preview(&HipRequest::default(), &|| Ok(()))
        .unwrap_err()
        .raw_os_error(),
      Some(libc::EIO)
    );
  }
  #[test]
  fn child_environment_and_cwd_are_explicit_without_parent_mutation() {
    let fixture = ScriptFixture::new("printf '%s:%s' \"$HIP_TEST_VALUE\" \"$PWD\"");
    let cwd = fs::canonicalize(&fixture.0).unwrap().to_str().unwrap().to_owned();
    let script = fixture
      .script(None)
      .with_environment(crate::HipScriptEnvironment {
        variables: vec![("HIP_TEST_VALUE".into(), "child".into())],
        cwd: cwd.clone(),
      })
      .unwrap();
    assert_eq!(
      script.preview(&HipRequest::default(), &|| Ok(())).unwrap(),
      format!("child:{cwd}")
    );
    assert!(std::env::var_os("HIP_TEST_VALUE").is_none());
  }
  #[test]
  fn script_empty_nul_and_nonzero_outputs_fail() {
    for contents in ["true", "printf '\\000'", "printf '<script/>'; exit 1"] {
      let fixture = ScriptFixture::new(contents);
      assert_eq!(
        fixture
          .script(None)
          .preview(&HipRequest::default(), &|| Ok(()))
          .unwrap_err()
          .raw_os_error(),
        Some(libc::EINVAL)
      );
    }
  }
  #[test]
  fn cancellation_and_deadline_kill_descendants_and_reap_child() {
    for errno in [libc::EINTR, libc::ETIMEDOUT] {
      let fixture = ScriptFixture::new("sleep 20 &\nwait\nprintf '<script/>'");
      let start = Instant::now();
      let check = || {
        if start.elapsed() > Duration::from_millis(80) {
          Err(io::Error::from_raw_os_error(errno))
        } else {
          Ok(())
        }
      };
      assert_eq!(
        fixture
          .script(None)
          .preview(&HipRequest::default(), &check)
          .unwrap_err()
          .raw_os_error(),
        Some(errno)
      );
      assert!(start.elapsed() < Duration::from_secs(2));
    }
  }
}
