use std::{
  ffi::{CStr, c_char, c_int, c_void},
  io,
  panic::{AssertUnwindSafe, catch_unwind},
};

use crate::{NetworkConfiguration, Vpn};

/// Matches OpenConnect's oc_vpn_option list. Borrowed for this invocation only.
#[repr(C)]
pub(crate) struct VpnOptionRaw {
  pub option: *const c_char,
  pub value: *const c_char,
  pub next: *const VpnOptionRaw,
}

/// # Safety
/// A non-null `value` must point to a readable, NUL-terminated string that
/// remains valid and unchanged until this function returns.
unsafe fn read_string(value: *const c_char) -> io::Result<String> {
  if value.is_null() {
    return Err(io::Error::from_raw_os_error(libc::EINVAL));
  }
  // SAFETY: The caller guarantees a valid C string; null was rejected above.
  unsafe { CStr::from_ptr(value) }
    .to_str()
    .map(str::to_owned)
    .map_err(|_| io::Error::from_raw_os_error(libc::EINVAL))
}

/// # Safety
/// Non-null strings must satisfy `read_string`'s contract. `options` must be
/// null or a finite, acyclic list of aligned, readable nodes. The list and its
/// strings must remain valid and unchanged until this function returns.
unsafe fn snapshot(reason: *const c_char, mut options: *const VpnOptionRaw) -> io::Result<NetworkConfiguration> {
  // SAFETY: The caller guarantees the string lifetime and validity.
  let reason = unsafe { read_string(reason)? };
  let mut environment = Vec::new();
  while !options.is_null() {
    // SAFETY: This is a non-null node in the valid list supplied by the caller.
    let option = unsafe { &*options };
    // SAFETY: Node strings satisfy read_string's contract, including null handling.
    let name = unsafe { read_string(option.option)? };
    let value = if option.value.is_null() {
      None
    } else {
      // SAFETY: The caller guarantees this non-null string's validity and lifetime.
      Some(unsafe { read_string(option.value)? })
    };
    environment.push((name, value));
    options = option.next;
  }
  Ok(NetworkConfiguration { reason, environment })
}

/// # Safety
/// `data` must point to an aligned, live `Vpn` that may be shared immutably for
/// this entire invocation. `reason` and `options` must satisfy `snapshot`'s
/// contract. OpenConnect invokes this callback synchronously during `connect`,
/// including cleanup, before the borrowed `Vpn` or configuration data is freed.
pub(crate) unsafe extern "C" fn configure_network(
  data: *mut c_void,
  reason: *const c_char,
  options: *const VpnOptionRaw,
) -> c_int {
  let result = catch_unwind(AssertUnwindSafe(|| {
    // SAFETY: The caller supplies the live Vpn pointer registered for this connection.
    let vpn = unsafe { &*(data as *const Vpn) };
    // SAFETY: The caller keeps OpenConnect's list and strings valid during this call.
    let configuration = unsafe { snapshot(reason, options)? };
    let handler = vpn
      .network_configuration_handler
      .as_ref()
      .ok_or_else(|| io::Error::from_raw_os_error(libc::EINVAL))?;
    handler(&configuration)
  }));
  match result {
    Ok(Ok(())) => 0,
    Ok(Err(error)) => {
      log::warn!("Network configuration failed: {error}");
      -error.raw_os_error().filter(|code| *code > 0).unwrap_or(libc::EIO)
    }
    Err(_) => {
      log::warn!("Network configuration handler panicked");
      -libc::EIO
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use std::{
    ffi::CString,
    sync::{Arc, Mutex},
  };

  #[test]
  fn callback_copies_configuration_and_preserves_unset_values() {
    let observed = Arc::new(Mutex::new(None));
    let output = observed.clone();
    let vpn = Vpn::builder("server", "cookie")
      .network_configuration_handler(move |configuration| {
        *output.lock().unwrap() = Some(configuration.clone());
        Ok(())
      })
      .build()
      .unwrap();
    let name = CString::new("TUNDEV").unwrap();
    let value = CString::new("utun3").unwrap();
    let unset_name = CString::new("INTERNAL_IP6_ADDRESS").unwrap();
    let unset = VpnOptionRaw {
      option: unset_name.as_ptr(),
      value: std::ptr::null(),
      next: std::ptr::null(),
    };
    let option = VpnOptionRaw {
      option: name.as_ptr(),
      value: value.as_ptr(),
      next: &unset,
    };
    // SAFETY: vpn, both list nodes, and their C strings remain live for the call.
    let result = unsafe { configure_network((&vpn as *const Vpn).cast_mut().cast(), c"connect".as_ptr(), &option) };
    assert_eq!(result, 0);
    let configuration = observed.lock().unwrap().take().unwrap();
    assert_eq!(configuration.reason, "connect");
    assert_eq!(
      configuration.environment,
      vec![
        ("TUNDEV".into(), Some("utun3".into())),
        ("INTERNAL_IP6_ADDRESS".into(), None)
      ]
    );
  }

  #[test]
  fn callback_errors_and_panics_do_not_cross_ffi() {
    for panics in [false, true] {
      let vpn = Vpn::builder("server", "cookie")
        .network_configuration_handler(move |_| {
          if panics {
            panic!("test panic");
          }
          Err(io::Error::from_raw_os_error(libc::EPERM))
        })
        .build()
        .unwrap();
      // SAFETY: vpn and the static reason are live; a null list is permitted.
      let result = unsafe {
        configure_network(
          (&vpn as *const Vpn).cast_mut().cast(),
          c"connect".as_ptr(),
          std::ptr::null(),
        )
      };
      assert_eq!(result, if panics { -libc::EIO } else { -libc::EPERM });
    }
  }

  #[test]
  fn callback_rejects_script_options() {
    assert!(
      Vpn::builder("server", "cookie")
        .network_configuration_handler(|_| Ok(()))
        .script_tun(true)
        .build()
        .is_err()
    );
    assert!(
      Vpn::builder("server", "cookie")
        .network_configuration_handler(|_| Ok(()))
        .script(Some("/bin/true".into()))
        .build()
        .is_err()
    );
  }
}
