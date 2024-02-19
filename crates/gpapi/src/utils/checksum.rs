use std::path::Path;

use anyhow::bail;

pub fn verify_checksum(path: &str, expected: &str) -> anyhow::Result<()> {
  let file = Path::new(&path);
  let checksum = sha256::try_digest(&file)?;

  if checksum != expected {
    bail!("Checksum mismatch, expected: {}, actual: {}", expected, checksum);
  }

  Ok(())
}
