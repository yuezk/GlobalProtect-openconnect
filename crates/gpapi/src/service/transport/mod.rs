mod credential;
mod error;
mod handshake;
mod message;
mod noise;

pub use credential::SessionCredential;
pub use error::{CloseReason, HandshakeRejection, ServiceErrorCode, TransportError};
pub use handshake::{ClientPrelude, ServerPrelude};
pub use message::{ClientMessage, ServerMessage, ServiceRejection, ServiceResult};
pub use noise::{NoiseInitiator, NoiseResponder, NoiseTransport};

pub const MAX_CREDENTIAL_FRAME: usize = 512;
pub const MAX_PRELUDE: usize = 256;
pub const MAX_PRODUCT_VERSION: usize = 64;
pub const MAX_PLAINTEXT: usize = 60 * 1024;
pub const MAX_WS_MESSAGE: usize = u16::MAX as usize;
pub const MAX_SERVICE_MESSAGE: usize = 256;
