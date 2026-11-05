use super::{
  ClientOs, NativeHostIdentitySnapshot, derive_uuid_from_seed, fallback_hostname, primary_mac_colon, resolve_host_id,
  serial_number,
};
use log::debug;

pub(super) struct RuntimeNativeHostIdentity;

impl RuntimeNativeHostIdentity {
  pub(super) fn runtime_os() -> ClientOs {
    ClientOs::Linux
  }

  pub(super) fn collect_with_control(
    host_id_override: Option<&str>,
    control: &dyn super::CollectionControl,
  ) -> super::io::Result<NativeHostIdentitySnapshot> {
    control.check()?;
    let identity = Self::collect(host_id_override);
    control.check()?;
    Ok(identity)
  }

  fn collect(host_id_override: Option<&str>) -> NativeHostIdentitySnapshot {
    if let Some(host_id) = host_id_override.filter(|value| !value.trim().is_empty()) {
      debug!("Runtime host-id source: explicit override ({})", host_id);
    }
    let host_id = resolve_host_id(host_id_override, Self::host_id);
    NativeHostIdentitySnapshot {
      host_id,
      computer: fallback_hostname(),
      serial_number: collect_linux_serial_number(),
      mac_address: primary_mac_colon(),
    }
  }

  fn host_id() -> String {
    if let Some(host_id) = collect_linux_product_uuid() {
      debug!("Runtime host-id source: /sys/class/dmi/id/product_uuid ({})", host_id);
      return host_id;
    }

    let (source_name, source) = collect_linux_fallback_seed().unwrap_or_else(|| {
      let hostname = fallback_hostname();
      ("hostname", hostname)
    });
    let host_id = derive_uuid_from_seed(&source, &[]);
    debug!("Runtime host-id source: derived from {} ({})", source_name, host_id);
    host_id
  }
}

fn collect_linux_product_uuid() -> Option<String> {
  read_linux_identity_file("/sys/class/dmi/id/product_uuid").and_then(|value| normalize_uuid(&value))
}

fn normalize_uuid(value: &str) -> Option<String> {
  uuid::Uuid::parse_str(value.trim())
    .ok()
    .map(|uuid| uuid.hyphenated().to_string())
}

fn collect_linux_fallback_seed() -> Option<(&'static str, String)> {
  ["/etc/machine-id", "/var/lib/dbus/machine-id"]
    .iter()
    .find_map(|path| read_linux_identity_file(path).map(|value| (*path, value)))
}

fn read_linux_identity_file(path: &str) -> Option<String> {
  std::fs::read_to_string(path)
    .ok()
    .map(|value| value.trim().to_string())
    .filter(|value| is_usable_linux_identity_value(value))
}

fn is_usable_linux_identity_value(value: &str) -> bool {
  !value.is_empty() && value != "None" && value != "To Be Filled By O.E.M."
}

fn collect_linux_serial_number() -> Option<String> {
  resolve_linux_serial_number(
    read_linux_identity_file("/sys/class/dmi/id/product_serial"),
    collect_linux_product_uuid,
  )
}

fn resolve_linux_serial_number(
  product_serial: Option<String>,
  product_uuid: impl FnOnce() -> Option<String>,
) -> Option<String> {
  product_serial.or_else(|| product_uuid().and_then(|uuid| serial_number::vmware_from_uuid(&uuid)))
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn normalize_uuid_preserves_real_uuid_identity() {
    assert_eq!(
      normalize_uuid("5A784D56-6461-19AC-9EA9-D36A3B9C6CEF"),
      Some("5a784d56-6461-19ac-9ea9-d36a3b9c6cef".to_string())
    );
    assert_eq!(normalize_uuid("not-a-uuid"), None);
  }

  #[test]
  fn linux_serial_number_preserves_product_serial_and_formats_uuid_fallback() {
    let uuid = Some("5a784d56-6461-19ac-9ea9-d36a3b9c6cef".to_string());

    assert_eq!(
      resolve_linux_serial_number(Some("product-serial".to_string()), || uuid.clone()),
      Some("product-serial".to_string())
    );
    assert_eq!(
      resolve_linux_serial_number(None, || uuid),
      Some("VMware-56 4d 78 5a 61 64 ac 19-9e a9 d3 6a 3b 9c 6c ef".to_string())
    );
  }
}

impl RuntimeNativeHostIdentity {
  pub(super) fn versions(control: &dyn super::CollectionControl) -> super::io::Result<(String, String)> {
    control.check()?;
    Ok(Self::fallback_versions())
  }
  pub(super) fn fallback_versions() -> (String, String) {
    (
      format!(
        "Linux {}",
        whoami::distro().unwrap_or_else(|_| super::DEFAULT_LINUX_DISTRO.into())
      ),
      String::new(),
    )
  }
}
