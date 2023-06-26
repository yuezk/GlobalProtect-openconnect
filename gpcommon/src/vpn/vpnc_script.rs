use std::path::Path;

use is_executable::IsExecutable;

const VPNC_SCRIPT_LOCATIONS: [&str; 4] = [
    "/usr/local/share/vpnc-scripts/vpnc-script",
    "/usr/local/sbin/vpnc-script",
    "/usr/share/vpnc-scripts/vpnc-script",
    "/usr/sbin/vpnc-script /etc/vpnc/vpnc-script",
];

pub(crate) fn find_default_vpnc_script() -> Option<&'static str> {
    for location in VPNC_SCRIPT_LOCATIONS.iter() {
        let path = Path::new(location);
        if path.is_executable() {
            return Some(location);
        }
    }

    None
}
