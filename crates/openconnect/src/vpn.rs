use std::{
  ffi::{CStr, CString, c_char},
  fmt,
  sync::{Arc, RwLock},
};

use log::info;

use crate::ffi;
use crate::vpn_utils::{check_executable, find_csd_wrapper, find_vpnc_script};

type OnConnectedCallback = Arc<RwLock<Option<Box<dyn FnOnce(VpnSessionInfo) + 'static + Send + Sync>>>>;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct VpnSessionInfo {
  pub lifetime_secs: Option<u32>,
  pub user_expires: Option<u32>,
  pub lifetime_warning: Option<VpnSessionWarning>,
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

pub struct Vpn {
  server: CString,
  cookie: CString,

  user_agent: CString,
  os: CString,
  os_version: Option<CString>,
  client_version: Option<CString>,
  local_hostname: Option<CString>,

  script: CString,
  interface: Option<CString>,
  script_tun: bool,

  certificate: Option<CString>,
  sslkey: Option<CString>,
  key_password: Option<CString>,
  servercert: Option<CString>,

  csd_uid: u32,
  csd_wrapper: Option<CString>,

  reconnect_timeout: u32,
  mtu: u32,
  disable_ipv6: bool,
  no_dtls: bool,

  dpd_interval: u32,
  no_xmlpost: bool,

  callback: OnConnectedCallback,
}

impl Vpn {
  pub fn builder(server: &str, cookie: &str) -> VpnBuilder {
    VpnBuilder::new(server, cookie)
  }

  pub fn connect(&self, on_connected: impl FnOnce(VpnSessionInfo) + 'static + Send + Sync) -> i32 {
    self.callback.write().unwrap().replace(Box::new(on_connected));
    let options = self.build_connect_options();

    ffi::connect(&options)
  }

  pub(crate) fn on_connected(&self, pipe_fd: i32, session_info: VpnSessionInfo) {
    info!("Connected to VPN, pipe_fd: {}", pipe_fd);

    if let Some(callback) = self.callback.write().unwrap().take() {
      callback(session_info);
    }
  }

  pub fn disconnect(&self) {
    ffi::disconnect();
  }

  fn build_connect_options(&self) -> ffi::ConnectOptions {
    ffi::ConnectOptions {
      user_data: self as *const _ as *mut _,

      server: self.server.as_ptr(),
      cookie: self.cookie.as_ptr(),

      user_agent: self.user_agent.as_ptr(),
      os: self.os.as_ptr(),
      os_version: Self::option_to_ptr(&self.os_version),
      client_version: Self::option_to_ptr(&self.client_version),
      local_hostname: Self::option_to_ptr(&self.local_hostname),

      script: self.script.as_ptr(),
      interface: Self::option_to_ptr(&self.interface),
      script_tun: self.script_tun as u32,

      certificate: Self::option_to_ptr(&self.certificate),
      sslkey: Self::option_to_ptr(&self.sslkey),
      key_password: Self::option_to_ptr(&self.key_password),
      servercert: Self::option_to_ptr(&self.servercert),

      csd_uid: self.csd_uid,
      csd_wrapper: Self::option_to_ptr(&self.csd_wrapper),

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
  interface: Option<String>,
  script_tun: bool,

  user_agent: Option<String>,
  os: Option<String>,
  os_version: Option<String>,
  client_version: Option<String>,
  local_hostname: Option<String>,

  certificate: Option<String>,
  sslkey: Option<String>,
  key_password: Option<String>,

  hip: bool,
  csd_uid: u32,
  csd_wrapper: Option<String>,

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
      interface: None,
      script_tun: false,

      user_agent: None,
      os: None,
      os_version: None,
      client_version: None,
      local_hostname: None,

      certificate: None,
      sslkey: None,
      key_password: None,

      hip: false,
      csd_uid: 0,
      csd_wrapper: None,

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

  pub fn hip(mut self, hip: bool) -> Self {
    self.hip = hip;
    self
  }

  pub fn csd_uid(mut self, csd_uid: u32) -> Self {
    self.csd_uid = csd_uid;
    self
  }

  pub fn csd_wrapper<T: Into<Option<String>>>(mut self, csd_wrapper: T) -> Self {
    self.csd_wrapper = csd_wrapper.into();
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
        check_executable(script).map_err(|e| VpnError::new(e.to_string()))?;
        Ok(script)
      }
      None => find_vpnc_script().ok_or_else(|| VpnError::new(String::from("Failed to find vpnc-script"))),
    }
  }

  fn determine_csd_wrapper(&self) -> Result<Option<&str>, VpnError> {
    if !self.hip {
      return Ok(None);
    }

    match &self.csd_wrapper {
      Some(csd_wrapper) if !csd_wrapper.is_empty() => {
        check_executable(csd_wrapper).map_err(|e| VpnError::new(e.to_string()))?;
        Ok(Some(csd_wrapper))
      }
      _ => {
        let s = find_csd_wrapper().ok_or_else(|| VpnError::new(String::from("Failed to find csd wrapper")))?;
        Ok(Some(s))
      }
    }
  }

  pub fn build(self) -> Result<Vpn, VpnError> {
    let script = self.determine_script()?.to_owned();
    let csd_wrapper = self.determine_csd_wrapper()?.map(|s| s.to_owned());

    let user_agent = self.user_agent.unwrap_or_default();
    let os = self.os.unwrap_or("linux".to_string());

    Ok(Vpn {
      server: Self::to_cstring(&self.server),
      cookie: Self::to_cstring(&self.cookie),

      user_agent: Self::to_cstring(&user_agent),
      os: Self::to_cstring(&os),
      os_version: self.os_version.as_deref().map(Self::to_cstring),
      client_version: self.client_version.as_deref().map(Self::to_cstring),
      local_hostname: self.local_hostname.as_deref().map(Self::to_cstring),

      script: Self::to_cstring(&script),
      interface: self.interface.as_deref().map(Self::to_cstring),
      script_tun: self.script_tun,

      certificate: self.certificate.as_deref().map(Self::to_cstring),
      sslkey: self.sslkey.as_deref().map(Self::to_cstring),
      key_password: self.key_password.as_deref().map(Self::to_cstring),
      servercert: None,

      csd_uid: self.csd_uid,
      csd_wrapper: csd_wrapper.as_deref().map(Self::to_cstring),

      reconnect_timeout: self.reconnect_timeout,
      mtu: self.mtu,
      disable_ipv6: self.disable_ipv6,
      no_dtls: self.no_dtls,
      dpd_interval: self.dpd_interval,
      no_xmlpost: self.no_xmlpost,

      callback: Default::default(),
    })
  }

  fn to_cstring(value: &str) -> CString {
    CString::new(value.to_string()).expect("Failed to convert to CString")
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use std::ffi::CString;

  #[test]
  fn maps_session_info_from_callback_payload() {
    let message = CString::new("Session expires soon").unwrap();
    let raw = ffi::VpnSessionInfoRaw {
      auth_expiration: 0,
      lifetime_secs: 43_200,
      user_expires: 1_776_828_409,
      lifetime_warning_prior: 1_800,
      lifetime_warning_message: message.as_ptr(),
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
  }

  #[test]
  fn falls_back_to_auth_expiration_when_user_expires_is_absent() {
    let raw = ffi::VpnSessionInfoRaw {
      auth_expiration: 1_776_828_409,
      lifetime_secs: 0,
      user_expires: 0,
      lifetime_warning_prior: 0,
      lifetime_warning_message: std::ptr::null(),
    };

    let info = session_info_from_raw(&raw);

    assert_eq!(info.user_expires, Some(1_776_828_409));
    assert_eq!(info.lifetime_secs, None);
    assert_eq!(info.lifetime_warning, None);
  }
}
