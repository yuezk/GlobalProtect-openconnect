use std::{
  ffi::{CStr, CString, c_char},
  fmt, io,
  sync::{Arc, Mutex, RwLock},
};

use log::{info, warn};

use crate::vpn_utils::{check_executable, find_vpnc_script};
use crate::{HipSource, ffi};

type OnConnectedCallback = Arc<RwLock<Option<Box<dyn FnOnce(VpnSessionInfo) + 'static + Send + Sync>>>>;
type OnHipReportCallback = RwLock<Option<Arc<dyn Fn(&str) + Send + Sync>>>;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct VpnSessionInfo {
  pub lifetime_secs: Option<u32>,
  pub user_expires: Option<u32>,
  pub lifetime_warning: Option<VpnSessionWarning>,
  pub nlb_enabled: bool,
  pub nlb_connected_gw_ip: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VpnSessionWarning {
  pub prior_secs: u32,
  pub message: String,
}

pub(crate) fn session_info_from_raw(raw: *const ffi::VpnSessionInfoRaw) -> VpnSessionInfo {
  if raw.is_null() {
    return VpnSessionInfo::default();
  }

  let raw = unsafe { &*raw };
  let warning_message =
    unsafe { optional_c_string(raw.lifetime_warning_message) }.filter(|message| !message.is_empty());
  let user_expires = positive_i64_to_u32(raw.user_expires).or_else(|| positive_i64_to_u32(raw.auth_expiration));

  VpnSessionInfo {
    lifetime_secs: positive_i64_to_u32(raw.lifetime_secs as i64),
    user_expires,
    lifetime_warning: match (positive_i64_to_u32(raw.lifetime_warning_prior as i64), warning_message) {
      (Some(prior_secs), Some(message)) => Some(VpnSessionWarning { prior_secs, message }),
      _ => None,
    },
    nlb_enabled: raw.nlb_enabled != 0,
    nlb_connected_gw_ip: unsafe { optional_c_string(raw.nlb_connected_gw_ip) },
  }
}

unsafe fn optional_c_string(value: *const c_char) -> Option<String> {
  if value.is_null() {
    return None;
  }

  unsafe { CStr::from_ptr(value) }.to_str().ok().map(ToOwned::to_owned)
}

fn positive_i64_to_u32(value: i64) -> Option<u32> {
  u32::try_from(value).ok().filter(|value| *value > 0)
}

// The descriptor is borrowed only between attach and detach. Serialize writes
// against teardown and retain cancellation before the pipe exists.
#[derive(Default)]
struct Cancellation {
  state: Mutex<CancellationState>,
}

#[derive(Default)]
struct CancellationState {
  requested: bool,
  command_fd: Option<i32>,
}

impl Cancellation {
  fn attach(&self, fd: i32) -> bool {
    let mut state = self.state.lock().unwrap();
    state.command_fd = Some(fd);
    state.requested
  }

  fn detach(&self) {
    self.state.lock().unwrap().command_fd = None;
  }

  fn cancel(&self) -> io::Result<()> {
    let mut state = self.state.lock().unwrap();
    if state.requested {
      return Ok(());
    }
    state.requested = true;
    if let Some(fd) = state.command_fd {
      ffi::write_cancel(fd)?;
    }
    Ok(())
  }
}

pub struct Vpn {
  cancellation: Cancellation,
  server: CString,
  cookie: CString,

  user_agent: CString,
  os: CString,
  os_version: Option<CString>,
  client_version: Option<CString>,
  host_id: Option<CString>,
  local_hostname: Option<CString>,

  script: CString,
  interface: Option<CString>,
  script_tun: bool,

  certificate: Option<CString>,
  sslkey: Option<CString>,
  key_password: Option<CString>,
  servercert: Option<CString>,

  pub(crate) hip_source: HipSource,

  reconnect_timeout: u32,
  mtu: u32,
  disable_ipv6: bool,
  no_dtls: bool,

  dpd_interval: u32,
  no_xmlpost: bool,

  callback: OnConnectedCallback,
  hip_report_callback: OnHipReportCallback,
}

impl Vpn {
  pub fn builder(server: &str, cookie: &str) -> VpnBuilder {
    VpnBuilder::new(server, cookie)
  }

  pub fn connect(&self, on_connected: impl FnOnce(VpnSessionInfo) + 'static + Send + Sync) -> i32 {
    self.callback.write().unwrap().replace(Box::new(on_connected));
    let mut options = self.build_connect_options();
    let environment = match &self.hip_source {
      HipSource::Script(script) => ffi::script_environment(script),
      _ => Vec::new(),
    };
    if !environment.is_empty() {
      options.hip_script.environment = environment.as_ptr();
    }
    ffi::connect(&options)
  }

  /// Receives a borrowed report after the gateway accepts its HIP submission.
  /// The callback must copy the XML if it needs to retain it.
  pub fn set_hip_report_callback(&self, callback: impl Fn(&str) + Send + Sync + 'static) {
    *self.hip_report_callback.write().unwrap() = Some(Arc::new(callback));
  }

  pub(crate) fn on_hip_report_submitted(&self, report: &str) {
    let callback = self.hip_report_callback.read().unwrap().clone();
    if let Some(callback) = callback {
      callback(report);
    }
  }

  pub(crate) fn on_connected(&self, pipe_fd: i32, session_info: VpnSessionInfo) {
    info!("Connected to VPN, pipe_fd: {}", pipe_fd);

    if let Some(callback) = self.callback.write().unwrap().take() {
      callback(session_info);
    }
  }

  pub(crate) fn attach_command_pipe(&self, fd: i32) -> bool {
    self.cancellation.attach(fd)
  }

  pub(crate) fn detach_command_pipe(&self) {
    self.cancellation.detach();
  }

  pub fn disconnect(&self) {
    if let Err(error) = self.cancellation.cancel() {
      warn!("Failed to signal VPN cancellation: {error}");
    }
  }

  fn build_connect_options(&self) -> ffi::ConnectOptions {
    ffi::ConnectOptions {
      user_data: self as *const _ as *mut _,
      on_hip_report_submitted: Some(ffi::on_hip_report_submitted),

      server: self.server.as_ptr(),
      cookie: self.cookie.as_ptr(),

      user_agent: self.user_agent.as_ptr(),
      os: self.os.as_ptr(),
      os_version: Self::option_to_ptr(&self.os_version),
      client_version: Self::option_to_ptr(&self.client_version),
      host_id: Self::option_to_ptr(&self.host_id),
      local_hostname: Self::option_to_ptr(&self.local_hostname),

      script: self.script.as_ptr(),
      interface: Self::option_to_ptr(&self.interface),
      script_tun: self.script_tun as u32,

      certificate: Self::option_to_ptr(&self.certificate),
      sslkey: Self::option_to_ptr(&self.sslkey),
      key_password: Self::option_to_ptr(&self.key_password),
      servercert: Self::option_to_ptr(&self.servercert),

      hip_script: match &self.hip_source {
        HipSource::Script(script) => ffi::HipScriptRaw::from_script(script),
        _ => Default::default(),
      },
      generate_hip: match &self.hip_source {
        HipSource::Generator(_) => Some(ffi::generate_hip_report),
        _ => None,
      },

      reconnect_timeout: self.reconnect_timeout,
      mtu: self.mtu,
      disable_ipv6: self.disable_ipv6 as u32,
      no_dtls: self.no_dtls as u32,
      dpd_interval: self.dpd_interval,
      no_xmlpost: self.no_xmlpost as u32,
    }
  }

  fn option_to_ptr(option: &Option<CString>) -> *const c_char {
    match option {
      Some(value) => value.as_ptr(),
      None => std::ptr::null(),
    }
  }
}

#[derive(Debug)]
pub struct VpnError {
  message: String,
}

impl VpnError {
  fn new(message: String) -> Self {
    Self { message }
  }
}

impl fmt::Display for VpnError {
  fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
    write!(f, "{}", self.message)
  }
}

impl std::error::Error for VpnError {}

pub struct VpnBuilder {
  server: String,
  cookie: String,
  script: Option<String>,
  script_is_path: bool,
  interface: Option<String>,
  script_tun: bool,

  user_agent: Option<String>,
  os: Option<String>,
  os_version: Option<String>,
  client_version: Option<String>,
  host_id: Option<String>,
  local_hostname: Option<String>,

  certificate: Option<String>,
  sslkey: Option<String>,
  key_password: Option<String>,

  hip_source: HipSource,

  reconnect_timeout: u32,
  mtu: u32,
  disable_ipv6: bool,
  no_dtls: bool,

  dpd_interval: u32,
  no_xmlpost: bool,
}

impl VpnBuilder {
  fn new(server: &str, cookie: &str) -> Self {
    Self {
      server: server.to_string(),
      cookie: cookie.to_string(),
      script: None,
      script_is_path: false,
      interface: None,
      script_tun: false,

      user_agent: None,
      os: None,
      os_version: None,
      client_version: None,
      host_id: None,
      local_hostname: None,

      certificate: None,
      sslkey: None,
      key_password: None,

      hip_source: HipSource::Disabled,

      reconnect_timeout: 300,
      mtu: 0,
      disable_ipv6: false,
      no_dtls: false,
      dpd_interval: 0,
      no_xmlpost: false,
    }
  }

  pub fn script<T: Into<Option<String>>>(mut self, script: T) -> Self {
    self.script = script.into();
    self.script_is_path = false;
    self
  }

  pub fn script_path<T: Into<Option<String>>>(mut self, script: T) -> Self {
    self.script = script.into();
    self.script_is_path = true;
    self
  }

  pub fn interface<T: Into<Option<String>>>(mut self, interface: T) -> Self {
    self.interface = interface.into();
    self
  }

  pub fn script_tun(mut self, script_tun: bool) -> Self {
    self.script_tun = script_tun;
    self
  }

  pub fn user_agent<T: Into<Option<String>>>(mut self, user_agent: T) -> Self {
    self.user_agent = user_agent.into();
    self
  }

  pub fn os<T: Into<Option<String>>>(mut self, os: T) -> Self {
    self.os = os.into();
    self
  }

  pub fn os_version<T: Into<Option<String>>>(mut self, os_version: T) -> Self {
    self.os_version = os_version.into();
    self
  }

  pub fn client_version<T: Into<Option<String>>>(mut self, client_version: T) -> Self {
    self.client_version = client_version.into();
    self
  }

  pub fn host_id<T: Into<Option<String>>>(mut self, host_id: T) -> Self {
    self.host_id = host_id.into();
    self
  }

  pub fn local_hostname<T: Into<Option<String>>>(mut self, local_hostname: T) -> Self {
    self.local_hostname = local_hostname.into();
    self
  }

  pub fn certificate<T: Into<Option<String>>>(mut self, certificate: T) -> Self {
    self.certificate = certificate.into();
    self
  }

  pub fn sslkey<T: Into<Option<String>>>(mut self, sslkey: T) -> Self {
    self.sslkey = sslkey.into();
    self
  }

  pub fn key_password<T: Into<Option<String>>>(mut self, key_password: T) -> Self {
    self.key_password = key_password.into();
    self
  }

  pub fn hip_source(mut self, hip_source: HipSource) -> Self {
    self.hip_source = hip_source;
    self
  }

  pub fn reconnect_timeout(mut self, reconnect_timeout: u32) -> Self {
    self.reconnect_timeout = reconnect_timeout;
    self
  }

  pub fn mtu(mut self, mtu: u32) -> Self {
    self.mtu = mtu;
    self
  }

  pub fn disable_ipv6(mut self, disable_ipv6: bool) -> Self {
    self.disable_ipv6 = disable_ipv6;
    self
  }

  pub fn no_dtls(mut self, no_dtls: bool) -> Self {
    self.no_dtls = no_dtls;
    self
  }

  pub fn dpd_interval(mut self, dpd_interval: u32) -> Self {
    self.dpd_interval = dpd_interval;
    self
  }

  pub fn no_xmlpost(mut self, no_xmlpost: bool) -> Self {
    self.no_xmlpost = no_xmlpost;
    self
  }

  fn determine_script(&self) -> Result<&str, VpnError> {
    match &self.script {
      Some(script) => {
        if self.script_is_path && !std::path::Path::new(script).exists() {
          return Err(VpnError::new(format!("VPN script does not exist: {script}")));
        }
        check_executable(script).map_err(|e| VpnError::new(e.to_string()))?;
        Ok(script)
      }
      None => find_vpnc_script().ok_or_else(|| VpnError::new(String::from("Failed to find vpnc-script"))),
    }
  }

  pub fn build(self) -> Result<Vpn, VpnError> {
    let script = self.determine_script()?.to_owned();
    let script = if self.script_is_path {
      shell_quote_path(&script)
    } else {
      script
    };

    let user_agent = self.user_agent.unwrap_or_default();
    let os = self.os.unwrap_or("linux".to_string());

    Ok(Vpn {
      cancellation: Cancellation::default(),
      server: Self::to_cstring(&self.server),
      cookie: Self::to_cstring(&self.cookie),

      user_agent: Self::to_cstring(&user_agent),
      os: Self::to_cstring(&os),
      os_version: self.os_version.as_deref().map(Self::to_cstring),
      client_version: self.client_version.as_deref().map(Self::to_cstring),
      host_id: self.host_id.as_deref().map(Self::to_cstring),
      local_hostname: self.local_hostname.as_deref().map(Self::to_cstring),

      script: Self::to_cstring(&script),
      interface: self.interface.as_deref().map(Self::to_cstring),
      script_tun: self.script_tun,

      certificate: self.certificate.as_deref().map(Self::to_cstring),
      sslkey: self.sslkey.as_deref().map(Self::to_cstring),
      key_password: self.key_password.as_deref().map(Self::to_cstring),
      servercert: None,

      hip_source: self.hip_source,

      reconnect_timeout: self.reconnect_timeout,
      mtu: self.mtu,
      disable_ipv6: self.disable_ipv6,
      no_dtls: self.no_dtls,
      dpd_interval: self.dpd_interval,
      no_xmlpost: self.no_xmlpost,

      callback: Default::default(),
      hip_report_callback: Default::default(),
    })
  }

  fn to_cstring(value: &str) -> CString {
    CString::new(value.to_string()).expect("Failed to convert to CString")
  }
}

fn shell_quote_path(path: &str) -> String {
  format!("'{}'", path.replace('\'', "'\\''"))
}

#[cfg(test)]
mod tests {
  use super::*;
  use std::ffi::CString;

  #[test]
  fn quotes_vpnc_script_path_for_the_shell() {
    assert_eq!(
      shell_quote_path("/Applications/GP Connect.app/Contents/Helpers/vpnc-script"),
      "'/Applications/GP Connect.app/Contents/Helpers/vpnc-script'"
    );
    assert_eq!(shell_quote_path("/tmp/user's script"), "'/tmp/user'\\''s script'");
  }

  #[test]
  fn preserves_vpnc_script_commands() {
    let command = "\"/tmp/script with spaces\" --option";
    let vpn = Vpn::builder("vpn.example.com", "cookie")
      .script(command.to_string())
      .build()
      .unwrap();

    assert_eq!(vpn.script.to_str().unwrap(), command);
  }

  #[test]
  fn quotes_explicit_vpnc_script_paths() {
    let vpn = Vpn::builder("vpn.example.com", "cookie")
      .script_path("/bin/sh".to_string())
      .build()
      .unwrap();

    assert_eq!(vpn.script.to_str().unwrap(), "'/bin/sh'");
  }

  #[test]
  fn maps_session_info_from_callback_payload() {
    let message = CString::new("Session expires soon").unwrap();
    let connected_gw = CString::new("10.1.2.3").unwrap();
    let raw = ffi::VpnSessionInfoRaw {
      auth_expiration: 0,
      lifetime_secs: 43_200,
      user_expires: 1_776_828_409,
      lifetime_warning_prior: 1_800,
      lifetime_warning_message: message.as_ptr(),
      nlb_enabled: 1,
      nlb_connected_gw_ip: connected_gw.as_ptr(),
    };

    let info = session_info_from_raw(&raw);

    assert_eq!(info.lifetime_secs, Some(43_200));
    assert_eq!(info.user_expires, Some(1_776_828_409));
    assert_eq!(
      info.lifetime_warning,
      Some(VpnSessionWarning {
        prior_secs: 1_800,
        message: "Session expires soon".to_string(),
      })
    );
    assert!(info.nlb_enabled);
    assert_eq!(info.nlb_connected_gw_ip.as_deref(), Some("10.1.2.3"));
  }

  #[test]
  fn falls_back_to_auth_expiration_when_user_expires_is_absent() {
    let raw = ffi::VpnSessionInfoRaw {
      auth_expiration: 1_776_828_409,
      lifetime_secs: 0,
      user_expires: 0,
      lifetime_warning_prior: 0,
      lifetime_warning_message: std::ptr::null(),
      nlb_enabled: 0,
      nlb_connected_gw_ip: std::ptr::null(),
    };

    let info = session_info_from_raw(&raw);

    assert_eq!(info.user_expires, Some(1_776_828_409));
    assert_eq!(info.lifetime_secs, None);
    assert_eq!(info.lifetime_warning, None);
  }

  #[test]
  fn connect_options_include_host_id() {
    let vpn = Vpn {
      cancellation: Cancellation::default(),
      server: CString::new("gateway.example.com").unwrap(),
      cookie: CString::new("cookie").unwrap(),
      user_agent: CString::new("agent").unwrap(),
      os: CString::new("linux").unwrap(),
      os_version: None,
      client_version: None,
      host_id: Some(CString::new("profile-host-id").unwrap()),
      local_hostname: None,
      script: CString::new("/bin/true").unwrap(),
      interface: None,
      script_tun: false,
      certificate: None,
      sslkey: None,
      key_password: None,
      servercert: None,
      hip_source: HipSource::Disabled,
      reconnect_timeout: 300,
      mtu: 0,
      disable_ipv6: false,
      no_dtls: false,
      dpd_interval: 0,
      no_xmlpost: false,
      callback: Default::default(),
      hip_report_callback: Default::default(),
    };

    let options = vpn.build_connect_options();

    let host_id = unsafe { CStr::from_ptr(options.host_id) }.to_str().unwrap();
    assert_eq!(host_id, "profile-host-id");
  }

  #[test]
  fn hip_report_callback_copies_only_bounded_utf8() {
    let vpn = Vpn::builder("vpn.example.com", "cookie")
      .script_path("/bin/sh".to_string())
      .build()
      .unwrap();
    let reports = Arc::new(Mutex::new(Vec::new()));
    let received = Arc::clone(&reports);
    vpn.set_hip_report_callback(move |report| received.lock().unwrap().push(report.to_owned()));

    let options = vpn.build_connect_options();
    let callback = options.on_hip_report_submitted.unwrap();
    callback(options.user_data, b"<report/>".as_ptr().cast(), 9);
    callback(options.user_data, b"\xff".as_ptr().cast(), 1);
    callback(options.user_data, b"x".as_ptr().cast(), 1024 * 1024 + 1);

    assert_eq!(*reports.lock().unwrap(), ["<report/>"]);
  }

  #[test]
  fn disconnect_before_connect_is_retained_without_contacting_server() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let vpn = Vpn::builder(&format!("https://{}", listener.local_addr().unwrap()), "test-cookie")
      .script_path("/bin/sh".to_string())
      .build()
      .unwrap();
    vpn.disconnect();
    vpn.disconnect();
    assert_ne!(vpn.connect(|_| panic!("Canceled attempt connected")), 0);
    assert_eq!(listener.accept().unwrap_err().kind(), io::ErrorKind::WouldBlock);
  }

  #[test]
  fn command_pipe_failure_preserves_stdin() {
    use std::{
      ffi::{c_int, c_void},
      fs::File,
      os::fd::AsFd,
      process::{Command, Stdio},
    };

    const CHILD: &str = "OPENCONNECT_TEST_PIPE_FAILURE";
    if std::env::var_os(CHILD).is_none() {
      // Limit descriptors only in a subprocess, leaving the test runner untouched.
      let output = Command::new("sh")
        .args([
          "-c",
          "ulimit -n 64 && exec \"$1\" --exact vpn::tests::command_pipe_failure_preserves_stdin --nocapture",
          "sh",
        ])
        .arg(std::env::current_exe().unwrap())
        .env(CHILD, "1")
        .stdin(Stdio::null())
        .output()
        .unwrap();
      assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
      );
      return;
    }

    unsafe extern "C" {
      fn openconnect_vpninfo_new(
        agent: *const c_char,
        validate: Option<unsafe extern "C" fn(*mut c_void, *const c_char) -> c_int>,
        config: Option<unsafe extern "C" fn(*mut c_void, *const c_char, c_int) -> c_int>,
        auth: Option<unsafe extern "C" fn(*mut c_void, *mut c_void) -> c_int>,
        progress: Option<unsafe extern "C" fn(*mut c_void, c_int, *const c_char, ...)>,
        data: *mut c_void,
      ) -> *mut c_void;
      fn openconnect_setup_cmd_pipe(info: *mut c_void) -> c_int;
      fn openconnect_vpninfo_free(info: *mut c_void);
    }
    // Construct first so resource exhaustion targets pipe creation specifically.
    let info = unsafe { openconnect_vpninfo_new(c"test".as_ptr(), None, None, None, None, std::ptr::null_mut()) };
    assert!(!info.is_null());
    let mut descriptors = Vec::new();
    loop {
      match File::open("/dev/null") {
        Ok(file) => descriptors.push(file),
        Err(error) => {
          assert_eq!(error.raw_os_error(), Some(24)); // EMFILE on supported Unix platforms.
          break;
        }
      }
    }
    assert!(unsafe { openconnect_setup_cmd_pipe(info) } < 0);
    unsafe { openconnect_vpninfo_free(info) };
    // Constructor resource acquisition may also fail before selecting a protocol.
    let info = unsafe { openconnect_vpninfo_new(c"test".as_ptr(), None, None, None, None, std::ptr::null_mut()) };
    if !info.is_null() {
      unsafe { openconnect_vpninfo_free(info) };
    }
    drop(descriptors);
    std::io::stdin()
      .as_fd()
      .try_clone_to_owned()
      .expect("Cleanup closed stdin");
  }

  #[test]
  fn disconnect_interrupts_in_progress_tls_and_cannot_cancel_another_attempt() {
    use std::{io::Read, sync::mpsc, thread, time::Duration};
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let vpn = Arc::new(
      Vpn::builder(&format!("https://{}", listener.local_addr().unwrap()), "test-cookie")
        .script_path("/bin/sh".to_string())
        .build()
        .unwrap(),
    );
    let (tx, rx) = mpsc::channel();
    let worker_vpn = vpn.clone();
    let worker = thread::spawn(move || {
      tx.send(worker_vpn.connect(|_| panic!("Test peer never completes TLS")))
        .unwrap();
    });
    let deadline = std::time::Instant::now() + Duration::from_secs(3);
    let peer = loop {
      match listener.accept() {
        Ok((peer, _)) => break peer,
        Err(error) if error.kind() == io::ErrorKind::WouldBlock && std::time::Instant::now() < deadline => {
          thread::sleep(Duration::from_millis(5));
        }
        other => {
          vpn.disconnect();
          worker.join().unwrap();
          panic!("Connection did not reach test peer: {other:?}");
        }
      }
    };
    vpn.disconnect();
    let result = rx.recv_timeout(Duration::from_secs(2));
    drop(peer); // Release the peer even if cancellation regresses.
    worker.join().unwrap();
    assert_ne!(result.expect("Cancellation did not interrupt TLS"), 0);
    use std::os::{fd::AsRawFd, unix::net::UnixStream};
    let next = Cancellation::default();
    let (mut reader, writer) = UnixStream::pair().unwrap();
    reader.set_nonblocking(true).unwrap();
    assert!(!next.attach(writer.as_raw_fd()));
    vpn.disconnect(); // A late stop of the old attempt cannot affect the new one.
    assert_eq!(reader.read(&mut [0]).unwrap_err().kind(), io::ErrorKind::WouldBlock);
    next.detach();
  }

  #[test]
  fn cancellation_racing_pipe_attachment_is_never_lost() {
    use std::{
      io::Read,
      os::{fd::AsRawFd, unix::net::UnixStream},
      thread,
    };
    for _ in 0..100 {
      let cancellation = Cancellation::default();
      let (mut reader, writer) = UnixStream::pair().unwrap();
      reader.set_nonblocking(true).unwrap();
      let pending = thread::scope(|scope| {
        let cancel = scope.spawn(|| cancellation.cancel().unwrap());
        let pending = cancellation.attach(writer.as_raw_fd());
        cancel.join().unwrap();
        pending
      });
      if !pending {
        let mut command = [0];
        assert_eq!(reader.read(&mut command).unwrap(), 1);
        assert_eq!(command[0], b'x');
      }
      cancellation.detach();
    }
  }

  #[test]
  fn disconnect_after_detach_never_writes_to_the_old_descriptor() {
    use std::{
      io::Read,
      os::{fd::AsRawFd, unix::net::UnixStream},
    };
    let cancellation = Cancellation::default();
    let (mut reader, writer) = UnixStream::pair().unwrap();
    reader.set_nonblocking(true).unwrap();
    assert!(!cancellation.attach(writer.as_raw_fd()));
    cancellation.detach();
    cancellation.cancel().unwrap();
    assert_eq!(reader.read(&mut [0]).unwrap_err().kind(), io::ErrorKind::WouldBlock);
  }
}
