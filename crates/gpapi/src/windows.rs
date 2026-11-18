//! Windows user identity and private local IPC.
use std::os::windows::io::{AsHandle, AsRawHandle, BorrowedHandle, FromRawHandle, OwnedHandle};
use std::{io, ptr};
mod pipes;
pub use pipes::{PacketPipe, connect_service_admission, connect_service_worker, create_service_admission_pipe};
use tokio::net::windows::named_pipe::NamedPipeServer;
use windows_sys::Win32::{
  Foundation::{CloseHandle, LocalFree},
  Security::{Authorization::ConvertSidToStringSidW, GetTokenInformation, TOKEN_QUERY, TOKEN_USER, TokenUser},
  System::Threading::{GetCurrentProcess, OpenProcess, OpenProcessToken, PROCESS_QUERY_LIMITED_INFORMATION},
};

pub fn current_user_sid() -> io::Result<String> {
  unsafe {
    let mut token = ptr::null_mut();
    if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) == 0 {
      return Err(io::Error::last_os_error());
    }
    let result = token_user_sid(token);
    CloseHandle(token);
    result
  }
}

/// An authenticated pipe client's primary token, retained for its user-session worker.
/// Windows user identity is a SID and token; it is never represented as a Unix UID.
pub struct PipeClientIdentity {
  sid: String,
  token: OwnedHandle,
}

impl PipeClientIdentity {
  pub fn sid(&self) -> &str {
    &self.sid
  }

  pub fn primary_token(&self) -> BorrowedHandle<'_> {
    self.token.as_handle()
  }

  /// Call after reading the client's admission request. Impersonation stays on
  /// this thread and ends before returning; no asynchronous work can run in it.
  pub fn from_pipe(pipe: &NamedPipeServer) -> io::Result<Self> {
    use windows_sys::Win32::{
      Security::{
        DuplicateTokenEx, RevertToSelf, SecurityImpersonation, TOKEN_ASSIGN_PRIMARY, TOKEN_DUPLICATE, TokenPrimary,
      },
      System::{
        Pipes::ImpersonateNamedPipeClient,
        Threading::{GetCurrentThread, OpenThreadToken},
      },
    };
    struct Revert;
    impl Drop for Revert {
      fn drop(&mut self) {
        // Continuing a service thread as its client would corrupt its privilege
        // boundary. Windows documents terminating if RevertToSelf fails.
        if unsafe { RevertToSelf() } == 0 {
          std::process::abort();
        }
      }
    }
    unsafe {
      if ImpersonateNamedPipeClient(pipe.as_raw_handle()) == 0 {
        return Err(io::Error::last_os_error());
      }
      let _revert = Revert;
      let mut token = ptr::null_mut();
      if OpenThreadToken(GetCurrentThread(), TOKEN_QUERY | TOKEN_DUPLICATE, 1, &mut token) == 0 {
        return Err(io::Error::last_os_error());
      }
      let token = OwnedHandle::from_raw_handle(token);
      let sid = token_user_sid(token.as_raw_handle())?;
      let mut primary = ptr::null_mut();
      if DuplicateTokenEx(
        token.as_raw_handle(),
        TOKEN_QUERY | TOKEN_DUPLICATE | TOKEN_ASSIGN_PRIMARY,
        ptr::null(),
        SecurityImpersonation,
        TokenPrimary,
        &mut primary,
      ) == 0
      {
        return Err(io::Error::last_os_error());
      }
      Ok(Self {
        sid,
        token: OwnedHandle::from_raw_handle(primary),
      })
    }
  }
}

/// Resolve a local IPC peer from its OS process token.
pub fn process_user_sid(pid: u32) -> io::Result<String> {
  unsafe {
    let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
    if process.is_null() {
      return Err(io::Error::last_os_error());
    }
    let mut token = ptr::null_mut();
    if OpenProcessToken(process, TOKEN_QUERY, &mut token) == 0 {
      let error = io::Error::last_os_error();
      CloseHandle(process);
      return Err(error);
    }
    CloseHandle(process);
    let result = token_user_sid(token);
    CloseHandle(token);
    result
  }
}

unsafe fn token_user_sid(token: windows_sys::Win32::Foundation::HANDLE) -> io::Result<String> {
  unsafe {
    let mut length = 0;
    GetTokenInformation(token, TokenUser, ptr::null_mut(), 0, &mut length);
    // TOKEN_USER contains pointers; retain pointer alignment in its backing allocation.
    let mut buffer = vec![0usize; (length as usize).div_ceil(std::mem::size_of::<usize>())];
    if GetTokenInformation(token, TokenUser, buffer.as_mut_ptr().cast(), length, &mut length) == 0 {
      return Err(io::Error::last_os_error());
    }
    let user = &*buffer.as_ptr().cast::<TOKEN_USER>();
    let mut text = ptr::null_mut();
    if ConvertSidToStringSidW(user.User.Sid, &mut text) == 0 {
      return Err(io::Error::last_os_error());
    }
    let mut length = 0;
    while *text.add(length) != 0 {
      length += 1;
    }
    let sid = String::from_utf16_lossy(std::slice::from_raw_parts(text, length));
    LocalFree(text.cast());
    Ok(sid)
  }
}

/// Only the owning user and SYSTEM can access authentication callbacks.
pub fn create_user_pipe(name: &str, first_instance: bool) -> io::Result<NamedPipeServer> {
  pipes::user_pipe(&current_user_sid()?, name, first_instance)
}

/// Query a registered service without requiring access to its SYSTEM process token.
pub fn service_process_id(name: &str) -> io::Result<u32> {
  use windows_sys::Win32::System::Services::*;
  let name: Vec<u16> = name.encode_utf16().chain(Some(0)).collect();
  unsafe {
    let manager = OpenSCManagerW(ptr::null(), ptr::null(), SC_MANAGER_CONNECT);
    if manager.is_null() {
      return Err(io::Error::last_os_error());
    }
    let service = OpenServiceW(manager, name.as_ptr(), SERVICE_QUERY_STATUS);
    if service.is_null() {
      let error = io::Error::last_os_error();
      CloseServiceHandle(manager);
      return Err(error);
    }
    let mut status = SERVICE_STATUS_PROCESS::default();
    let mut length = 0;
    let success = QueryServiceStatusEx(
      service,
      SC_STATUS_PROCESS_INFO,
      (&mut status as *mut SERVICE_STATUS_PROCESS).cast(),
      std::mem::size_of_val(&status) as u32,
      &mut length,
    );
    let result = if success == 0 {
      Err(io::Error::last_os_error())
    } else {
      Ok(status.dwProcessId)
    };
    CloseServiceHandle(service);
    CloseServiceHandle(manager);
    result
  }
}
