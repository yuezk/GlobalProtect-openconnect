//! Redact GlobalProtect response bodies before writing diagnostics.
use xmltree::{Element, EmitterConfig, XMLNode};

const REDACTED: &str = "[redacted]";
const MAX_XML_BYTES: usize = 1024 * 1024;

pub fn redact_xml(body: &[u8]) -> String {
  if body.len() > MAX_XML_BYTES {
    return "[XML omitted: exceeds 1 MiB]".into();
  }
  let Ok(text) = std::str::from_utf8(body) else {
    return "[XML omitted: invalid UTF-8]".into();
  };
  if text.to_ascii_lowercase().contains("<!doctype") {
    return "[XML omitted: DTD]".into();
  }
  let Ok(nodes) = Element::parse_all(body) else {
    return "[XML omitted: malformed or non-XML response]".into();
  };
  let mut roots = nodes.into_iter().filter_map(|node| match node {
    XMLNode::Element(element) => Some(element),
    _ => None,
  });
  let Some(mut root) = roots.next() else {
    return "[XML omitted: no document element]".into();
  };
  if roots.next().is_some() || root.name.eq_ignore_ascii_case("html") {
    return "[XML omitted: unsupported response document]".into();
  }
  redact_element(&mut root);
  let mut output = Vec::new();
  if root
    .write_with_config(
      &mut output,
      EmitterConfig::new()
        .perform_indent(true)
        .write_document_declaration(false),
    )
    .is_err()
  {
    return "[XML omitted: serialization failed]".into();
  }
  String::from_utf8(output).unwrap_or_else(|_| "[XML omitted: serialization failed]".into())
}

fn sensitive_name(name: &str) -> bool {
  let name: String = name
    .chars()
    .filter(char::is_ascii_alphanumeric)
    .map(|c| c.to_ascii_lowercase())
    .collect();
  if name.ends_with("label") {
    return false;
  }
  name.contains("cookie")
    || name.contains("token")
    || name.contains("password")
    || name.contains("passphrase")
    || name.contains("secret")
    || name.starts_with("ekey")
    || name.starts_with("akey")
    || matches!(
      name.as_str(),
      "argument"
        | "samlrequest"
        | "samlresponse"
        | "samlrequestid"
        | "assertion"
        | "encryptedassertion"
        | "authnrequest"
        | "challenge"
        | "privatekey"
        | "encryptionkey"
        | "enckey"
        | "hskey"
        | "enchskey"
        | "tunnelopaque"
        | "nonce"
        | "user"
        | "username"
        | "ccusername"
        | "credential"
    )
}

fn redact_element(element: &mut Element) {
  let named_secret = matches!(element.name.as_str(), "field" | "param" | "property" | "setting")
    && ["name", "key"]
      .iter()
      .any(|name| element.attributes.get(*name).is_some_and(|value| sensitive_name(value)));
  let secret = sensitive_name(&element.name) || named_secret;
  for (name, value) in &mut element.attributes {
    if sensitive_name(name) || (secret && !(named_secret && matches!(name.as_str(), "name" | "key"))) {
      *value = REDACTED.into();
    } else {
      *value = redact_url(value);
    }
  }
  if secret {
    element.children = vec![XMLNode::Text(REDACTED.into())];
    return;
  }
  element
    .children
    .retain(|node| matches!(node, XMLNode::Element(_) | XMLNode::Text(_) | XMLNode::CData(_)));
  for node in &mut element.children {
    match node {
      XMLNode::Element(child) => redact_element(child),
      XMLNode::Text(value) | XMLNode::CData(value) => *value = redact_url(value),
      _ => {}
    }
  }
}

fn redact_url(value: &str) -> String {
  let Ok(mut url) = url::Url::parse(value) else {
    return value.into();
  };
  if !matches!(url.scheme(), "http" | "https") {
    return value.into();
  }
  if !url.username().is_empty() {
    let _ = url.set_username(REDACTED);
  }
  if url.password().is_some() {
    let _ = url.set_password(Some(REDACTED));
  }
  if url.query().is_some() {
    let pairs: Vec<_> = url
      .query_pairs()
      .map(|(name, _)| (name.into_owned(), REDACTED))
      .collect();
    url.query_pairs_mut().clear().extend_pairs(pairs);
  }
  if url.fragment().is_some() {
    url.set_fragment(Some(REDACTED));
  }
  url.to_string()
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn redacts_positional_login_arguments_and_session_keys() {
    let xml = redact_xml(br#"<response><jnlp><application-desc><argument>fixture-auth-cookie</argument><argument>fixture-user</argument></application-desc></jnlp><ipsec><ekey-c2s><bits>256</bits><val>fixture-key-c2s</val></ekey-c2s><ekey-s2c>fixture-key-s2c</ekey-s2c><akey-c2s><val>fixture-auth-c2s</val></akey-c2s><akey-s2c>fixture-auth-s2c</akey-s2c></ipsec><enc-hs-key>fixture-hs-key</enc-hs-key><PrivateKey>fixture-private-key</PrivateKey><SAMLResponse>fixture-saml-response</SAMLResponse><tunnel-opaque><val>fixture-opaque</val></tunnel-opaque><mtu>1400</mtu></response>"#);
    assert!(!xml.contains("fixture-"));
    assert!(xml.contains("<mtu>1400</mtu>"));
    Element::parse(xml.as_bytes()).unwrap();
  }

  #[test]
  fn omits_unparseable_html_dtd_and_oversized_bodies_without_echoing_them() {
    for body in [
      b"not XML fixture-secret".as_slice(),
      b"<response><cookie>fixture-secret",
      b"<html><body>fixture-secret</body></html>",
      b"<!DOCTYPE response [<!ENTITY secret 'fixture-secret'>]><response>&secret;</response>",
      b"<response/>fixture-secret",
      b"\xfffixture-secret",
    ] {
      let xml = redact_xml(body);
      assert!(xml.starts_with("[XML omitted:"));
      assert!(!xml.contains("fixture-secret"));
    }
    assert_eq!(
      redact_xml(&vec![b'x'; MAX_XML_BYTES + 1]),
      "[XML omitted: exceeds 1 MiB]"
    );
  }
}
