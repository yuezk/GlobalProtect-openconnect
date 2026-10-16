//! Terminal integration runs in child test processes so descriptor/signal changes
//! cannot capture another test's output or change its controlling terminal.
use std::{
  fs::File,
  io::{self, Read, Write},
  os::unix::process::CommandExt,
  process::{Child, Command, Stdio},
  time::{Duration, Instant},
};

use nix::{
  fcntl::{FcntlArg, OFlag, fcntl},
  pty::{Winsize, openpty},
  sys::signal::{Signal, kill},
  unistd::Pid,
};

use super::*;

struct Fixture {
  child: Child,
  master: File,
  slave: File,
  output: Vec<u8>,
}

impl Fixture {
  fn start(piped: bool) -> Self {
    Self::start_mode(if piped { "piped" } else { "terminal" }, piped)
  }

  fn start_mode(mode: &str, piped: bool) -> Self {
    let mut command = Command::new(std::env::current_exe().unwrap());
    command
      .args([
        "--exact",
        "dashboard::tests::terminal_fixture",
        "--ignored",
        "--nocapture",
      ])
      .env("GPCLIENT_DASHBOARD_FIXTURE", mode);
    Self::spawn(command, piped)
  }

  fn spawn(mut command: Command, piped: bool) -> Self {
    let pty = openpty(
      Some(&Winsize {
        ws_row: 40,
        ws_col: 100,
        ws_xpixel: 0,
        ws_ypixel: 0,
      }),
      None,
    )
    .unwrap();
    let master = File::from(pty.master);
    let slave = File::from(pty.slave);
    fcntl(&master, FcntlArg::F_SETFL(OFlag::O_NONBLOCK)).unwrap();
    command
      .env("TERM", "xterm-256color")
      .stdin(if piped {
        Stdio::piped()
      } else {
        Stdio::from(slave.try_clone().unwrap())
      })
      .stdout(if piped {
        Stdio::piped()
      } else {
        Stdio::from(slave.try_clone().unwrap())
      })
      .stderr(Stdio::from(slave.try_clone().unwrap()));
    unsafe {
      command.pre_exec(|| {
        if nix::libc::setsid() < 0 {
          return Err(io::Error::last_os_error());
        }
        if nix::libc::ioctl(2, nix::libc::TIOCSCTTY as _, 0) < 0 {
          return Err(io::Error::last_os_error());
        }
        let no_core = nix::libc::rlimit {
          rlim_cur: 0,
          rlim_max: 0,
        };
        nix::libc::setrlimit(nix::libc::RLIMIT_CORE, &no_core);
        Ok(())
      });
    }
    let mut child = command.spawn().unwrap();
    if piped {
      child.stdin.take().unwrap().write_all(b"fixture-cookie\n").unwrap();
    }
    Self {
      child,
      master,
      slave,
      output: Vec::new(),
    }
  }

  fn drain(&mut self) {
    let mut bytes = [0; 8192];
    loop {
      match self.master.read(&mut bytes) {
        Ok(0) => break,
        Ok(length) => self.output.extend_from_slice(&bytes[..length]),
        Err(error) if error.kind() == io::ErrorKind::WouldBlock => break,
        Err(error) if error.raw_os_error() == Some(nix::libc::EIO) => break,
        Err(error) => panic!("PTY read: {error}"),
      }
    }
  }

  fn wait_text(&mut self, text: &str) {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
      self.drain();
      let plain = strip_ansi_escapes::strip(&self.output);
      if String::from_utf8_lossy(&plain).contains(text) {
        return;
      }
      if let Some(status) = self.child.try_wait().unwrap() {
        panic!(
          "Fixture exited {status} before {text:?}: {}",
          String::from_utf8_lossy(&self.output)
        );
      }
      assert!(
        Instant::now() < deadline,
        "Missing {text:?}: {}",
        String::from_utf8_lossy(&self.output)
      );
      std::thread::sleep(Duration::from_millis(20));
    }
  }

  fn finish(&mut self) {
    self.wait_text("DASHBOARD_RESTORED");
    let attrs = nix::sys::termios::tcgetattr(&self.slave).unwrap();
    assert!(attrs.local_flags.contains(nix::sys::termios::LocalFlags::ICANON));
    assert!(attrs.local_flags.contains(nix::sys::termios::LocalFlags::ECHO));
    self.master.write_all(b"\n").unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
      self.drain();
      if let Some(status) = self.child.try_wait().unwrap() {
        assert!(
          status.success(),
          "Fixture failed {status}: {}",
          String::from_utf8_lossy(&self.output)
        );
        self.drain();
        break;
      }
      assert!(Instant::now() < deadline, "Fixture did not restore/exit");
      std::thread::sleep(Duration::from_millis(20));
    }
    assert!(String::from_utf8_lossy(&self.output).contains("DASHBOARD_RESTORED"));
  }
}

impl Drop for Fixture {
  fn drop(&mut self) {
    // A failed assertion must not leave a suspended child or its helper alive.
    let _ = kill(Pid::from_raw(self.child.id() as i32), Signal::SIGCONT);
    let _ = self.child.kill();
    let _ = self.child.wait();
  }
}

#[test]
fn panel_switches_to_captured_child_logs_and_ctrl_c_restores_terminal() {
  let mut fixture = Fixture::start(false);
  fixture.wait_text("Internal");
  fixture.master.write_all(b"\t").unwrap();
  fixture.wait_text("Live logs");
  fixture.wait_text("child-stderr-after-entry");
  fixture.master.write_all(b"\x03").unwrap();
  fixture.finish();
}

#[test]
fn warning_before_first_tunnel_frame_remains_visible() {
  let mut fixture = Fixture::start_mode("tunnel", false);
  fixture.wait_text("initial-fallback-warning");
  fixture.master.write_all(b"\x03").unwrap();
  fixture.finish();
}

#[test]
fn connected_tunnel_samples_statistics_and_accepts_ctrl_c() {
  let mut fixture = Fixture::start_mode("active-tunnel", false);
  fixture.wait_text("IPsec");
  fixture.wait_text("(ESP)");
  fixture.wait_text("Received");
  fixture.wait_text("4.0");
  fixture.master.write_all(b"\x03").unwrap();
  fixture.finish();
}

#[test]
fn authenticated_terminal_input_remains_usable_after_dashboard_entry() {
  let mut fixture = Fixture::start_mode("prompted-tunnel", false);
  fixture.wait_text("Received");
  fixture.master.write_all(b"\t").unwrap();
  fixture.wait_text("Live logs");
  fixture.master.write_all(b"\x03").unwrap();
  fixture.finish();
}

#[test]
fn piped_credentials_and_stdout_redirection_preserve_their_streams() {
  let mut fixture = Fixture::start(true);
  fixture.wait_text("Internal");
  fixture.master.write_all(b"\x03").unwrap();
  fixture.finish();
  let mut output = String::new();
  fixture
    .child
    .stdout
    .take()
    .unwrap()
    .read_to_string(&mut output)
    .unwrap();
  assert!(output.contains("redirected-stdout"));
  assert!(!output.contains('\x1b'));
}

#[test]
fn external_termination_and_suspend_resume_restore_terminal() {
  let mut fixture = Fixture::start(false);
  fixture.wait_text("Internal");
  kill(Pid::from_raw(fixture.child.id() as i32), Signal::SIGTSTP).unwrap();
  let deadline = Instant::now() + Duration::from_secs(5);
  loop {
    let attrs = nix::sys::termios::tcgetattr(&fixture.slave).unwrap();
    if attrs.local_flags.contains(nix::sys::termios::LocalFlags::ICANON) {
      break;
    }
    assert!(Instant::now() < deadline, "suspend did not restore termios");
    std::thread::sleep(Duration::from_millis(20));
  }
  kill(Pid::from_raw(fixture.child.id() as i32), Signal::SIGCONT).unwrap();
  std::thread::sleep(Duration::from_millis(200));
  kill(Pid::from_raw(fixture.child.id() as i32), Signal::SIGTERM).unwrap();
  fixture.finish();
}

#[test]
fn actual_panic_hook_restores_terminal_with_panic_abort() {
  let dependencies = std::env::current_exe().unwrap().parent().unwrap().to_path_buf();
  let nix = std::fs::read_dir(&dependencies)
    .unwrap()
    .filter_map(Result::ok)
    .map(|entry| entry.path())
    .find(|path| {
      path.file_name().unwrap().to_string_lossy().starts_with("libnix-")
        && path.extension().is_some_and(|ext| ext == "rlib")
    })
    .expect("compiled nix rlib");
  let directory = tempfile::tempdir().unwrap();
  let binary = directory.path().join("dashboard-abort");
  let build = Command::new("rustc")
    .args(["--edition=2024", "-C", "panic=abort", "-A", "dead_code"])
    .arg(format!(
      "{}/tests/fixtures/dashboard_abort.rs",
      env!("CARGO_MANIFEST_DIR")
    ))
    .arg("--extern")
    .arg(format!("nix={}", nix.display()))
    .arg("-L")
    .arg(format!("dependency={}", dependencies.display()))
    .arg("-o")
    .arg(&binary)
    .output()
    .unwrap();
  assert!(
    build.status.success(),
    "Abort fixture compilation failed: {}",
    String::from_utf8_lossy(&build.stderr)
  );
  let mut fixture = Fixture::spawn(Command::new(binary), false);
  fixture.wait_text("ABORT_FIXTURE_READY");
  fixture.master.write_all(b"x").unwrap();
  fixture.wait_text("ABORT_HOOK_RESTORED");
  let attrs = nix::sys::termios::tcgetattr(&fixture.slave).unwrap();
  assert!(attrs.local_flags.contains(nix::sys::termios::LocalFlags::ICANON));
  assert!(attrs.local_flags.contains(nix::sys::termios::LocalFlags::ECHO));
  fixture.master.write_all(b"\n").unwrap();
  let deadline = Instant::now() + Duration::from_secs(5);
  loop {
    fixture.drain();
    if let Some(status) = fixture.child.try_wait().unwrap() {
      assert!(!status.success(), "panic=abort fixture unexpectedly succeeded");
      break;
    }
    assert!(Instant::now() < deadline, "abort fixture did not exit");
    std::thread::sleep(Duration::from_millis(20));
  }
  fixture.drain();
  assert!(String::from_utf8_lossy(&fixture.output).contains("intentional abort-hook fixture panic"));
}

#[test]
#[ignore = "runs in an isolated PTY subprocess"]
fn terminal_fixture() {
  let mode = std::env::var("GPCLIENT_DASHBOARD_FIXTURE").expect("subprocess fixture only");
  if mode == "active-tunnel" || mode == "prompted-tunnel" {
    env_logger::builder()
      .filter_level(log::LevelFilter::Info)
      .try_init()
      .unwrap();
  }
  if mode == "piped" {
    let mut credentials = String::new();
    std::io::stdin().read_to_string(&mut credentials).unwrap();
    assert_eq!(credentials, "fixture-cookie\n");
  }
  validate(LogFormat::Text).unwrap();
  if mode == "prompted-tunnel" {
    // Authentication prompts use Crossterm before dashboard output routing.
    // Exercise its real global reader initialization while stdin is the TTY.
    let _ = crossterm::event::poll(Duration::from_millis(1)).unwrap();
  }
  let runtime = tokio::runtime::Runtime::new().unwrap();
  runtime.block_on(async {
    let cancellation = CancellationToken::new();
    let signal_cancel = cancellation.clone();
    let signals = tokio::spawn(async move {
      gpapi::utils::shutdown_signal().await;
      signal_cancel.cancel();
    });
    let input_flags = fcntl(std::io::stdin(), FcntlArg::F_GETFD).unwrap();
    let mut dashboard = Dashboard::prepare("vpn.example.com".into(), cancellation.clone()).unwrap();
    eprintln!("child-stderr-before-entry");
    // Native VPN helpers exec /bin/sh without Rust's standard-fd repair.
    let mut child = Command::new("/bin/sh")
      .args([
        "-c",
        r#"
        sleep 1
        if test -t 0; then exit 1; fi
        if read -r input 2>/dev/null; then exit 1; fi
        printf '%s\n' child-stderr-after-entry >&2
      "#,
      ])
      .spawn()
      .unwrap();
    let members = InternalMembers::default();
    members
      .lock()
      .unwrap()
      .insert("internal.example.com".into(), "internal.example.com: Healthy".into());
    let mut native = None;
    let monitor = if mode == "tunnel" || mode == "active-tunnel" || mode == "prompted-tunnel" {
      let vpn = Arc::new(
        openconnect::Vpn::builder("vpn.example.com", "fixture-cookie")
          .network_configuration_handler(|_| Ok(()))
          .build()
          .unwrap(),
      );
      let monitor = gptunnel::TunnelMonitor::attach(vpn.clone());
      if mode == "active-tunnel" || mode == "prompted-tunnel" {
        native = Some(active_native_fixture(vpn, cancellation.clone()));
      }
      let _ = notice::slot();
      session_notice(log::Level::Warn, "initial-fallback-warning");
      dashboard.start_tunnel(monitor.clone()).unwrap();
      Some(monitor)
    } else {
      dashboard.start_internal(members).unwrap();
      None
    };
    println!("redirected-stdout");
    tokio::time::timeout(Duration::from_secs(15), cancellation.cancelled())
      .await
      .unwrap();
    assert!(
      child.wait().unwrap().success(),
      "helper must not inherit dashboard input"
    );
    if let Some(monitor) = monitor {
      monitor.stop().await;
    }
    if let Some(native) = native {
      native.join().unwrap();
    }
    dashboard.finish().unwrap();
    assert_eq!(fcntl(std::io::stdin(), FcntlArg::F_GETFD).unwrap(), input_flags);
    signals.abort();
    eprintln!("DASHBOARD_RESTORED");
    let mut acknowledgement = String::new();
    let mut tty = std::fs::OpenOptions::new().read(true).open("/dev/tty").unwrap();
    use std::io::BufRead;
    std::io::BufReader::new(&mut tty)
      .read_line(&mut acknowledgement)
      .unwrap();
  });
}

fn active_native_fixture(vpn: Arc<openconnect::Vpn>, cancellation: CancellationToken) -> std::thread::JoinHandle<()> {
  use std::{
    ffi::{c_int, c_void},
    os::fd::AsRawFd,
    os::unix::net::UnixStream,
  };
  #[repr(C)]
  struct Statistics {
    tx_pkts: u64,
    tx_bytes: u64,
    rx_pkts: u64,
    rx_bytes: u64,
  }
  unsafe extern "C" {
    fn vpn_attach_command_pipe(vpn: *mut c_void, fd: c_int) -> c_int;
    fn vpn_detach_command_pipe(vpn: *mut c_void);
    fn vpn_tunnel_event(vpn: *mut c_void, event: c_int);
    fn vpn_tunnel_statistics(vpn: *mut c_void, statistics: *const Statistics);
  }
  let (sender, mut receiver) = UnixStream::pair().unwrap();
  receiver.set_nonblocking(true).unwrap();
  let pointer = Arc::as_ptr(&vpn).cast_mut().cast();
  // SAFETY: the fixture retains Vpn and both endpoints through detach; event
  // numbers and statistics layout match the native bridge used in production.
  unsafe {
    assert_eq!(vpn_attach_command_pipe(pointer, sender.as_raw_fd()), 0);
    vpn_tunnel_event(pointer, 1);
    vpn_tunnel_event(pointer, 5);
  }
  std::thread::spawn(move || {
    let pointer = Arc::as_ptr(&vpn).cast_mut().cast();
    let mut command = [0];
    while !cancellation.is_cancelled() {
      match receiver.read(&mut command) {
        Ok(1) if command[0] == b's' => unsafe {
          vpn_tunnel_statistics(
            pointer,
            &Statistics {
              tx_pkts: 2,
              tx_bytes: 1024,
              rx_pkts: 4,
              rx_bytes: 4096,
            },
          );
        },
        Err(error) if error.kind() == io::ErrorKind::WouldBlock => std::thread::sleep(Duration::from_millis(10)),
        other => panic!("Unexpected native fixture command: {other:?}"),
      }
    }
    // SAFETY: all native observations finish before the retained Vpn is freed.
    unsafe {
      vpn_detach_command_pipe(pointer);
      vpn_tunnel_event(pointer, 7);
    }
    drop(sender);
  })
}
