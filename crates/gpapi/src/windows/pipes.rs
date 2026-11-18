//! Local service admission and worker pipes with explicit user access.
use super::PipeClientIdentity;
use std::{
  fs::OpenOptions,
  io,
  os::windows::{
    fs::OpenOptionsExt,
    io::{AsRawHandle, FromRawHandle, IntoRawHandle, OwnedHandle},
  },
  ptr,
  time::Duration,
};
use tokio::net::windows::named_pipe::{NamedPipeClient, NamedPipeServer, ServerOptions};
use windows_sys::Win32::{
  Foundation::{ERROR_IO_PENDING, ERROR_PIPE_CONNECTED, INVALID_HANDLE_VALUE, LocalFree, WAIT_OBJECT_0, WAIT_TIMEOUT},
  Security::{Authorization::ConvertStringSecurityDescriptorToSecurityDescriptorW, SECURITY_ATTRIBUTES},
  Storage::FileSystem::*,
  System::{
    IO::{CancelIoEx, GetOverlappedResult, OVERLAPPED},
    Pipes::*,
    Threading::{CreateEventW, WaitForSingleObject},
  },
};

// Generic write also grants FILE_CREATE_PIPE_INSTANCE. Clients receive only
// data/metadata access; creating another instance remains SYSTEM-only.
const CLIENT_ACCESS: u32 = FILE_GENERIC_READ | FILE_WRITE_DATA | FILE_WRITE_EA | FILE_WRITE_ATTRIBUTES;
fn wide(value: &str) -> Vec<u16> {
  value.encode_utf16().chain(Some(0)).collect()
}
fn with_security<T>(sddl: &str, operation: impl FnOnce(&SECURITY_ATTRIBUTES) -> io::Result<T>) -> io::Result<T> {
  struct Descriptor(*mut std::ffi::c_void);
  impl Drop for Descriptor {
    fn drop(&mut self) {
      unsafe {
        LocalFree(self.0);
      }
    }
  }
  let mut pointer = ptr::null_mut();
  if unsafe {
    ConvertStringSecurityDescriptorToSecurityDescriptorW(wide(sddl).as_ptr(), 1, &mut pointer, ptr::null_mut())
  } == 0
  {
    return Err(io::Error::last_os_error());
  }
  let descriptor = Descriptor(pointer);
  operation(&SECURITY_ATTRIBUTES {
    nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
    lpSecurityDescriptor: descriptor.0,
    bInheritHandle: 0,
  })
}
fn server(sddl: &str, name: &str, first: bool) -> io::Result<NamedPipeServer> {
  with_security(sddl, |attributes| unsafe {
    ServerOptions::new()
      .first_pipe_instance(first)
      .reject_remote_clients(true)
      .create_with_security_attributes_raw(name, (attributes as *const SECURITY_ATTRIBUTES).cast_mut().cast())
  })
}
pub(super) fn user_pipe(sid: &str, name: &str, first: bool) -> io::Result<NamedPipeServer> {
  server(&format!("D:P(A;;GA;;;SY)(A;;GA;;;{sid})"), name, first)
}
pub fn create_service_admission_pipe(first: bool) -> io::Result<NamedPipeServer> {
  server(
    &format!("D:P(A;;GA;;;SY)(A;;0x{CLIENT_ACCESS:x};;;AU)"),
    common::paths::SERVICE_PIPE,
    first,
  )
}
fn client(name: &str, message: bool, impersonation: bool) -> io::Result<NamedPipeClient> {
  let file = OpenOptions::new()
    .read(true)
    .write(true)
    .access_mode(CLIENT_ACCESS)
    .custom_flags(FILE_FLAG_OVERLAPPED)
    .security_qos_flags(if impersonation {
      SECURITY_IMPERSONATION
    } else {
      SECURITY_IDENTIFICATION
    })
    .open(name)?;
  let pipe = unsafe { NamedPipeClient::from_raw_handle(file.into_raw_handle()) }?;
  let mut pid = 0;
  if unsafe { GetNamedPipeServerProcessId(pipe.as_raw_handle(), &mut pid) } == 0 {
    return Err(io::Error::last_os_error());
  }
  if pid == 0 || pid != super::service_process_id(common::paths::SERVICE_NAME)? {
    return Err(io::Error::new(
      io::ErrorKind::PermissionDenied,
      "Pipe does not belong to the registered VPN service",
    ));
  }
  if message {
    let mode = PIPE_READMODE_MESSAGE;
    if unsafe { SetNamedPipeHandleState(pipe.as_raw_handle(), &mode, ptr::null(), ptr::null()) } == 0 {
      return Err(io::Error::last_os_error());
    }
  }
  Ok(pipe)
}
/// Verify the service before the first request permits it to impersonate this user.
pub fn connect_service_admission() -> io::Result<NamedPipeClient> {
  client(common::paths::SERVICE_PIPE, false, true)
}
pub fn connect_service_worker(name: &str, message: bool) -> io::Result<NamedPipeClient> {
  client(name, message, false)
}

impl PipeClientIdentity {
  pub fn create_control_pipe(&self, name: &str) -> io::Result<NamedPipeServer> {
    server(
      &format!("D:P(A;;GA;;;SY)(A;;0x{CLIENT_ACCESS:x};;;{})", self.sid()),
      name,
      true,
    )
  }
  pub fn create_packet_pipe(&self, name: &str) -> io::Result<PacketPipe> {
    let name = wide(name);
    with_security(
      &format!("D:P(A;;GA;;;SY)(A;;0x{CLIENT_ACCESS:x};;;{})", self.sid()),
      |attributes| {
        let handle = unsafe {
          CreateNamedPipeW(
            name.as_ptr(),
            PIPE_ACCESS_DUPLEX | FILE_FLAG_OVERLAPPED | FILE_FLAG_FIRST_PIPE_INSTANCE,
            PIPE_TYPE_MESSAGE | PIPE_READMODE_MESSAGE | PIPE_REJECT_REMOTE_CLIENTS,
            1,
            256 * 1024,
            256 * 1024,
            0,
            attributes,
          )
        };
        if handle == INVALID_HANDLE_VALUE {
          return Err(io::Error::last_os_error());
        }
        Ok(PacketPipe(unsafe { OwnedHandle::from_raw_handle(handle) }))
      },
    )
  }
}
/// Raw Win32 overlapped I/O, deliberately never registered with Tokio/Mio IOCP.
pub struct PacketPipe(OwnedHandle);
impl PacketPipe {
  pub fn connect(&mut self, expected_pid: u32, timeout: Duration) -> io::Result<()> {
    let event = unsafe { CreateEventW(ptr::null(), 1, 0, ptr::null()) };
    if event.is_null() {
      return Err(io::Error::last_os_error());
    }
    let event = unsafe { OwnedHandle::from_raw_handle(event) };
    let mut overlap = OVERLAPPED {
      hEvent: event.as_raw_handle(),
      ..Default::default()
    };
    if unsafe { ConnectNamedPipe(self.0.as_raw_handle(), &mut overlap) } == 0 {
      let error = io::Error::last_os_error();
      match error.raw_os_error().map(|code| code as u32) {
        Some(ERROR_PIPE_CONNECTED) => {}
        Some(ERROR_IO_PENDING) => {
          let status = unsafe {
            WaitForSingleObject(
              event.as_raw_handle(),
              timeout.as_millis().min(u32::MAX as u128 - 1) as u32,
            )
          };
          let mut bytes = 0;
          if status != WAIT_OBJECT_0 {
            // Complete cancellation before OVERLAPPED/event storage is released.
            unsafe {
              CancelIoEx(self.0.as_raw_handle(), &overlap);
              GetOverlappedResult(self.0.as_raw_handle(), &overlap, &mut bytes, 1);
            }
            if status == WAIT_TIMEOUT {
              return Err(io::Error::new(io::ErrorKind::TimedOut, "Packet worker did not connect"));
            }
            return Err(io::Error::last_os_error());
          }
          if unsafe { GetOverlappedResult(self.0.as_raw_handle(), &overlap, &mut bytes, 0) } == 0 {
            return Err(io::Error::last_os_error());
          }
        }
        _ => return Err(error),
      }
    }
    let mut pid = 0;
    if unsafe { GetNamedPipeClientProcessId(self.0.as_raw_handle(), &mut pid) } == 0 {
      return Err(io::Error::last_os_error());
    }
    if pid != expected_pid {
      return Err(io::Error::new(
        io::ErrorKind::PermissionDenied,
        "Unexpected packet pipe client",
      ));
    }
    Ok(())
  }
  pub fn into_handle(self) -> OwnedHandle {
    self.0
  }
}
