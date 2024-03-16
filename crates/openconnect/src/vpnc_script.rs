use is_executable::IsExecutable;
use std::path::Path;

const VPNC_SCRIPT_LOCATIONS: [&str; 6] = [
  "/usr/local/share/vpnc-scripts/vpnc-script",
  "/usr/local/sbin/vpnc-script",
  "/usr/share/vpnc-scripts/vpnc-script",
  "/usr/sbin/vpnc-script",
  "/etc/vpnc/vpnc-script",
  "/etc/openconnect/vpnc-script"
];

pub(crate) fn find_default_vpnc_script() -> Option<String> {
  for location in VPNC_SCRIPT_LOCATIONS.iter() {
    let path = Path::new(location);
    if path.is_executable() {
      return Some(location.to_string());
    }
  }

  log::warn!("vpnc-script not found");

  None
}
