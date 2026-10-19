pub const TOOLBAR_HEIGHT: f64 = 40.0;
pub const LOADING_ADDRESS: &str = "Loading sign-in page…";

/// Display web addresses with query strings and fragments, but without credentials.
pub fn display_address(uri: Option<&str>) -> String {
  let Some(mut url) = uri.and_then(|uri| url::Url::parse(uri).ok()) else {
    return LOADING_ADDRESS.to_string();
  };
  if !matches!(url.scheme(), "http" | "https") || url.host_str().is_none() {
    return LOADING_ADDRESS.to_string();
  }

  let _ = url.set_username("");
  let _ = url.set_password(None);
  url.into()
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn hides_credentials_but_preserves_query_and_fragment() {
    assert_eq!(
      display_address(Some(
        "https://user:secret@login.example:8443/saml?SAMLRequest=secret#token"
      )),
      "https://login.example:8443/saml?SAMLRequest=secret#token"
    );
  }

  #[test]
  fn preserves_scheme_host_and_encoded_path() {
    assert_eq!(
      display_address(Some("http://example.com/a%20b")),
      "http://example.com/a%20b"
    );
    assert_eq!(display_address(Some("https://[::1]:8443/")), "https://[::1]:8443/");
  }

  #[test]
  fn uses_placeholder_for_local_pages_and_callbacks() {
    for uri in [
      None,
      Some(""),
      Some("invalid"),
      Some("about:blank"),
      Some("data:text/html,secret"),
      Some("globalprotectcallback:secret"),
    ] {
      assert_eq!(display_address(uri), LOADING_ADDRESS);
    }
  }
}
