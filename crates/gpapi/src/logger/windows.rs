use file_rotate::{ContentLimit, FileRotate, compression::Compression, suffix::AppendCount};
use std::{
  fs::{self, OpenOptions},
  io::{self, Write},
  path::Path,
  sync::{Arc, Mutex, OnceLock},
};

const ROTATE_BYTES: usize = 5 * 1024 * 1024;
const ARCHIVE_COUNT: usize = 3;
static DIAGNOSTICS: OnceLock<DiagnosticWriter> = OnceLock::new();

#[derive(Clone)]
struct DiagnosticWriter(Arc<Mutex<FileRotate<AppendCount>>>);

impl DiagnosticWriter {
  fn open(path: &Path) -> io::Result<Self> {
    if let Some(parent) = path.parent().filter(|parent| !parent.as_os_str().is_empty()) {
      fs::create_dir_all(parent)?;
    }
    // FileRotate's constructor suppresses open errors, so check the destination first.
    OpenOptions::new().create(true).append(true).open(path)?;
    Ok(Self(Arc::new(Mutex::new(FileRotate::new(
      path,
      AppendCount::new(ARCHIVE_COUNT),
      ContentLimit::BytesSurpassed(ROTATE_BYTES),
      Compression::None,
      None,
    )))))
  }
}

impl Write for DiagnosticWriter {
  fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
    self
      .0
      .lock()
      .map_err(|_| io::Error::other("Log writer lock is poisoned"))?
      .write(bytes)
  }

  fn flush(&mut self) -> io::Result<()> {
    self
      .0
      .lock()
      .map_err(|_| io::Error::other("Log writer lock is poisoned"))?
      .flush()
  }
}

pub fn init_file(path: &Path, level: log::Level) -> io::Result<()> {
  let writer = DiagnosticWriter::open(path)?;
  let logger = super::build_env_logger(crate::log_format::LogFormat::Text)
    .target(env_logger::Target::Pipe(Box::new(writer.clone())))
    .build();
  DIAGNOSTICS
    .set(writer)
    .map_err(|_| io::Error::other("File logging is already initialized"))?;
  super::init_with_logger(level, logger);
  Ok(())
}

pub(super) fn diagnostic_writer() -> Option<Box<dyn Write + Send>> {
  DIAGNOSTICS
    .get()
    .map(|writer| Box::new(writer.clone()) as Box<dyn Write + Send>)
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn gui_and_helper_writers_share_rotation_and_keep_only_three_archives() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("logs/gui.log");
    let mut gui = DiagnosticWriter::open(&path).unwrap();
    let mut helper = gui.clone();
    for marker in b'0'..=b'5' {
      let writer = if marker % 2 == 0 { &mut gui } else { &mut helper };
      writer.write_all(&vec![marker; ROTATE_BYTES + 1]).unwrap();
    }
    helper.flush().unwrap();
    drop((gui, helper));
    assert_eq!(fs::read(&path).unwrap(), vec![b'5'; ROTATE_BYTES + 1]);
    for (suffix, marker) in [(1, b'4'), (2, b'3'), (3, b'2')] {
      assert_eq!(
        fs::read(path.with_extension(format!("log.{suffix}"))).unwrap(),
        vec![marker; ROTATE_BYTES + 1]
      );
    }
    assert_eq!(fs::read_dir(path.parent().unwrap()).unwrap().count(), 4);
    let mut reopened = DiagnosticWriter::open(&path).unwrap();
    reopened.write_all(b"after restart\n").unwrap();
    reopened.flush().unwrap();
    assert_eq!(fs::read(&path).unwrap(), b"after restart\n");
  }
}
