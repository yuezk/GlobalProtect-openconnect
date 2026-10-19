use std::error::Error;

use super::ServerTlsErrorKind;

pub(super) fn classify(error: &(dyn Error + 'static)) -> Option<ServerTlsErrorKind> {
  let stack = error.downcast_ref::<openssl::error::ErrorStack>().or_else(|| {
    error
      .downcast_ref::<openssl::ssl::Error>()
      .and_then(openssl::ssl::Error::ssl_error)
  })?;
  // ERR_LIB_SSL and SSL_R_TLSV13_ALERT_CERTIFICATE_REQUIRED from sslerr.h.
  stack
    .errors()
    .iter()
    .any(|error| error.library_code() == 20 && error.reason_code() == 1116)
    .then_some(ServerTlsErrorKind::ClientCertificateRequired)
}
