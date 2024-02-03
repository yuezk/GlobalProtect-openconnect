use std::{
  ffi::{c_char, CString},
  sync::{Arc, RwLock},
};

use log::info;

use crate::{ffi, vpnc_script::find_default_vpnc_script};

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
    }
  }

  fn option_to_ptr(option: &Option<CString>) -> *const c_char {
    match option {
      Some(value) => value.as_ptr(),
      None => std::ptr::null(),
    }
  }
}

pub struct VpnBuilder {
  server: String,
  cookie: String,
  user_agent: Option<String>,
  script: Option<String>,
  os: Option<String>,

  csd_uid: u32,
  csd_wrapper: Option<String>,
}

impl VpnBuilder {
  fn new(server: &str, cookie: &str) -> Self {
    Self {
      server: server.to_string(),
      cookie: cookie.to_string(),
      user_agent: None,
      script: None,
      os: None,
      csd_uid: 0,
      csd_wrapper: None,
    }
  }

  pub fn user_agent<T: Into<Option<String>>>(mut self, user_agent: T) -> Self {
    self.user_agent = user_agent.into();
    self
  }

  pub fn script<T: Into<Option<String>>>(mut self, script: T) -> Self {
    self.script = script.into();
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

  pub fn build(self) -> Vpn {
    let user_agent = self.user_agent.unwrap_or_default();
    let script = self.script.or_else(find_default_vpnc_script).unwrap_or_default();
    let os = self.os.unwrap_or("linux".to_string());

    Vpn {
      server: Self::to_cstring(&self.server),
      cookie: Self::to_cstring(&self.cookie),
      user_agent: Self::to_cstring(&user_agent),
      script: Self::to_cstring(&script),
      os: Self::to_cstring(&os),
      certificate: None,
      servercert: None,

      csd_uid: self.csd_uid,
      csd_wrapper: self.csd_wrapper.as_deref().map(Self::to_cstring),

      callback: Default::default(),
    }
  }

  fn to_cstring(value: &str) -> CString {
    CString::new(value.to_string()).expect("Failed to convert to CString")
  }
}
