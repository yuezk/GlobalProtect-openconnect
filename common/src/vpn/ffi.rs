use std::ffi::c_void;

#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub(crate) struct Options {
    pub server: *const ::std::os::raw::c_char,
    pub cookie: *const ::std::os::raw::c_char,
    pub script: *const ::std::os::raw::c_char,
    pub user_data: *mut c_void,
}

#[link(name = "vpn")]
extern "C" {
    #[link_name = "start"]
    pub(crate) fn connect(
        options: *const Options,
        on_connected: extern "C" fn(i32, *mut c_void),
    ) -> ::std::os::raw::c_int;

    #[link_name = "stop"]
    pub(crate) fn disconnect();
}
