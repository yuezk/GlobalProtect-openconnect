use log::{debug, info, trace, warn};
use std::ffi::c_void;
use tokio::sync::mpsc;

#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub(crate) struct Options {
    pub server: *const std::os::raw::c_char,
    pub cookie: *const std::os::raw::c_char,
    pub script: *const std::os::raw::c_char,
    pub user_agent: *const std::os::raw::c_char,
    pub user_data: *mut c_void,
}

#[link(name = "vpn")]
extern "C" {
    #[link_name = "vpn_connect"]
    pub(crate) fn connect(options: *const Options) -> std::os::raw::c_int;

    #[link_name = "vpn_disconnect"]
    pub(crate) fn disconnect();
}

#[no_mangle]
extern "C" fn on_vpn_connected(value: i32, sender: *mut c_void) {
    let sender = unsafe { &*(sender as *const mpsc::Sender<i32>) };
    sender
        .blocking_send(value)
        .expect("Failed to send VPN connection code");
}

// Logger used in the C code.
// level: 0 = error, 1 = info, 2 = debug, 3 = trace
// map the error level log in openconnect to the warning level
#[no_mangle]
extern "C" fn vpn_log(level: i32, message: *const std::os::raw::c_char) {
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
