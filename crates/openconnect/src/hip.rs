use std::{ffi::CString, io, marker::PhantomData, rc::Rc, sync::Arc};

use crate::{ffi, vpn_utils::check_executable};

pub type HipGenerator = dyn Fn(&HipRequest, &HipControl<'_>) -> io::Result<String> + Send + Sync;
type HipValidator = dyn Fn(&HipControl<'_>) -> io::Result<()> + Send + Sync;

#[derive(Default)]
pub enum HipSource {
  #[default]
  Disabled,
  Generator(Arc<HipGenerator>),
  Script(HipScript),
}

/// Current gateway and negotiated identity inputs for one report invocation.
#[derive(Clone, Default)]
pub struct HipRequest {
  pub cookie: String,
  pub client_ip: Option<String>,
  pub client_ipv6: Option<String>,
  pub md5: String,
  pub client_version: String,
  pub client_os: String,
  pub os_version: String,
  pub host_id: Option<String>,
  pub local_hostname: Option<String>,
}

/// Borrowed cancellation and deadline control. It must stay on the invocation thread.
pub struct HipControl<'a> {
  pub(crate) raw: &'a ffi::HipControlRaw,
  pub(crate) _thread: PhantomData<Rc<()>>,
}

impl HipControl<'_> {
  pub fn check(&self) -> io::Result<()> {
    let result = unsafe { (self.raw.check)(self.raw.data) };
    if result < 0 {
      Err(io::Error::from_raw_os_error(-result))
    } else {
      Ok(())
    }
  }
}

pub struct HipScriptEnvironment {
  pub variables: Vec<(String, String)>,
  pub cwd: String,
}

pub struct HipScript {
  pub(crate) path: CString,
  pub(crate) user: Option<u32>,
  pub(crate) validator: Option<Arc<HipValidator>>,
  pub(crate) environment: Option<Vec<CString>>,
  pub(crate) cwd: Option<CString>,
}

impl HipScript {
  pub fn new(path: String, user: Option<u32>) -> io::Result<Self> {
    if !std::fs::metadata(&path)?.is_file() {
      return Err(io::Error::new(
        io::ErrorKind::InvalidInput,
        "HIP executable is not a regular file",
      ));
    }
    check_executable(&path)?;
    let path = c_string(path)?;
    Ok(Self {
      path,
      user,
      validator: None,
      environment: None,
      cwd: None,
    })
  }

  pub fn with_validator(
    mut self,
    validator: impl Fn(&HipControl<'_>) -> io::Result<()> + Send + Sync + 'static,
  ) -> Self {
    self.validator = Some(Arc::new(validator));
    self
  }

  /// A child-local replacement environment and working directory, never process-global changes.
  pub fn with_environment(mut self, environment: HipScriptEnvironment) -> io::Result<Self> {
    let variables = environment
      .variables
      .into_iter()
      .map(|(key, value)| {
        if key.is_empty() || key.contains('=') {
          return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "Invalid HIP environment key",
          ));
        }
        c_string(format!("{key}={value}"))
      })
      .collect::<io::Result<Vec<_>>>()?;
    self.environment = Some(variables);
    self.cwd = Some(c_string(environment.cwd)?);
    Ok(self)
  }

  /// Collect using the same OpenConnect subprocess machinery as connected submissions.
  pub fn preview(&self, request: &HipRequest, check: &dyn Fn() -> io::Result<()>) -> io::Result<String> {
    ffi::preview_hip_script(self, request, check)
  }
}

fn c_string(value: String) -> io::Result<CString> {
  CString::new(value).map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "HIP value contains NUL"))
}
