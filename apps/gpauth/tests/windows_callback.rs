#![cfg(windows)]

use auth::AuthCallbackReceiver;
use std::{process::Stdio, time::Duration};

#[tokio::test]
async fn callback_command_forwards_between_processes() {
  let receiver = AuthCallbackReceiver::bind().unwrap();
  let child = tokio::process::Command::new(env!("CARGO_BIN_EXE_gpauth"))
    .args([
      "auth-callback",
      "globalprotectcallback:cas-as=1&un=windows-fixture&token=callback-command-test",
    ])
    .stdout(Stdio::piped())
    .stderr(Stdio::piped())
    .kill_on_drop(true)
    .spawn()
    .unwrap();
  let (data, output) = tokio::time::timeout(Duration::from_secs(10), async {
    tokio::join!(receiver.receive(), child.wait_with_output())
  })
  .await
  .expect("callback executable did not complete");
  let output = output.unwrap();
  assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
  let data = data.unwrap();
  assert_eq!(data.username(), "windows-fixture");
  assert_eq!(data.token(), Some("callback-command-test"));
}
