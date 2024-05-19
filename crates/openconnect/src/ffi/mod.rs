use crate::Vpn;
use log::{debug, info, trace, warn};
use std::ffi::{c_char, c_int, c_void};

#[repr(C)]
#[derive(Debug)]
pub(crate) struct ConnectOptions {
  pub user_data: *mut c_void,

  pub server: *const c_char,
  pub cookie: *const c_char,
  pub user_agent: *const c_char,

  pub script: *const c_char,
  pub os: *const c_char,
  pub certificate: *const c_char,
  pub servercert: *const c_char,

  pub csd_uid: u32,
  pub csd_wrapper: *const c_char,

  pub reconnect_timeout: u32,
  pub mtu: u32,
  pub disable_ipv6: u32,
}

#[link(name = "vpn")]
extern "C" {
  #[link_name = "vpn_connect"]
  fn vpn_connect(options: *const ConnectOptions, callback: extern "C" fn(i32, *mut c_void)) -> c_int;

  #[link_name = "vpn_disconnect"]
  fn vpn_disconnect();
}

pub(crate) fn connect(options: &ConnectOptions) -> i32 {
  unsafe { vpn_connect(options, on_vpn_connected) }
}

pub(crate) fn disconnect() {
  unsafe { vpn_disconnect() }
}

#[no_mangle]
extern "C" fn on_vpn_connected(pipe_fd: i32, vpn: *mut c_void) {
  let vpn = unsafe { &*(vpn as *const Vpn) };
  vpn.on_connected(pipe_fd);
}

// Logger used in the C code.
// level: 0 = error, 1 = info, 2 = debug, 3 = trace
// map the error level log in openconnect to the warning level
#[no_mangle]
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
