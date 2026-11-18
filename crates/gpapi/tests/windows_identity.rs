#![cfg(windows)]

use gpapi::windows::{PipeClientIdentity, create_user_pipe, current_user_sid};
use std::{io, os::windows::io::AsRawHandle, ptr, time::Duration};
use tokio::{
  io::{AsyncReadExt, AsyncWriteExt},
  net::windows::named_pipe::ClientOptions,
};
use windows_sys::Win32::{
  Foundation::{CloseHandle, ERROR_NO_TOKEN},
  Security::{GetTokenInformation, TOKEN_QUERY, TOKEN_TYPE, TokenPrimary, TokenType},
  Storage::FileSystem::{SECURITY_IDENTIFICATION, SECURITY_IMPERSONATION},
  System::Threading::{GetCurrentThread, OpenThreadToken},
};

fn assert_reverted() {
  let mut token = ptr::null_mut();
  let success = unsafe { OpenThreadToken(GetCurrentThread(), TOKEN_QUERY, 1, &mut token) };
  if success != 0 {
    unsafe {
      CloseHandle(token);
    }
    panic!("The service thread still impersonates its client");
  }
  assert_eq!(io::Error::last_os_error().raw_os_error(), Some(ERROR_NO_TOKEN as i32));
}

async fn admission(qos: u32) -> io::Result<PipeClientIdentity> {
  let path = format!(r"\\.\pipe\GPConnect.IdentityTest.{}", uuid::Uuid::new_v4());
  let mut pipe = create_user_pipe(&path, true)?;
  let client = tokio::spawn(async move {
    let mut pipe = ClientOptions::new().security_qos_flags(qos).open(path).unwrap();
    pipe.write_all(b"admit").await.unwrap();
    let mut response = [0];
    pipe.read_exact(&mut response).await.unwrap();
    // Confirm delivery before the server disconnects its Windows pipe.
    pipe.write_all(b"ack").await.unwrap();
  });
  pipe.connect().await?;
  let mut request = [0; 5];
  pipe.read_exact(&mut request).await?;
  assert_eq!(&request, b"admit");
  let identity = PipeClientIdentity::from_pipe(&pipe);
  assert_reverted();
  pipe.write_all(&[1]).await?;
  let mut ack = [0; 3];
  pipe.read_exact(&mut ack).await?;
  assert_eq!(&ack, b"ack");
  client.await.unwrap();
  identity
}

#[tokio::test]
async fn captures_primary_token_and_reverts_before_returning() {
  let identity = tokio::time::timeout(Duration::from_secs(5), admission(SECURITY_IMPERSONATION))
    .await
    .unwrap()
    .unwrap();
  assert_eq!(identity.sid(), current_user_sid().unwrap());
  let mut kind: TOKEN_TYPE = 0;
  let mut length = 0;
  assert_ne!(
    unsafe {
      GetTokenInformation(
        identity.primary_token().as_raw_handle(),
        TokenType,
        (&mut kind as *mut TOKEN_TYPE).cast(),
        std::mem::size_of_val(&kind) as u32,
        &mut length,
      )
    },
    0
  );
  assert_eq!(kind, TokenPrimary);
  assert_reverted();
}

#[tokio::test]
async fn identification_only_client_cannot_supply_a_worker_token() {
  assert!(
    tokio::time::timeout(Duration::from_secs(5), admission(SECURITY_IDENTIFICATION))
      .await
      .unwrap()
      .is_err()
  );
  assert_reverted();
}
