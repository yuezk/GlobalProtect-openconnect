mod hip;
use crate::Vpn;
use hip::HipGenerateFn;
pub(crate) use hip::{HipControlRaw, HipScriptRaw, generate_hip_report, preview_hip_script, script_environment};
use log::{debug, info, trace, warn};
use std::ffi::{c_char, c_int, c_long, c_void};
use std::panic::{AssertUnwindSafe, catch_unwind};

/// ConnectOptions struct for FFI, the field names and order must match the C definition.
#[repr(C)]
#[derive(Debug)]
pub(crate) struct ConnectOptions {
  pub user_data: *mut c_void,
  pub on_hip_report_submitted: Option<extern "C" fn(*mut c_void, *const c_char, usize)>,

  pub server: *const c_char,
  pub cookie: *const c_char,

  pub user_agent: *const c_char,
  pub os: *const c_char,
  pub os_version: *const c_char,
  pub client_version: *const c_char,
  pub host_id: *const c_char,
  pub local_hostname: *const c_char,

  pub script: *const c_char,
  pub interface: *const c_char,
  pub script_tun: u32,

  pub certificate: *const c_char,
  pub sslkey: *const c_char,
  pub key_password: *const c_char,
  pub servercert: *const c_char,

  pub hip_script: HipScriptRaw,
  pub generate_hip: Option<HipGenerateFn>,

  pub reconnect_timeout: u32,
  pub mtu: u32,
  pub disable_ipv6: u32,
  pub no_dtls: u32,

  pub dpd_interval: u32,
  pub no_xmlpost: u32,
}

#[repr(C)]
#[derive(Debug)]
pub(crate) struct VpnSessionInfoRaw {
  pub auth_expiration: c_long,
  pub lifetime_secs: c_int,
  pub user_expires: c_long,
  pub lifetime_warning_prior: c_int,
  pub lifetime_warning_message: *const c_char,
  pub nlb_enabled: c_int,
  pub nlb_connected_gw_ip: *const c_char,
}

#[link(name = "vpn")]
unsafe extern "C" {
  fn vpn_write_cancel(fd: c_int) -> c_int;
  #[link_name = "vpn_connect"]
  fn vpn_connect(
    options: *const ConnectOptions,
    callback: extern "C" fn(i32, *const VpnSessionInfoRaw, *mut c_void),
  ) -> c_int;

}

pub(crate) fn write_cancel(fd: i32) -> std::io::Result<()> {
  let error = unsafe { vpn_write_cancel(fd) };
  if error == 0 {
    Ok(())
  } else {
    Err(std::io::Error::from_raw_os_error(error))
  }
}

#[unsafe(no_mangle)]
extern "C" fn vpn_attach_command_pipe(vpn: *mut c_void, fd: c_int) -> c_int {
  unsafe { &*(vpn as *const Vpn) }.attach_command_pipe(fd).into()
}

#[unsafe(no_mangle)]
extern "C" fn vpn_detach_command_pipe(vpn: *mut c_void) {
  unsafe { &*(vpn as *const Vpn) }.detach_command_pipe();
}

pub(crate) fn connect(options: &ConnectOptions) -> i32 {
  unsafe { vpn_connect(options, on_vpn_connected) }
}

#[unsafe(no_mangle)]
pub(crate) extern "C" fn on_hip_report_submitted(vpn: *mut c_void, report: *const c_char, length: usize) {
  let vpn = unsafe { &*(vpn as *const Vpn) };
  if report.is_null() || length > 1024 * 1024 {
    return;
  }
  let bytes = unsafe { std::slice::from_raw_parts(report.cast::<u8>(), length) };
  if let Ok(report) = std::str::from_utf8(bytes) {
    if catch_unwind(AssertUnwindSafe(|| vpn.on_hip_report_submitted(report))).is_err() {
      warn!("HIP submission observer failed");
    }
  }
}

#[unsafe(no_mangle)]
extern "C" fn on_vpn_connected(pipe_fd: i32, session_info: *const VpnSessionInfoRaw, vpn: *mut c_void) {
  let vpn = unsafe { &*(vpn as *const Vpn) };
  vpn.on_connected(pipe_fd, crate::vpn::session_info_from_raw(session_info));
}

// Logger used in the C code.
// level: 0 = error, 1 = info, 2 = debug, 3 = trace
// map the error level log in openconnect to the warning level
#[unsafe(no_mangle)]
extern "C" fn vpn_log(level: i32, message: *const c_char) {
  let message = unsafe { std::ffi::CStr::from_ptr(message) };
  let message = message.to_str().unwrap_or("Invalid log message");
  // Strip the trailing newline
  let message = message.trim_end_matches('\n');

  if level == 0 {
    warn!("{}", message);
  } else if level == 1 {
    info!("{}", message);
  } else if level == 2 {
    debug!("{}", message);
  } else if level == 3 {
    trace!("{}", message);
  } else {
    warn!(
      "Unknown log level: {}, enable DEBUG log level to see more details",
      level
    );
    debug!("{}", message);
  }
}
