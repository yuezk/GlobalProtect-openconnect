//! Launch a native worker with the admitted user's primary token and environment.
use std::{
  ffi::{OsStr, c_void},
  io,
  os::windows::{
    ffi::OsStrExt,
    io::{AsRawHandle, BorrowedHandle, FromRawHandle, OwnedHandle},
  },
  path::Path,
  ptr,
};
use windows_sys::Win32::{
  Foundation::{WAIT_OBJECT_0, WAIT_TIMEOUT},
  Globalization::{CSTR_EQUAL, CompareStringOrdinal},
  Security::{ImpersonateLoggedOnUser, RevertToSelf},
  System::{
    Environment::{CreateEnvironmentBlock, DestroyEnvironmentBlock},
    JobObjects::*,
    Threading::*,
  },
};

pub struct UserProcess {
  process: OwnedHandle,
  job: Option<OwnedHandle>,
  pid: u32,
}
struct Environment(*mut c_void);
impl Drop for Environment {
  fn drop(&mut self) {
    unsafe {
      DestroyEnvironmentBlock(self.0);
    }
  }
}
fn invalid(message: &str) -> io::Error {
  io::Error::new(io::ErrorKind::InvalidInput, message)
}
pub(super) fn wide(value: &OsStr) -> io::Result<Vec<u16>> {
  let value: Vec<_> = value.encode_wide().collect();
  if value.contains(&0) {
    return Err(invalid("Windows process strings cannot contain NUL"));
  }
  Ok(value)
}
fn quote(argument: &[u16], output: &mut Vec<u16>) {
  output.push(b'"' as u16);
  let mut backslashes = 0;
  for &character in argument {
    if character == b'\\' as u16 {
      backslashes += 1;
      continue;
    }
    let count = if character == b'"' as u16 {
      backslashes * 2 + 1
    } else {
      backslashes
    };
    output.extend(std::iter::repeat_n(b'\\' as u16, count));
    output.push(character);
    backslashes = 0;
  }
  output.extend(std::iter::repeat_n(b'\\' as u16, backslashes * 2));
  output.push(b'"' as u16);
}
pub(super) fn command_line<'a>(
  program: &[u16],
  arguments: impl IntoIterator<Item = &'a OsStr>,
) -> io::Result<Vec<u16>> {
  let mut output = Vec::new();
  quote(program, &mut output);
  for argument in arguments {
    output.push(b' ' as u16);
    quote(&wide(argument)?, &mut output);
  }
  output.push(0);
  if output.len() > 32767 {
    return Err(invalid("Windows worker command line is too long"));
  }
  Ok(output)
}
fn key(entry: &[u16]) -> &[u16] {
  let end = entry
    .iter()
    .enumerate()
    .find(|(index, value)| *index != 0 && **value == b'=' as u16)
    .map(|(index, _)| index)
    .unwrap_or(entry.len());
  &entry[..end]
}
fn compare(left: &[u16], right: &[u16]) -> std::cmp::Ordering {
  // Environment names follow Windows' ordinal, case-insensitive ordering.
  unsafe { CompareStringOrdinal(left.as_ptr(), left.len() as i32, right.as_ptr(), right.len() as i32, 1) }
    .cmp(&CSTR_EQUAL)
}
pub(super) fn user_environment<'a>(
  token: BorrowedHandle<'_>,
  variables: impl IntoIterator<Item = (&'a OsStr, &'a OsStr)>,
) -> io::Result<Vec<u16>> {
  let mut pointer = ptr::null_mut();
  if unsafe { CreateEnvironmentBlock(&mut pointer, token.as_raw_handle(), 0) } == 0 {
    return Err(io::Error::last_os_error());
  }
  let environment = Environment(pointer);
  let mut entries = Vec::new();
  unsafe {
    let mut current = environment.0.cast::<u16>();
    while *current != 0 {
      let mut length = 0;
      while *current.add(length) != 0 {
        length += 1;
      }
      entries.push(std::slice::from_raw_parts(current, length).to_vec());
      current = current.add(length + 1);
    }
  }
  for (name, value) in variables {
    let name = wide(name)?;
    if name.is_empty() || name.contains(&(b'=' as u16)) {
      return Err(invalid("Invalid worker environment name"));
    }
    let value = wide(value)?;
    entries.retain(|entry| compare(key(entry), &name) != std::cmp::Ordering::Equal);
    let mut entry = name;
    entry.push(b'=' as u16);
    entry.extend(value);
    entries.push(entry);
  }
  entries.sort_by(|left, right| compare(key(left), key(right)));
  let mut block = Vec::new();
  for entry in entries {
    block.extend(entry);
    block.push(0);
  }
  if block.is_empty() {
    block.push(0);
  }
  block.push(0);
  Ok(block)
}
impl UserProcess {
  /// No service handles or service environment are inherited. The user token
  /// determines the Windows session, including an existing interactive desktop.
  pub fn launch(
    token: BorrowedHandle<'_>,
    program: &Path,
    arguments: &[String],
    variables: &[(&str, &str)],
  ) -> io::Result<Self> {
    if !program.is_absolute() {
      return Err(invalid("Windows worker executable must be absolute"));
    }
    let mut executable = wide(program.as_os_str())?;
    let mut command = command_line(&executable, arguments.iter().map(OsStr::new))?;
    executable.push(0);
    let mut environment = user_environment(
      token,
      variables
        .iter()
        .map(|(name, value)| (OsStr::new(name), OsStr::new(value))),
    )?;
    let directory = program.parent().ok_or_else(|| invalid("Worker directory is missing"))?;
    let mut directory = wide(directory.as_os_str())?;
    directory.push(0);
    let job = unsafe { CreateJobObjectW(ptr::null(), ptr::null()) };
    if job.is_null() {
      return Err(io::Error::last_os_error());
    }
    let job = unsafe { OwnedHandle::from_raw_handle(job) };
    let limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION {
      BasicLimitInformation: JOBOBJECT_BASIC_LIMIT_INFORMATION {
        LimitFlags: JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
        ..Default::default()
      },
      ..Default::default()
    };
    if unsafe {
      SetInformationJobObject(
        job.as_raw_handle(),
        JobObjectExtendedLimitInformation,
        (&limits as *const JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast(),
        std::mem::size_of_val(&limits) as u32,
      )
    } == 0
    {
      return Err(io::Error::last_os_error());
    }
    let mut desktop: Vec<u16> = "winsta0\\default".encode_utf16().chain(Some(0)).collect();
    let startup = STARTUPINFOW {
      cb: std::mem::size_of::<STARTUPINFOW>() as u32,
      lpDesktop: desktop.as_mut_ptr(),
      ..Default::default()
    };
    let mut information = PROCESS_INFORMATION::default();
    // Read/execute the requested image as its user, as Unix exec-after-setuid
    // does. The service's filesystem access must not authorize a private image.
    if unsafe { ImpersonateLoggedOnUser(token.as_raw_handle()) } == 0 {
      return Err(io::Error::last_os_error());
    }
    struct Revert;
    impl Drop for Revert {
      fn drop(&mut self) {
        if unsafe { RevertToSelf() } == 0 {
          std::process::abort();
        }
      }
    }
    let revert = Revert;
    let created = unsafe {
      CreateProcessAsUserW(
        token.as_raw_handle(),
        executable.as_ptr(),
        command.as_mut_ptr(),
        ptr::null(),
        ptr::null(),
        0,
        CREATE_UNICODE_ENVIRONMENT | CREATE_SUSPENDED | CREATE_NO_WINDOW,
        environment.as_mut_ptr().cast(),
        directory.as_ptr(),
        &startup,
        &mut information,
      )
    };
    let error = (created == 0).then(io::Error::last_os_error);
    drop(revert);
    if let Some(error) = error {
      return Err(error);
    }
    let process = unsafe { OwnedHandle::from_raw_handle(information.hProcess) };
    let thread = unsafe { OwnedHandle::from_raw_handle(information.hThread) };
    if unsafe { AssignProcessToJobObject(job.as_raw_handle(), process.as_raw_handle()) } == 0 {
      let error = io::Error::last_os_error();
      // The suspended child is not owned by the job on this failure path.
      unsafe {
        TerminateProcess(process.as_raw_handle(), 1);
        WaitForSingleObject(process.as_raw_handle(), 5000);
      }
      return Err(error);
    }
    let child = Self {
      process,
      job: Some(job),
      pid: information.dwProcessId,
    };
    if unsafe { ResumeThread(thread.as_raw_handle()) } == u32::MAX {
      return Err(io::Error::last_os_error());
    }
    Ok(child)
  }
  pub fn id(&self) -> u32 {
    self.pid
  }
  pub fn has_exited(&self) -> io::Result<bool> {
    match unsafe { WaitForSingleObject(self.process.as_raw_handle(), 0) } {
      WAIT_OBJECT_0 => Ok(true),
      WAIT_TIMEOUT => Ok(false),
      _ => Err(io::Error::last_os_error()),
    }
  }
  pub fn stop(&mut self) -> io::Result<()> {
    if self.job.take().is_none() {
      return Ok(());
    }
    match unsafe { WaitForSingleObject(self.process.as_raw_handle(), 5000) } {
      WAIT_OBJECT_0 => Ok(()),
      WAIT_TIMEOUT => Err(io::Error::new(
        io::ErrorKind::TimedOut,
        "Windows worker did not terminate",
      )),
      _ => Err(io::Error::last_os_error()),
    }
  }
}
impl Drop for UserProcess {
  fn drop(&mut self) {
    if let Err(error) = self.stop() {
      log::warn!("Could not stop Windows worker: {error}");
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use windows_sys::Win32::{Foundation::LocalFree, UI::Shell::CommandLineToArgvW};
  #[test]
  fn command_line_round_trips_through_windows_parser() {
    let program: Vec<u16> = r"C:\Program Files\GP Connect\gpgui-nsh.exe".encode_utf16().collect();
    let arguments = [
      "proxy-worker",
      "",
      "space value",
      "quoted\"value",
      "two\\\\\"quotes",
      "trailing\\",
      "雪",
      "&|<>",
    ]
    .map(String::from);
    let command = command_line(&program, arguments.iter().map(OsStr::new)).unwrap();
    let mut count = 0;
    unsafe {
      let parsed = CommandLineToArgvW(command.as_ptr(), &mut count);
      assert!(!parsed.is_null());
      let result = std::slice::from_raw_parts(parsed, count as usize)
        .iter()
        .map(|&value| {
          let mut length = 0;
          while *value.add(length) != 0 {
            length += 1;
          }
          String::from_utf16_lossy(std::slice::from_raw_parts(value, length))
        })
        .collect::<Vec<_>>();
      LocalFree(parsed.cast());
      assert_eq!(result[0], String::from_utf16_lossy(&program));
      assert_eq!(result[1..], arguments);
    }
  }
}
