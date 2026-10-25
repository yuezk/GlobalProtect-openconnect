#[cfg(target_os = "macos")]
#[path = "pkcs12/macos.rs"]
pub(super) mod pkcs12_identity;
#[cfg(not(any(target_os = "macos", target_os = "ios")))]
#[path = "pkcs12/openssl.rs"]
pub(super) mod pkcs12_identity;
#[cfg(target_os = "ios")]
#[path = "pkcs12/ios.rs"]
pub(super) mod pkcs12_identity;
#[cfg(any(target_os = "macos", target_os = "ios"))]
#[path = "traditional_key/apple.rs"]
pub(super) mod traditional_key;
#[cfg(not(any(target_os = "macos", target_os = "ios")))]
#[path = "traditional_key/openssl.rs"]
pub(super) mod traditional_key;

#[cfg(feature = "desktop")]
pub(super) mod legacy_pkcs8;
