//! Relay a pasted browser callback from the elevated CLI's own console.
use std::{
  fs::File,
  io::{self, Write},
  os::windows::io::{AsRawHandle, BorrowedHandle, FromRawHandle, OwnedHandle},
  ptr,
  sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
  },
  thread::{self, JoinHandle},
  time::Duration,
};
use windows_sys::Win32::{
  Foundation::{DUPLICATE_SAME_ACCESS, DuplicateHandle, ERROR_OPERATION_ABORTED, INVALID_HANDLE_VALUE},
  System::{
    Console::{GetConsoleMode, GetStdHandle, ReadConsoleW, STD_INPUT_HANDLE},
    IO::CancelSynchronousIo,
    Threading::GetCurrentProcess,
  },
};

const MAX_CALLBACK_CHARS: usize = 64 * 1024;

pub(super) fn is_console(input: &OwnedHandle) -> bool {
  let mut mode = 0;
  unsafe { GetConsoleMode(input.as_raw_handle(), &mut mode) != 0 }
}

pub(super) struct ConsoleInput {
  stop: Arc<AtomicBool>,
  worker: Option<JoinHandle<io::Result<()>>>,
}

impl ConsoleInput {
  pub(super) fn start(input: OwnedHandle, output: File) -> io::Result<Self> {
    let stop = Arc::new(AtomicBool::new(false));
    let worker_stop = stop.clone();
    let worker = thread::Builder::new().spawn(move || {
      let line = read_line(input, &worker_stop)?;
      if !worker_stop.load(Ordering::Relaxed) {
        let mut output = output;
        output.write_all(line.as_bytes())?;
      }
      Ok(())
    })?;
    Ok(Self {
      stop,
      worker: Some(worker),
    })
  }

  pub(super) fn check(&mut self) -> io::Result<()> {
    if self.worker.as_ref().is_some_and(JoinHandle::is_finished) {
      let worker = self.worker.take().expect("completed console reader");
      worker
        .join()
        .map_err(|_| io::Error::other("Console reader stopped unexpectedly"))??;
    }
    Ok(())
  }
}

impl Drop for ConsoleInput {
  fn drop(&mut self) {
    if let Some(worker) = self.worker.take() {
      self.stop.store(true, Ordering::Relaxed);
      // Retry in case the worker was between checking stop and entering I/O.
      // Join before returning so a failed sign-in cannot consume a later prompt.
      while !worker.is_finished() {
        unsafe { CancelSynchronousIo(worker.as_raw_handle()) };
        thread::sleep(Duration::from_millis(5));
      }
      let _ = worker.join();
    }
  }
}

fn read_line(input: OwnedHandle, stop: &AtomicBool) -> io::Result<String> {
  let mut text = Vec::new();
  let mut buffer = [0u16; 4096];
  while !stop.load(Ordering::Relaxed) {
    let mut count = 0;
    if unsafe {
      ReadConsoleW(
        input.as_raw_handle(),
        buffer.as_mut_ptr().cast(),
        buffer.len() as u32,
        &mut count,
        ptr::null(),
      )
    } == 0
    {
      let error = io::Error::last_os_error();
      if error.raw_os_error() == Some(ERROR_OPERATION_ABORTED as i32) {
        return Err(io::Error::new(io::ErrorKind::Interrupted, error));
      }
      return Err(error);
    }
    if count == 0 {
      break;
    }
    let chunk = &buffer[..count as usize];
    let newline = chunk.iter().position(|&ch| ch == b'\n' as u16);
    text.extend_from_slice(&chunk[..newline.map_or(chunk.len(), |index| index + 1)]);
    if text.len() > MAX_CALLBACK_CHARS {
      return Err(io::Error::new(
        io::ErrorKind::InvalidData,
        "Browser callback is too long",
      ));
    }
    if newline.is_some() {
      break;
    }
  }
  String::from_utf16(&text).map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))
}

pub(super) fn terminal_input() -> io::Result<OwnedHandle> {
  let input = unsafe { GetStdHandle(STD_INPUT_HANDLE) };
  if input.is_null() || input == INVALID_HANDLE_VALUE {
    return Err(io::Error::other("Manual browser sign-in requires terminal input"));
  }
  let input = unsafe { BorrowedHandle::borrow_raw(input) };
  let process = unsafe { GetCurrentProcess() };
  let mut duplicate = ptr::null_mut();
  if unsafe {
    DuplicateHandle(
      process,
      input.as_raw_handle(),
      process,
      &mut duplicate,
      0,
      1,
      DUPLICATE_SAME_ACCESS,
    )
  } == 0
  {
    return Err(io::Error::last_os_error());
  }
  Ok(unsafe { OwnedHandle::from_raw_handle(duplicate) })
}
