use gpapi::utils::checksum::verify_checksum;
use std::{
  fs::{self, File, OpenOptions, Permissions},
  io::{BufReader, Read, Write},
  os::unix::fs::{OpenOptionsExt, PermissionsExt},
  path::Path,
};
use tar::Archive;
use tempfile::NamedTempFile;
use xz2::read::XzDecoder;
const MAX_GUI_ARCHIVE_SIZE: u64 = 256 * 1024 * 1024;
const MAX_GUI_ARCHIVE_CONTENT_SIZE: u64 = 512 * 1024 * 1024;
const MAX_GUI_BINARY_SIZE: u64 = 256 * 1024 * 1024;
#[derive(Debug)]
pub(crate) enum InstallError {
  Checksum(anyhow::Error),
  Install(anyhow::Error),
}

pub(crate) fn installer() -> Option<std::path::PathBuf> {
  std::env::current_exe()
    .ok()
    .and_then(|executable| {
      executable
        .parent()?
        .parent()
        .map(|prefix| prefix.join("libexec/gpclient/gp-gui-installer"))
    })
    .filter(|path| path.is_file())
}

pub(crate) fn install(src: &Path, checksum: &str, target: &Path) -> Result<(), InstallError> {
  let installer = installer();
  install_gui_with_installer(src, checksum, target, installer.as_deref())
}

fn install_gui_with_installer(
  src: &Path,
  checksum: &str,
  target: &Path,
  installer: Option<&Path>,
) -> Result<(), InstallError> {
  let Some(dir) = target.parent() else {
    return Err(InstallError::Install(anyhow::anyhow!(
      "Failed to get parent directory of GUI binary"
    )));
  };
  fs::create_dir_all(dir).map_err(|err| InstallError::Install(err.into()))?;

  let staged_archive = stage_gui_archive(src, dir)?;
  let archive_path = staged_archive.path().to_string_lossy();
  verify_checksum(&archive_path, checksum).map_err(InstallError::Checksum)?;

  // A package may need to prepare downloaded executables for its runtime.
  // Resolve this helper beside the running service, never from the request or PATH.
  if let Some(installer) = installer {
    let status = std::process::Command::new(installer)
      .arg(staged_archive.path())
      .arg(checksum)
      .arg(target)
      .stdin(std::process::Stdio::null())
      .stdout(std::process::Stdio::null())
      .status()
      .map_err(|err| InstallError::Install(err.into()))?;
    return if status.success() {
      Ok(())
    } else {
      Err(InstallError::Install(anyhow::anyhow!(
        "GUI package installation failed: {status}"
      )))
    };
  }

  let tar = XzDecoder::new(BufReader::new(
    staged_archive
      .reopen()
      .map_err(|err| InstallError::Install(err.into()))?,
  ))
  .take(MAX_GUI_ARCHIVE_CONTENT_SIZE);
  let mut archive = Archive::new(tar);
  let binary_name = target.file_name().and_then(|name| name.to_str()).unwrap_or("gpgui");
  let file = extract_gui_binary(&mut archive, dir, binary_name)?;
  file
    .as_file()
    .set_permissions(Permissions::from_mode(0o755))
    .map_err(|err| InstallError::Install(err.into()))?;
  file
    .as_file()
    .sync_all()
    .map_err(|err| InstallError::Install(err.into()))?;
  file.persist(&target).map_err(|err| InstallError::Install(err.into()))?;
  File::open(dir)
    .and_then(|directory| directory.sync_all())
    .map_err(|err| InstallError::Install(err.into()))?;
  Ok(())
}

fn stage_gui_archive(src: &Path, dir: &Path) -> Result<NamedTempFile, InstallError> {
  let mut source = OpenOptions::new()
    .read(true)
    .custom_flags(nix::libc::O_NONBLOCK | nix::libc::O_NOFOLLOW)
    .open(src)
    .map_err(|err| InstallError::Install(err.into()))?;
  let metadata = source.metadata().map_err(|err| InstallError::Install(err.into()))?;
  let source_size = metadata.len();
  if !metadata.file_type().is_file() || source_size == 0 || source_size > MAX_GUI_ARCHIVE_SIZE {
    return Err(InstallError::Install(anyhow::anyhow!(
      "GUI update archive must be a non-empty regular file no larger than {MAX_GUI_ARCHIVE_SIZE} bytes"
    )));
  }

  let mut staged = NamedTempFile::new_in(dir).map_err(|err| InstallError::Install(err.into()))?;
  let copied = std::io::copy(&mut (&mut source).take(MAX_GUI_ARCHIVE_SIZE + 1), &mut staged)
    .map_err(|err| InstallError::Install(err.into()))?;
  if copied != source_size {
    return Err(InstallError::Install(anyhow::anyhow!(
      "GUI update archive changed while it was being read"
    )));
  }
  staged.flush().map_err(|err| InstallError::Install(err.into()))?;
  staged
    .as_file()
    .sync_all()
    .map_err(|err| InstallError::Install(err.into()))?;
  Ok(staged)
}

fn extract_gui_binary<R: Read>(
  archive: &mut Archive<R>,
  dir: &Path,
  binary_name: &str,
) -> Result<NamedTempFile, InstallError> {
  for entry in archive.entries().map_err(|err| InstallError::Install(err.into()))? {
    let mut entry = entry.map_err(|err| InstallError::Install(err.into()))?;
    let is_gui = entry
      .path()
      .map_err(|err| InstallError::Install(err.into()))?
      .file_name()
      .is_some_and(|name| name == binary_name);
    if is_gui && entry.header().entry_type().is_file() {
      let declared_size = entry.size();
      if declared_size == 0 || declared_size > MAX_GUI_BINARY_SIZE {
        return Err(InstallError::Install(anyhow::anyhow!(
          "GUI binary must be non-empty and no larger than {MAX_GUI_BINARY_SIZE} bytes"
        )));
      }
      let mut file = NamedTempFile::new_in(dir).map_err(|err| InstallError::Install(err.into()))?;
      let copied = std::io::copy(&mut entry, &mut file).map_err(|err| InstallError::Install(err.into()))?;
      if copied != declared_size {
        return Err(InstallError::Install(anyhow::anyhow!(
          "GUI binary size does not match its archive header"
        )));
      }
      file.flush().map_err(|err| InstallError::Install(err.into()))?;
      return Ok(file);
    }
  }
  Err(InstallError::Install(anyhow::anyhow!(
    "GUI update archive does not contain a non-empty regular {binary_name} file"
  )))
}

#[cfg(test)]
mod tests {
  use std::{io::Cursor, os::unix::fs::PermissionsExt};

  use tar::{Builder, EntryType, Header};
  use xz2::write::XzEncoder;

  use super::*;

  fn archive_with_entry(entry_type: EntryType, contents: &[u8]) -> Vec<u8> {
    let mut data = Vec::new();
    {
      let mut builder = Builder::new(&mut data);
      let mut header = Header::new_gnu();
      header.set_entry_type(entry_type);
      header.set_size(contents.len() as u64);
      header.set_mode(0o755);
      header.set_cksum();
      builder.append_data(&mut header, "gpgui", contents).unwrap();
      builder.finish().unwrap();
    }
    data
  }

  fn compressed_gui_archive(contents: &[u8]) -> NamedTempFile {
    let tar = archive_with_entry(EntryType::Regular, contents);
    let mut encoder = XzEncoder::new(Vec::new(), 6);
    encoder.write_all(&tar).unwrap();
    let compressed = encoder.finish().unwrap();
    let mut archive = NamedTempFile::new().unwrap();
    archive.write_all(&compressed).unwrap();
    archive.flush().unwrap();
    archive
  }

  #[test]
  fn rejects_non_file_and_empty_gui_entries() {
    let output = tempfile::tempdir().unwrap();
    for (entry_type, contents) in [(EntryType::Directory, &[][..]), (EntryType::Regular, &[][..])] {
      let data = archive_with_entry(entry_type, contents);
      let mut archive = Archive::new(Cursor::new(data));
      assert!(extract_gui_binary(&mut archive, output.path(), "gpgui").is_err());
    }
  }

  #[test]
  fn extracts_non_empty_regular_gui_entry() {
    let output = tempfile::tempdir().unwrap();
    let data = archive_with_entry(EntryType::Regular, b"gui binary");
    let mut archive = Archive::new(Cursor::new(data));
    let extracted = extract_gui_binary(&mut archive, output.path(), "gpgui").unwrap();
    let contents = std::fs::read(extracted.path()).unwrap();
    assert_eq!(contents, b"gui binary");
  }

  #[test]
  fn rejects_invalid_archive_sources() {
    let output = tempfile::tempdir().unwrap();
    assert!(stage_gui_archive(output.path(), output.path()).is_err());

    let oversized = output.path().join("oversized.tar.xz");
    File::create(&oversized)
      .unwrap()
      .set_len(MAX_GUI_ARCHIVE_SIZE + 1)
      .unwrap();
    assert!(stage_gui_archive(&oversized, output.path()).is_err());
  }

  #[test]
  fn rejects_oversized_gui_entry() {
    let output = tempfile::tempdir().unwrap();
    let mut header = Header::new_gnu();
    header.set_entry_type(EntryType::Regular);
    header.set_size(MAX_GUI_BINARY_SIZE + 1);
    header.set_mode(0o755);
    header.set_path("gpgui").unwrap();
    header.set_cksum();
    let mut data = header.as_bytes().to_vec();
    data.resize(512 * 3, 0);

    let mut archive = Archive::new(Cursor::new(data));
    assert!(extract_gui_binary(&mut archive, output.path(), "gpgui").is_err());
  }

  #[test]
  fn installs_gui_at_the_supplied_target() {
    let archive = compressed_gui_archive(b"gui binary");
    let checksum = gpapi::utils::checksum::file_sha256(archive.path()).unwrap();
    let output = tempfile::tempdir().unwrap();
    let target = output.path().join("gpclient/gpgui");

    install(archive.path(), &checksum, &target).unwrap();

    assert_eq!(std::fs::read(&target).unwrap(), b"gui binary");
    assert_eq!(std::fs::metadata(&target).unwrap().permissions().mode() & 0o777, 0o755);
  }

  #[test]
  fn installs_nsh_beside_tauri_without_replacing_it() {
    let output = tempfile::tempdir().unwrap();
    let tauri = output.path().join("gpgui");
    std::fs::write(&tauri, b"tauri").unwrap();
    let mut encoder = XzEncoder::new(Vec::new(), 1);
    {
      let mut archive = Builder::new(&mut encoder);
      let mut header = Header::new_gnu();
      header.set_size(3);
      header.set_mode(0o755);
      header.set_cksum();
      archive
        .append_data(&mut header, "gpgui-nsh_3.2.0/gpgui-nsh", &b"nsh"[..])
        .unwrap();
      archive.finish().unwrap();
    }
    let mut archive = NamedTempFile::new().unwrap();
    archive.write_all(&encoder.finish().unwrap()).unwrap();
    archive.flush().unwrap();
    let checksum = gpapi::utils::checksum::file_sha256(archive.path()).unwrap();
    let nsh = output.path().join("gpgui-nsh");
    install(archive.path(), &checksum, &nsh).unwrap();
    assert_eq!(std::fs::read(nsh).unwrap(), b"nsh");
    assert_eq!(std::fs::read(tauri).unwrap(), b"tauri");
  }

  #[test]
  fn checksum_failure_preserves_the_existing_gui() {
    let archive = compressed_gui_archive(b"replacement");
    let output = tempfile::tempdir().unwrap();
    let target = output.path().join("gpclient/gpgui");
    std::fs::create_dir_all(target.parent().unwrap()).unwrap();
    std::fs::write(&target, b"existing").unwrap();

    assert!(install(archive.path(), "invalid", &target).is_err());
    assert_eq!(std::fs::read(target).unwrap(), b"existing");
  }

  #[test]
  fn package_installer_receives_a_verified_staged_archive() {
    let archive = compressed_gui_archive(b"gui binary");
    let checksum = gpapi::utils::checksum::file_sha256(archive.path()).unwrap();
    let output = tempfile::tempdir().unwrap();
    let target = output.path().join("gpgui-nsh");
    let installer = output.path().join("installer");
    std::fs::write(
      &installer,
      "#!/bin/sh\nset -eu\ntest -f \"$1\"\nprintf '%s' \"$2\" > \"$3\"\n",
    )
    .unwrap();
    std::fs::set_permissions(&installer, Permissions::from_mode(0o755)).unwrap();

    assert!(install_gui_with_installer(archive.path(), "invalid", &target, Some(&installer)).is_err());
    assert!(!target.exists());
    install_gui_with_installer(archive.path(), &checksum, &target, Some(&installer)).unwrap();
    assert_eq!(std::fs::read_to_string(target).unwrap(), checksum);
  }

  #[test]
  fn package_installation_failure_preserves_the_existing_gui() {
    let archive = compressed_gui_archive(b"replacement");
    let checksum = gpapi::utils::checksum::file_sha256(archive.path()).unwrap();
    let output = tempfile::tempdir().unwrap();
    let target = output.path().join("gpgui");
    std::fs::write(&target, b"existing").unwrap();
    let installer = output.path().join("installer");
    std::fs::write(&installer, "#!/bin/sh\nexit 1\n").unwrap();
    std::fs::set_permissions(&installer, Permissions::from_mode(0o755)).unwrap();

    assert!(install_gui_with_installer(archive.path(), &checksum, &target, Some(&installer)).is_err());
    assert_eq!(std::fs::read(target).unwrap(), b"existing");
  }

  #[test]
  #[ignore = "requires GP_TEST_GUI_BINARY pointing to a built gpgui"]
  fn installs_single_gui_binary_for_fresh_and_upgrade_downloads() {
    let binary = std::fs::read(std::env::var("GP_TEST_GUI_BINARY").expect("GP_TEST_GUI_BINARY")).unwrap();
    let archive = compressed_gui_archive(&binary);
    let checksum = gpapi::utils::checksum::file_sha256(archive.path()).unwrap();
    let temporary = tempfile::tempdir().unwrap();
    let output = std::env::var_os("GP_TEST_GUI_INSTALL_DIR")
      .map(std::path::PathBuf::from)
      .unwrap_or_else(|| temporary.path().to_owned());
    std::fs::create_dir_all(&output).unwrap();
    std::fs::set_permissions(&output, std::fs::Permissions::from_mode(0o755)).unwrap();
    let target = output.join("gpclient/gpgui");
    for upgrade in [false, true] {
      if upgrade {
        std::fs::write(&target, b"old GUI").unwrap();
      }
      install(archive.path(), &checksum, &target).unwrap();
      assert_eq!(std::fs::read_dir(target.parent().unwrap()).unwrap().count(), 1);
      assert_eq!(std::fs::read(&target).unwrap(), binary);
      let result = std::process::Command::new(&target).arg("--version").output().unwrap();
      assert!(result.status.success(), "{}", String::from_utf8_lossy(&result.stderr));
      assert!(String::from_utf8_lossy(&result.stdout).contains(env!("CARGO_PKG_VERSION")));
    }
  }
}
