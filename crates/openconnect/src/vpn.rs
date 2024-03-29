use std::{
  ffi::{c_char, CString},
  fmt,
  sync::{Arc, RwLock},
};

use common::vpn_utils::{find_vpnc_script, is_executable};
use log::info;

use crate::ffi;

type OnConnectedCallback = Arc<RwLock<Option<Box<dyn FnOnce() + 'static + Send + Sync>>>>;

pub struct Vpn {
  server: CString,
  cookie: CString,
  user_agent: CString,
  script: CString,
  os: CString,
  certificate: Option<CString>,
  servercert: Option<CString>,

  csd_uid: u32,
  csd_wrapper: Option<CString>,

  mtu: u32,

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
      os: self.os.as_ptr(),
      certificate: Self::option_to_ptr(&self.certificate),
      servercert: Self::option_to_ptr(&self.servercert),

      csd_uid: self.csd_uid,
      csd_wrapper: Self::option_to_ptr(&self.csd_wrapper),

      mtu: self.mtu,
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
pub struct VpnError<'a> {
  message: &'a str,
}

impl<'a> VpnError<'a> {
  fn new(message: &'a str) -> Self {
    Self { message }
  }
}

impl fmt::Display for VpnError<'_> {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    write!(f, "{}", self.message)
  }
}

impl std::error::Error for VpnError<'_> {}

pub struct VpnBuilder {
  server: String,
  cookie: String,
  script: Option<String>,

  user_agent: Option<String>,
  os: Option<String>,

  csd_uid: u32,
  csd_wrapper: Option<String>,

  mtu: u32,
}

impl VpnBuilder {
  fn new(server: &str, cookie: &str) -> Self {
    Self {
      server: server.to_string(),
      cookie: cookie.to_string(),
      script: None,

      user_agent: None,
      os: None,

      csd_uid: 0,
      csd_wrapper: None,

      mtu: 0,
    }
  }

  pub fn script<T: Into<Option<String>>>(mut self, script: T) -> Self {
    self.script = script.into();
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

  pub fn csd_uid(mut self, csd_uid: u32) -> Self {
    self.csd_uid = csd_uid;
    self
  }

  pub fn csd_wrapper<T: Into<Option<String>>>(mut self, csd_wrapper: T) -> Self {
    self.csd_wrapper = csd_wrapper.into();
    self
  }

  pub fn mtu(mut self, mtu: u32) -> Self {
    self.mtu = mtu;
    self
  }

  pub fn build(self) -> Result<Vpn, VpnError<'static>> {
    let script = match self.script {
      Some(script) => {
        if !is_executable(&script) {
          return Err(VpnError::new("vpnc script is not executable"));
        }
        script
      }
      None => find_vpnc_script().ok_or_else(|| VpnError::new("Failed to find vpnc-script"))?,
    };

    if let Some(csd_wrapper) = &self.csd_wrapper {
      if !is_executable(csd_wrapper) {
        return Err(VpnError::new("CSD wrapper is not executable"));
      }
    }

    let user_agent = self.user_agent.unwrap_or_default();
    let os = self.os.unwrap_or("linux".to_string());

    Ok(Vpn {
      server: Self::to_cstring(&self.server),
      cookie: Self::to_cstring(&self.cookie),
      user_agent: Self::to_cstring(&user_agent),
      script: Self::to_cstring(&script),
      os: Self::to_cstring(&os),
      certificate: None,
      servercert: None,

      csd_uid: self.csd_uid,
      csd_wrapper: self.csd_wrapper.as_deref().map(Self::to_cstring),

      mtu: self.mtu,

      callback: Default::default(),
    })
  }

  fn to_cstring(value: &str) -> CString {
    CString::new(value.to_string()).expect("Failed to convert to CString")
  }
}
