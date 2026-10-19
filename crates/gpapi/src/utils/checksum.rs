use std::{
  fmt::Write as _,
  fs::File,
  io::{self, Read},
  path::Path,
};

use anyhow::bail;
use sha2::{Digest, Sha256};

pub fn sha256_hex(data: impl AsRef<[u8]>) -> String {
  digest_hex(&Sha256::digest(data.as_ref()))
}

pub fn file_sha256(path: impl AsRef<Path>) -> io::Result<String> {
  let mut file = File::open(path)?;
  let mut hasher = Sha256::new();
  let mut buffer = [0; 8192];
  loop {
    let count = match file.read(&mut buffer) {
      Ok(0) => break,
      Ok(count) => count,
      Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
      Err(error) => return Err(error),
    };
    hasher.update(&buffer[..count]);
  }
  Ok(digest_hex(&hasher.finalize()))
}

fn digest_hex(digest: &[u8]) -> String {
  let mut hex = String::with_capacity(digest.len() * 2);
  for byte in digest {
    write!(&mut hex, "{byte:02x}").expect("writing to a String cannot fail");
  }
  hex
}

pub fn verify_checksum(path: &str, expected: &str) -> anyhow::Result<()> {
  let checksum = file_sha256(path)?;

  if checksum != expected {
    bail!("Checksum mismatch, expected: {}, actual: {}", expected, checksum);
  }

  Ok(())
}

#[cfg(test)]
mod tests {
  use std::io::Write;

  use super::*;

  #[test]
  fn known_sha256_vectors_use_lowercase_hex() {
    assert_eq!(
      sha256_hex(b""),
      "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
    );
    assert_eq!(
      sha256_hex(b"abc"),
      "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
  }

  #[test]
  fn hashes_files_across_buffer_boundaries_and_checks_expected_digest() {
    let mut file = tempfile::NamedTempFile::new().unwrap();
    file.write_all(&vec![b'a'; 1_000_000]).unwrap();
    let expected = "cdc76e5c9914fb9281a1c7e284d73e67f1809a48a497200e046d39ccc7112cd0";

    assert_eq!(file_sha256(file.path()).unwrap(), expected);
    verify_checksum(file.path().to_str().unwrap(), expected).unwrap();
    assert!(verify_checksum(file.path().to_str().unwrap(), "wrong").is_err());
  }

  #[test]
  fn hashes_empty_files_and_propagates_file_errors() {
    let file = tempfile::NamedTempFile::new().unwrap();
    assert_eq!(file_sha256(file.path()).unwrap(), sha256_hex(b""));
    let directory = tempfile::tempdir().unwrap();
    let error = file_sha256(directory.path().join("missing")).unwrap_err();
    assert_eq!(error.kind(), std::io::ErrorKind::NotFound);
    assert!(file_sha256(directory.path()).is_err());
  }
}
