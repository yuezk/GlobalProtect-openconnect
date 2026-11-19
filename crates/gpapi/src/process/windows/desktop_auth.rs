//! Run the CLI's installed authenticator with the interactive user's standard token.
use std::{
  env,
  fs::File,
  io,
  os::windows::{
    io::{AsHandle, AsRawHandle, FromRawHandle, OwnedHandle},
    process::ExitStatusExt,
  },
  path::Path,
  process::{Command, ExitStatus},
  ptr,
};
use windows_sys::Win32::{
  Foundation::{HANDLE, HANDLE_FLAG_INHERIT, SetHandleInformation, WAIT_OBJECT_0, WAIT_TIMEOUT},
  Security::{
    DuplicateTokenEx, SECURITY_ATTRIBUTES, SecurityImpersonation, TOKEN_DUPLICATE, TOKEN_QUERY, TokenElevation,
    TokenPrimary, TokenSessionId,
  },
  System::{Pipes::CreatePipe, SystemServices::MAXIMUM_ALLOWED, Threading::*},
  UI::WindowsAndMessaging::{GetShellWindow, GetWindowThreadProcessId},
};

use super::{
  auth_bootstrap::AuthBootstrap,
  command_runner::ChildInput,
  console_input::{self, ConsoleInput},
  user_process::{command_line, user_environment, wide},
};
use crate::{
  collection::CollectionControl,
  windows::{current_user_sid, token_u32, token_user_sid},
};

pub(super) struct DesktopChild {
  process: OwnedHandle,
  console_input: Option<ConsoleInput>,
}

impl DesktopChild {
  pub(super) fn spawn(
    command: &Command,
    bootstrap: AuthBootstrap,
    input: &ChildInput<'_>,
    control: &dyn CollectionControl,
  ) -> io::Result<(Self, File, File, Option<File>)> {
    let desktop = desktop_token()?;
    let program = Path::new(command.get_program());
    if !program.is_absolute() {
      return Err(io::Error::new(
        io::ErrorKind::InvalidInput,
        "The authenticator path must be absolute",
      ));
    }
    let mut executable = wide(program.as_os_str())?;
    let mut arguments = command_line(&executable, bootstrap.arguments())?;
    executable.push(0);
    let mut environment = user_environment(
      desktop.as_handle(),
      command
        .get_envs()
        .filter_map(|(name, value)| value.map(|value| (name, value))),
    )?;
    let working_directory = match command.get_current_dir() {
      Some(directory) => directory.to_path_buf(),
      None => env::current_dir()?,
    };
    let mut directory = wide(working_directory.as_os_str())?;
    directory.push(0);
    let (stdout, child_out) = pipe(false)?;
    let (stderr, child_err) = pipe(false)?;
    let mut console = None;
    let (child_in, input_pipe) = match input {
      ChildInput::Terminal => {
        let terminal = console_input::terminal_input()?;
        if console_input::is_console(&terminal) {
          let (writer, reader) = pipe(true)?;
          console = Some((terminal, File::from(writer)));
          (reader, None)
        } else {
          (terminal, None)
        }
      }
      ChildInput::Null => {
        let (writer, reader) = pipe(true)?;
        drop(writer);
        (reader, None)
      }
      ChildInput::Frame(_) => {
        let (writer, reader) = pipe(true)?;
        (reader, Some(File::from(writer)))
      }
    };
    let startup = STARTUPINFOW {
      cb: std::mem::size_of::<STARTUPINFOW>() as u32,
      dwFlags: STARTF_USESTDHANDLES,
      hStdInput: child_in.as_raw_handle(),
      hStdOutput: child_out.as_raw_handle(),
      hStdError: child_err.as_raw_handle(),
      ..Default::default()
    };
    let mut process = PROCESS_INFORMATION::default();
    let mut flags = CREATE_UNICODE_ENVIRONMENT;
    if !matches!(input, ChildInput::Terminal) {
      flags |= CREATE_NO_WINDOW;
    }
    if unsafe {
      CreateProcessWithTokenW(
        desktop.as_raw_handle(),
        0,
        executable.as_ptr(),
        arguments.as_mut_ptr(),
        flags,
        environment.as_mut_ptr().cast(),
        directory.as_ptr(),
        &startup,
        &mut process,
      )
    } == 0
    {
      return Err(io::Error::last_os_error());
    }
    let mut child = Self {
      process: unsafe { OwnedHandle::from_raw_handle(process.hProcess) },
      console_input: None,
    };
    drop(unsafe { OwnedHandle::from_raw_handle(process.hThread) });
    bootstrap.send_to_child(process.dwProcessId, control)?;
    child.console_input = console
      .map(|(terminal, writer)| ConsoleInput::start(terminal, writer))
      .transpose()?;
    Ok((child, File::from(stdout), File::from(stderr), input_pipe))
  }

  pub(super) fn try_wait(&mut self) -> io::Result<Option<ExitStatus>> {
    if let Some(input) = &mut self.console_input {
      input.check()?;
    }
    match unsafe { WaitForSingleObject(self.process.as_raw_handle(), 0) } {
      WAIT_OBJECT_0 => {
        let mut code = 0;
        if unsafe { GetExitCodeProcess(self.process.as_raw_handle(), &mut code) } == 0 {
          return Err(io::Error::last_os_error());
        }
        Ok(Some(ExitStatus::from_raw(code)))
      }
      WAIT_TIMEOUT => Ok(None),
      _ => Err(io::Error::last_os_error()),
    }
  }
}

impl Drop for DesktopChild {
  fn drop(&mut self) {
    unsafe {
      if WaitForSingleObject(self.process.as_raw_handle(), 0) == WAIT_TIMEOUT
        && TerminateProcess(self.process.as_raw_handle(), 1) != 0
      {
        WaitForSingleObject(self.process.as_raw_handle(), INFINITE);
      }
    }
  }
}

fn desktop_token() -> io::Result<OwnedHandle> {
  let mut pid = 0;
  let window = unsafe { GetShellWindow() };
  if window.is_null() || unsafe { GetWindowThreadProcessId(window, &mut pid) } == 0 {
    return Err(io::Error::other("Sign-in requires an interactive Windows desktop"));
  }
  let process = owned(unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) })?;
  let mut token = ptr::null_mut();
  if unsafe { OpenProcessToken(process.as_raw_handle(), TOKEN_QUERY | TOKEN_DUPLICATE, &mut token) } == 0 {
    return Err(io::Error::last_os_error());
  }
  let source = unsafe { OwnedHandle::from_raw_handle(token) };
  if unsafe { token_user_sid(token) }? != current_user_sid()?
    || token_u32(source.as_handle(), TokenElevation)? != 0
    || token_u32(source.as_handle(), TokenSessionId)? != session_id()?
  {
    return Err(io::Error::other(
      "Sign-in requires the terminal user's standard Windows desktop",
    ));
  }
  let mut primary = ptr::null_mut();
  if unsafe {
    // A linked token queried without SeTcbPrivilege may only be usable for identification.
    // The shell already has the primary standard token in the interactive session.
    DuplicateTokenEx(
      token,
      MAXIMUM_ALLOWED,
      ptr::null(),
      SecurityImpersonation,
      TokenPrimary,
      &mut primary,
    )
  } == 0
  {
    return Err(io::Error::last_os_error());
  }
  owned(primary)
}

fn session_id() -> io::Result<u32> {
  let mut session = 0;
  if unsafe { windows_sys::Win32::System::RemoteDesktop::ProcessIdToSessionId(std::process::id(), &mut session) } == 0 {
    return Err(io::Error::last_os_error());
  }
  Ok(session)
}

fn owned(handle: HANDLE) -> io::Result<OwnedHandle> {
  if handle.is_null() || handle == windows_sys::Win32::Foundation::INVALID_HANDLE_VALUE {
    return Err(io::Error::last_os_error());
  }
  Ok(unsafe { OwnedHandle::from_raw_handle(handle) })
}

/// Returns the parent's non-inheritable end, then the child's end.
fn pipe(parent_writes: bool) -> io::Result<(OwnedHandle, OwnedHandle)> {
  let attributes = SECURITY_ATTRIBUTES {
    nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
    bInheritHandle: 1,
    lpSecurityDescriptor: ptr::null_mut(),
  };
  let mut read = ptr::null_mut();
  let mut write = ptr::null_mut();
  if unsafe { CreatePipe(&mut read, &mut write, &attributes, 0) } == 0 {
    return Err(io::Error::last_os_error());
  }
  let read = unsafe { OwnedHandle::from_raw_handle(read) };
  let write = unsafe { OwnedHandle::from_raw_handle(write) };
  let (parent, child) = if parent_writes { (write, read) } else { (read, write) };
  if unsafe { SetHandleInformation(parent.as_raw_handle(), HANDLE_FLAG_INHERIT, 0) } == 0 {
    return Err(io::Error::last_os_error());
  }
  Ok((parent, child))
}
