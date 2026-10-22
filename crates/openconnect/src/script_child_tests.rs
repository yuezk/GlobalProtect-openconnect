use std::{
  ffi::{CString, c_char, c_int, c_void},
  process::Command,
  time::{Duration, Instant},
};

unsafe extern "C" {
  fn openconnect_vpninfo_new(
    agent: *const c_char,
    validate: Option<unsafe extern "C" fn(*mut c_void, *const c_char) -> c_int>,
    config: Option<unsafe extern "C" fn(*mut c_void, *const c_char, c_int) -> c_int>,
    auth: Option<unsafe extern "C" fn(*mut c_void, *mut c_void) -> c_int>,
    progress: Option<unsafe extern "C" fn(*mut c_void, c_int, *const c_char, ...)>,
    data: *mut c_void,
  ) -> *mut c_void;
  fn openconnect_setup_tun_script(info: *mut c_void, script: *const c_char) -> c_int;
  fn openconnect_vpninfo_free(info: *mut c_void);
  fn openconnect_set_loglevel(info: *mut c_void, level: c_int);
}

#[test]
fn script_children_are_reaped_including_term_resistant_children_without_reaping_others() {
  for resistant in [false, true] {
    let directory = tempfile::tempdir().unwrap();
    let pid_file = directory.path().join("pid");
    let behavior = if resistant {
      "trap '' TERM; echo $$ >"
    } else {
      "echo $$ >"
    };
    let suffix = if resistant {
      "; while :; do sleep 1; done"
    } else {
      "; exit 0"
    };
    let command = CString::new(format!("{behavior} '{}' {suffix}", pid_file.display())).unwrap();
    let info = unsafe { openconnect_vpninfo_new(c"test".as_ptr(), None, None, None, None, std::ptr::null_mut()) };
    assert!(!info.is_null());
    // This raw fixture has no progress callback; suppress native logging.
    unsafe { openconnect_set_loglevel(info, -1) };
    assert_eq!(unsafe { openconnect_setup_tun_script(info, command.as_ptr()) }, 0);
    let started = Instant::now();
    let pid: i32 = loop {
      if let Ok(text) = std::fs::read_to_string(&pid_file) {
        if let Ok(pid) = text.trim().parse() {
          break pid;
        }
      }
      assert!(started.elapsed() < Duration::from_secs(3));
      std::thread::sleep(Duration::from_millis(5));
    };
    let mut unrelated = Command::new("/bin/sh").args(["-c", "exit 17"]).spawn().unwrap();
    let stop = Instant::now();
    unsafe { openconnect_vpninfo_free(info) };
    assert!(stop.elapsed() < Duration::from_secs(4));
    assert_eq!(unsafe { libc::waitpid(pid, std::ptr::null_mut(), libc::WNOHANG) }, -1);
    assert_eq!(std::io::Error::last_os_error().raw_os_error(), Some(libc::ECHILD));
    assert_eq!(unrelated.wait().unwrap().code(), Some(17));
  }
}

unsafe extern "C" {
  fn openconnect_setup_tun_script_exec(info: *mut c_void, launch: *const crate::ffi::ScriptTunLaunchRaw) -> c_int;
}

// Invoked by the native launcher in a fresh process, after credentials and FDs change.
#[test]
fn identity_probe() {
  use std::{io::Write, os::fd::FromRawFd};
  let Ok(fd) = std::env::var("IDENTITY_PROBE_FD") else {
    return;
  };
  assert_eq!(std::env::var("OPENCONNECT_SCRIPT_TUN_CONTROL_FD").unwrap(), fd);
  assert_ne!(unsafe { libc::getuid() }, 0);
  assert_eq!(unsafe { libc::getuid() }, unsafe { libc::geteuid() });
  assert_eq!(unsafe { libc::getgid() }, unsafe { libc::getegid() });
  assert_eq!(unsafe { libc::setuid(0) }, -1);
  assert_eq!(unsafe { libc::fcntl(500, libc::F_GETFD) }, -1);
  let mut groups = vec![0; unsafe { libc::getgroups(0, std::ptr::null_mut()) } as usize];
  assert_eq!(
    unsafe { libc::getgroups(groups.len() as i32, groups.as_mut_ptr()) },
    groups.len() as i32
  );
  groups.sort();
  let expected = std::env::var("IDENTITY_PROBE_GROUPS").unwrap();
  assert_eq!(
    groups.iter().map(ToString::to_string).collect::<Vec<_>>().join(","),
    expected
  );
  let mut control = unsafe { std::os::unix::net::UnixStream::from_raw_fd(fd.parse().unwrap()) };
  control.write_all(b"identity verified").unwrap();
}

#[test]
fn identity_launcher_drops_credentials_and_excludes_unrelated_descriptors() {
  use std::{io::Read, os::fd::AsRawFd};
  let uid = std::env::var("GP_TEST_UID")
    .ok()
    .map(|s| s.parse().unwrap())
    .unwrap_or_else(|| unsafe { libc::getuid() });
  assert_ne!(uid, 0, "root runs must set GP_TEST_UID");
  let user = unsafe { libc::getpwuid(uid) };
  assert!(!user.is_null());
  let gid = unsafe { (*user).pw_gid };
  let mut groups = if unsafe { libc::geteuid() } == 0 {
    vec![gid]
  } else {
    let n = unsafe { libc::getgroups(0, std::ptr::null_mut()) };
    assert!(n >= 0);
    let mut groups = vec![0; n as usize];
    assert_eq!(unsafe { libc::getgroups(n, groups.as_mut_ptr()) }, n);
    groups
  };
  groups.sort();
  let groups: Vec<u32> = groups.into_iter().map(|g| g as u32).collect();
  let (mut parent, child) = std::os::unix::net::UnixStream::pair().unwrap();
  parent.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
  let unrelated = std::fs::File::open("/dev/null").unwrap();
  assert_eq!(unsafe { libc::dup2(unrelated.as_raw_fd(), 500) }, 500);
  let executable = std::env::current_exe().unwrap();
  let command = CString::new(format!("DYLD_FALLBACK_LIBRARY_PATH='{}' IDENTITY_PROBE_FD={} IDENTITY_PROBE_GROUPS='{}' exec '{}' --exact script_child_tests::identity_probe --nocapture", std::env::var("DYLD_FALLBACK_LIBRARY_PATH").unwrap_or_default(), child.as_raw_fd(), groups.iter().map(ToString::to_string).collect::<Vec<_>>().join(","), executable.display())).unwrap();
  let info = unsafe { openconnect_vpninfo_new(c"test".as_ptr(), None, None, None, None, std::ptr::null_mut()) };
  assert!(!info.is_null());
  unsafe { openconnect_set_loglevel(info, -1) };
  let argv = [c"/bin/sh".as_ptr(), c"-c".as_ptr(), command.as_ptr(), std::ptr::null()];
  let launch = crate::ffi::ScriptTunLaunchRaw {
    program: c"/bin/sh".as_ptr(),
    argv: argv.as_ptr(),
    uid,
    control_fd: child.as_raw_fd(),
  };
  let result = unsafe { openconnect_setup_tun_script_exec(info, &launch) };
  unsafe { libc::close(500) };
  assert_eq!(result, 0);
  drop(child);
  let mut response = [0; 17];
  parent.read_exact(&mut response).unwrap();
  assert_eq!(&response, b"identity verified");
  unsafe { openconnect_vpninfo_free(info) };
}
