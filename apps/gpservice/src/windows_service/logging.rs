use std::{fs::OpenOptions, io, path::Path};

use file_rotate::{ContentLimit, FileRotate, compression::Compression, suffix::AppendCount};

const ROTATE_BYTES: usize = 5 * 1024 * 1024;
const ARCHIVE_COUNT: usize = 3;

pub(super) fn writer(path: &Path) -> io::Result<FileRotate<AppendCount>> {
  // Keep startup failure explicit: FileRotate's constructor suppresses open errors.
  OpenOptions::new().create(true).read(true).append(true).open(path)?;
  Ok(FileRotate::new(
    path,
    AppendCount::new(ARCHIVE_COUNT),
    ContentLimit::BytesSurpassed(ROTATE_BYTES),
    Compression::None,
    None,
  ))
}

#[cfg(test)]
mod tests {
  use super::*;
  use std::{fs, io::Write};

  #[test]
  fn keeps_the_latest_three_archives_and_complete_current_entry() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("gpservice.log");
    fs::write(dir.path().join("unrelated.log"), "keep").unwrap();
    let mut log = writer(&path).unwrap();
    for marker in b'0'..=b'5' {
      log.write_all(&vec![marker; ROTATE_BYTES + 1]).unwrap();
    }
    log.flush().unwrap();
    drop(log);

    assert_eq!(fs::read(&path).unwrap(), vec![b'5'; ROTATE_BYTES + 1]);
    for (suffix, marker) in [(1, b'4'), (2, b'3'), (3, b'2')] {
      let archive = dir.path().join(format!("gpservice.log.{suffix}"));
      assert_eq!(fs::read(archive).unwrap(), vec![marker; ROTATE_BYTES + 1]);
    }
    assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 5);
    assert_eq!(fs::read_to_string(dir.path().join("unrelated.log")).unwrap(), "keep");
  }

  #[test]
  fn restart_appends_and_accounts_for_existing_file_size() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("gpservice.log");
    fs::write(&path, "before restart\n").unwrap();
    {
      let mut log = writer(&path).unwrap();
      log.write_all(b"after restart\n").unwrap();
    }
    assert_eq!(fs::read_to_string(&path).unwrap(), "before restart\nafter restart\n");

    let entry = vec![b'x'; ROTATE_BYTES + 1];
    fs::write(&path, &entry).unwrap();
    {
      let mut log = writer(&path).unwrap();
      log.write_all(b"new entry\n").unwrap();
    }
    assert_eq!(fs::read(&path).unwrap(), b"new entry\n");
    assert_eq!(fs::read(dir.path().join("gpservice.log.1")).unwrap(), entry);
  }

  #[test]
  fn invalid_log_path_remains_a_startup_error() {
    let dir = tempfile::tempdir().unwrap();
    assert!(writer(dir.path()).is_err());
  }
}
