//! Optional compatibility conversion for standalone PKCS#12 PBE-SHA1-3DES keys.
//! The executable is a runtime option, not a linked or bundled dependency.
use std::{
  io::{ErrorKind, Read, Write},
  os::{fd::AsFd, unix::process::CommandExt},
  process::{Child, Command, Stdio},
  thread,
  time::{Duration, Instant},
};

use nix::{
  fcntl::{FcntlArg, OFlag, fcntl},
  sys::signal::{Signal, killpg},
  unistd::Pid,
};
use pkcs8::SecretDocument;
use zeroize::Zeroizing;

use super::request::RequestIdentityError::{self, LegacyKeyConversion};

const TIMEOUT: Duration = Duration::from_secs(10);
const MAX_OUTPUT: u64 = 1024 * 1024;
const MAX_PASSWORD_BYTES: usize = 1023;

// The converter owns a separate process group, including wrapper descendants.
// Clean up that group and reap the direct child on every return path.
struct Converter(Child);

impl Drop for Converter {
  fn drop(&mut self) {
    let _ = killpg(Pid::from_raw(self.0.id() as i32), Signal::SIGKILL);
    let _ = self.0.wait();
  }
}

fn nonblocking(pipe: &impl AsFd) -> anyhow::Result<()> {
  fcntl(pipe, FcntlArg::F_SETFL(OFlag::O_NONBLOCK))
    .map_err(|_| LegacyKeyConversion("could not configure converter pipes"))?;
  Ok(())
}

// Read at most one chunk per iteration so a busy pipe cannot starve the deadline.
// Returns true only after EOF; inherited pipes remain subject to the deadline.
fn read_output(pipe: &mut impl Read, output: &mut Zeroizing<Vec<u8>>, limit: u64) -> anyhow::Result<bool> {
  let mut buffer = Zeroizing::new([0u8; 8192]);
  match pipe.read(buffer.as_mut_slice()) {
    Ok(0) => Ok(true),
    Ok(count) => {
      if output.len() as u64 + count as u64 > limit {
        return Err(LegacyKeyConversion("converter output exceeds the size limit").into());
      }
      output.extend_from_slice(&buffer[..count]);
      Ok(false)
    }
    Err(error) if matches!(error.kind(), ErrorKind::WouldBlock | ErrorKind::Interrupted) => Ok(false),
    Err(_) => Err(LegacyKeyConversion("could not read converter output").into()),
  }
}

pub(super) fn decrypt(der: &[u8], passphrase: Option<&str>) -> anyhow::Result<SecretDocument> {
  let passphrase = passphrase.ok_or(RequestIdentityError::NoPassphrase("PEM"))?;
  // The command's line reader reserves one byte for a terminator. The linked
  // OpenSSL API accepted 1024 bytes, but the CLI would truncate that password.
  if passphrase.len() > MAX_PASSWORD_BYTES {
    return Err(LegacyKeyConversion("the password exceeds OpenSSL's 1023-byte input limit").into());
  }
  // OpenSSL's pipe password source reads one line. Never silently truncate it.
  if passphrase.contains(['\r', '\n', '\0']) {
    return Err(LegacyKeyConversion("the password cannot be passed through OpenSSL's line-based input").into());
  }

  // Only encrypted input touches disk, in a private temporary file. The
  // original file is never changed and decrypted output stays in memory.
  let mut input = tempfile::NamedTempFile::new()?;
  input.write_all(pem::encode(&pem::Pem::new("ENCRYPTED PRIVATE KEY", der)).as_bytes())?;
  let child = Command::new("openssl")
    .args([
      "pkcs8", "-topk8", "-nocrypt", "-inform", "PEM", "-outform", "DER", "-in",
    ])
    .arg(input.path())
    .args(["-passin", "stdin"])
    .env("LC_ALL", "C")
    .process_group(0)
    .stdin(Stdio::piped())
    .stdout(Stdio::piped())
    .stderr(Stdio::piped())
    .spawn()
    .map_err(|_| LegacyKeyConversion("the openssl executable is unavailable"))?;
  let mut converter = Converter(child);
  let mut stdin = converter.0.stdin.take();
  let mut stdout = converter.0.stdout.take().unwrap();
  let mut stderr = converter.0.stderr.take().unwrap();
  nonblocking(stdin.as_ref().unwrap())?;
  nonblocking(&stdout)?;
  nonblocking(&stderr)?;
  let mut password = Zeroizing::new(passphrase.as_bytes().to_vec());
  password.push(b'\n');
  let mut written = 0;
  let mut input_failed = false;
  let mut output = Zeroizing::new(Vec::new());
  let mut diagnostic = Zeroizing::new(Vec::new());
  let mut output_done = false;
  let mut diagnostic_done = false;
  let mut status = None;
  let started = Instant::now();
  let status = loop {
    if started.elapsed() >= TIMEOUT {
      return Err(LegacyKeyConversion("conversion timed out").into());
    }
    if let Some(pipe) = stdin.as_mut() {
      match pipe.write(&password[written..]) {
        Ok(0) => {
          input_failed = true;
          stdin = None;
        }
        Ok(count) => {
          written += count;
          if written == password.len() {
            stdin = None;
          }
        }
        Err(error) if matches!(error.kind(), ErrorKind::WouldBlock | ErrorKind::Interrupted) => {}
        Err(_) => {
          input_failed = true;
          stdin = None;
        }
      }
    }
    if !output_done {
      output_done = read_output(&mut stdout, &mut output, MAX_OUTPUT)?;
    }
    if !diagnostic_done {
      diagnostic_done = read_output(&mut stderr, &mut diagnostic, 16 * 1024)?;
    }
    if status.is_none() {
      status = converter
        .0
        .try_wait()
        .map_err(|_| LegacyKeyConversion("could not wait for the converter"))?;
    }
    if let Some(status) = status {
      if output_done && diagnostic_done && stdin.is_none() {
        break status;
      }
    }
    thread::sleep(Duration::from_millis(20));
  };
  if status.success() && input_failed {
    return Err(LegacyKeyConversion("could not send the password").into());
  }

  if !status.success() {
    // Never include the command's output in user-facing errors: it can contain
    // key material. Preserve the old Linux password retry contract only for
    // an explicit decryption failure, not missing cipher/provider support.
    if [b"bad decrypt".as_slice(), b"pkcs12 cipherfinal error".as_slice()]
      .iter()
      .any(|message| diagnostic.windows(message.len()).any(|window| window == *message))
    {
      return Err(RequestIdentityError::DecryptError("PEM").into());
    }
    return Err(LegacyKeyConversion("this OpenSSL installation cannot decrypt the key").into());
  }
  SecretDocument::try_from(output.as_slice()).map_err(|_| LegacyKeyConversion("invalid converted key").into())
}
