use std::{
  ffi::{c_char, CString},
  fmt,
  sync::{Arc, RwLock},
};

use log::info;

use crate::ffi;
use crate::vpn_utils::{check_executable, find_csd_wrapper, find_vpnc_script};

type OnConnectedCallback = Arc<RwLock<Option<Box<dyn FnOnce() + 'static + Send + Sync>>>>;

pub struct Vpn {
  server: CString,
  cookie: CString,
  user_agent: CString,
  script: CString,
  interface: Option<CString>,
  os: CString,
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

  callback: OnConnectedCallback,
}

impl Vpn {
  pub fn builder(server: &str, cookie: &str) -> VpnBuilder {
    VpnBuilder::new(server, cookie)
  }

  pub fn connect(&self, on_connected: impl FnOnce() + 'static + Send + Sync) -> i32 {
    self.callback.write().unwrap().replace(Box::new(on_connected));
    let options = self.build_connect_options();

    ffi::connect(&options)
  }

  pub(crate) fn on_connected(&self, pipe_fd: i32) {
    info!("Connected to VPN, pipe_fd: {}", pipe_fd);

    if let Some(callback) = self.callback.write().unwrap().take() {
      callback();
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
      script: self.script.as_ptr(),
      interface: Self::option_to_ptr(&self.interface),
      os: self.os.as_ptr(),

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

  user_agent: Option<String>,
  os: Option<String>,

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
}

impl VpnBuilder {
  fn new(server: &str, cookie: &str) -> Self {
    Self {
      server: server.to_string(),
      cookie: cookie.to_string(),
      script: None,
      interface: None,

      user_agent: None,
      os: None,

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

  pub fn user_agent<T: Into<Option<String>>>(mut self, user_agent: T) -> Self {
    self.user_agent = user_agent.into();
    self
  }

  pub fn os<T: Into<Option<String>>>(mut self, os: T) -> Self {
    self.os = os.into();
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

  fn determine_script(&self) -> Result<String, VpnError> {
    match &self.script {
      Some(script) => {
        check_executable(script).map_err(|e| VpnError::new(e.to_string()))?;
        Ok(script.clone())
      }
      None => find_vpnc_script().ok_or_else(|| VpnError::new(String::from("Failed to find vpnc-script"))),
    }
  }

  fn determine_csd_wrapper(&self) -> Result<Option<String>, VpnError> {
    if !self.hip {
      return Ok(None);
    }

    match &self.csd_wrapper {
      Some(csd_wrapper) if !csd_wrapper.is_empty() => {
        check_executable(csd_wrapper).map_err(|e| VpnError::new(e.to_string()))?;
        Ok(Some(csd_wrapper.clone()))
      }
      _ => {
        let s = find_csd_wrapper().ok_or_else(|| VpnError::new(String::from("Failed to find csd wrapper")))?;
        Ok(Some(s))
      }
    }
  }

  pub fn build(self) -> Result<Vpn, VpnError> {
    let script = self.determine_script()?;
    let csd_wrapper = self.determine_csd_wrapper()?;

    let user_agent = self.user_agent.unwrap_or_default();
    let os = self.os.unwrap_or("linux".to_string());

    Ok(Vpn {
      server: Self::to_cstring(&self.server),
      cookie: Self::to_cstring(&self.cookie),
      user_agent: Self::to_cstring(&user_agent),
      script: Self::to_cstring(&script),
      interface: self.interface.as_deref().map(Self::to_cstring),
      os: Self::to_cstring(&os),

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

      callback: Default::default(),
    })
  }

  fn to_cstring(value: &str) -> CString {
    CString::new(value.to_string()).expect("Failed to convert to CString")
  }
}
