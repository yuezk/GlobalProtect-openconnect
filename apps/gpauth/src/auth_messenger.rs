use anyhow::bail;
use gpapi::auth::SamlAuthData;
use log::{error, info};
use tokio::sync::{mpsc, RwLock};
use tokio_util::sync::CancellationToken;

pub enum AuthError {
  /// Failed to load page due to TLS error
  TlsError,
  /// 1. Found auth data in headers/body but it's invalid
  /// 2. Loaded an empty page, failed to load page. etc.
  Invalid,
  /// No auth data found in headers/body
  NotFound,
}

pub type AuthResult = anyhow::Result<SamlAuthData, AuthError>;

pub enum AuthEvent {
  Data(SamlAuthData),
  Error(AuthError),
  RaiseWindow,
  Close,
}

pub struct AuthMessenger {
  tx: mpsc::UnboundedSender<AuthEvent>,
  rx: RwLock<mpsc::UnboundedReceiver<AuthEvent>>,
  raise_window_cancel_token: RwLock<Option<CancellationToken>>,
}

impl AuthMessenger {
  pub fn new() -> Self {
    let (tx, rx) = mpsc::unbounded_channel();

    Self {
      tx,
      rx: RwLock::new(rx),
      raise_window_cancel_token: Default::default(),
    }
  }

  pub async fn subscribe(&self) -> anyhow::Result<AuthEvent> {
    let mut rx = self.rx.write().await;
    if let Some(event) = rx.recv().await {
      return Ok(event);
    }
    bail!("Failed to receive auth event");
  }

  pub fn send_auth_event(&self, event: AuthEvent) {
    if let Err(event) = self.tx.send(event) {
      error!("Failed to send auth event: {}", event);
    }
  }

  pub fn send_auth_result(&self, result: AuthResult) {
    match result {
      Ok(data) => self.send_auth_data(data),
      Err(err) => self.send_auth_error(err),
    }
  }

  pub fn send_auth_error(&self, err: AuthError) {
    self.send_auth_event(AuthEvent::Error(err));
  }

  pub fn send_auth_data(&self, data: SamlAuthData) {
    self.send_auth_event(AuthEvent::Data(data));
  }

  pub fn schedule_raise_window(&self, delay: u64) {
    let cancel_token = CancellationToken::new();
    let cancel_token_clone = cancel_token.clone();

    if let Ok(mut guard) = self.raise_window_cancel_token.try_write() {
      // Cancel the previous raise window task if it exists
      if let Some(token) = guard.take() {
        token.cancel();
      }
      *guard = Some(cancel_token_clone);
    }

    let tx = self.tx.clone();
    tokio::spawn(async move {
      info!("Displaying the window in {} second(s)...", delay);

      tokio::select! {
        _ = tokio::time::sleep(tokio::time::Duration::from_secs(delay)) => {
          if let Err(err) = tx.send(AuthEvent::RaiseWindow) {
            error!("Failed to send raise window event: {}", err);
          }
        }
        _ = cancel_token.cancelled() => {
          info!("Cancelled raise window task");
        }
      }
    });
  }

  pub fn cancel_raise_window(&self) {
    if let Ok(mut cancel_token) = self.raise_window_cancel_token.try_write() {
      if let Some(token) = cancel_token.take() {
        token.cancel();
      }
    }
  }
}
