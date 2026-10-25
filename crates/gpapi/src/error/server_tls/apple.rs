use std::error::Error;

use super::{ServerTlsErrorKind, classify_macos_status};

pub(super) fn classify(error: &(dyn Error + 'static)) -> Option<ServerTlsErrorKind> {
  error
    .downcast_ref::<security_framework::base::Error>()
    .map(|error| classify_macos_status(error.code()))
}
