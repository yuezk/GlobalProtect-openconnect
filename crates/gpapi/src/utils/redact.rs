use std::sync::RwLock;

use regex::Regex;
use url::{Url, form_urlencoded};

pub struct Redaction {
  patterns: RwLock<Vec<Regex>>,
}

impl Default for Redaction {
  fn default() -> Self {
    Self::new()
  }
}

impl Redaction {
  pub fn new() -> Self {
    Self {
      patterns: RwLock::new(vec![
        Regex::new("(((25[0-5]|(2[0-4]|1\\d|[1-9]|)\\d)\\.?\\b){4})").unwrap(),
      ]),
    }
  }

  pub fn add_value(&self, text: &str) -> anyhow::Result<()> {
    self.add_values(&[text])
  }

  pub fn add_values(&self, texts: &[&str]) -> anyhow::Result<()> {
    let patterns = texts
      .iter()
      .map(|text| Regex::new(&regex::escape(text)))
      .collect::<Result<Vec<_>, _>>()?;
    self
      .patterns
      .write()
      .map_err(|_| anyhow::anyhow!("Failed to acquire write lock on redaction patterns"))?
      .extend(patterns);

    Ok(())
  }

  pub fn redact_str(&self, text: &str) -> String {
    let patterns = self
      .patterns
      .read()
      .expect("Failed to acquire read lock on redaction patterns");
    let mut redacted = text.to_owned();
    // Match the original text, then replace in registration order. Matching the
    // replacements would change how overlapping values are redacted.
    for pattern in patterns.iter() {
      for matched in pattern.find_iter(text) {
        redacted = redacted.replacen(matched.as_str(), "[**********]", 1);
      }
    }
    redacted
  }
}

/// Redact a value by replacing all but the first and last character with asterisks,
/// The length of the value to be redacted must be at least 3 characters.
/// e.g. "foo" -> "f**********o"
pub fn redact_value(text: &str) -> String {
  if text.len() < 3 {
    return text.to_string();
  }

  let mut redacted = String::new();
  redacted.push_str(&text[0..1]);
  redacted.push_str(&"*".repeat(10));
  redacted.push_str(&text[text.len() - 1..]);

  redacted
}

pub fn redact_uri(uri: &str) -> String {
  let Ok(mut url) = Url::parse(uri) else {
    return uri.to_string();
  };

  // Could be a data: URI
  if url.cannot_be_a_base() {
    if url.scheme() == "about" {
      return uri.to_string();
    }

    if url.path().len() > 15 {
      return format!(
        "{}:{}{}",
        url.scheme(),
        &url.path()[0..10],
        redact_value(&url.path()[10..])
      );
    }

    return format!("{}:{}", url.scheme(), redact_value(url.path()));
  }

  let host = url.host_str().unwrap_or_default();
  if url.set_host(Some(&redact_value(host))).is_err() {
    let redacted_query = redact_query(url.query())
      .as_deref()
      .map(|query| format!("?{}", query))
      .unwrap_or_default();

    return format!("{}://[**********]{}{}", url.scheme(), url.path(), redacted_query);
  }

  let redacted_query = redact_query(url.query());
  url.set_query(redacted_query.as_deref());
  url.to_string()
}

pub fn redact_form_params(params: &[(&str, &str)]) -> String {
  let mut pairs = params
    .iter()
    .map(|(key, value)| (*key, redact_form_value(key, value)))
    .collect::<Vec<_>>();

  pairs.sort_by(|(left, _), (right, _)| left.cmp(right));

  form_urlencoded::Serializer::new(String::new())
    .extend_pairs(pairs)
    .finish()
}

fn redact_form_value(key: &str, value: &str) -> String {
  if value.is_empty() || is_safe_form_param(key) {
    return value.to_string();
  }

  format!("redacted-{}", value.len())
}

fn is_safe_form_param(key: &str) -> bool {
  matches!(
    key,
    "clientVer"
      | "clientos"
      | "config-digest"
      | "csc-digest"
      | "csc-support"
      | "direct"
      | "ipv6-support"
      | "jnlpReady"
      | "ok"
      | "os-version"
      | "prot"
      | "swg-auth-token"
      | "swg-nonce"
  )
}

fn redact_query(query: Option<&str>) -> Option<String> {
  let query = query?;

  let query_pairs = form_urlencoded::parse(query.as_bytes());
  let mut redacted_pairs = query_pairs.map(|(key, value)| (key, redact_value(&value)));

  let query = form_urlencoded::Serializer::new(String::new())
    .extend_pairs(redacted_pairs.by_ref())
    .finish();

  Some(query)
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn redacts_repeated_ipv4_addresses() {
    assert_eq!(
      Redaction::new().redact_str("from 192.0.2.1 to 198.51.100.9, then 192.0.2.1"),
      "from [**********] to [**********], then [**********]"
    );
  }

  #[test]
  fn registered_values_are_literal_and_support_unicode() {
    let redaction = Redaction::new();
    redaction.add_values(&["a.b+$[x]", "秘密🔑"]).unwrap();

    assert_eq!(
      redaction.redact_str("a.b+$[x] 秘密🔑 a.b+$[x] aXb"),
      "[**********] [**********] [**********] aXb"
    );
  }

  #[test]
  fn overlapping_values_keep_registration_order() {
    let redaction = Redaction::new();
    redaction.add_values(&["foobar", "foo"]).unwrap();

    assert_eq!(redaction.redact_str("foobar foo"), "[**********] [**********]");
  }

  #[test]
  fn unicode_word_characters_preserve_ipv4_boundaries() {
    assert_eq!(Redaction::new().redact_str("192.0.2.1é"), "192.0.2.1é");
  }

  #[test]
  fn it_should_not_redact_value() {
    let text = "fo";

    assert_eq!(redact_value(text), "fo");
  }

  #[test]
  fn it_should_redact_value() {
    let text = "foo";

    assert_eq!(redact_value(text), "f**********o");
  }

  #[test]
  fn it_should_redact_dynamic_value() {
    let redaction = Redaction::new();

    redaction.add_value("foo").unwrap();

    assert_eq!(redaction.redact_str("hello, foo, bar"), "hello, [**********], bar");
  }

  #[test]
  fn it_should_redact_dynamic_values() {
    let redaction = Redaction::new();

    redaction.add_values(&["foo", "bar"]).unwrap();

    assert_eq!(
      redaction.redact_str("hello, foo, bar"),
      "hello, [**********], [**********]"
    );
  }

  #[test]
  fn it_should_redact_uri() {
    let uri = "https://foo.bar";
    assert_eq!(redact_uri(uri), "https://f**********r/");

    let uri = "https://foo.bar/";
    assert_eq!(redact_uri(uri), "https://f**********r/");

    let uri = "https://foo.bar/baz";
    assert_eq!(redact_uri(uri), "https://f**********r/baz");

    let uri = "https://foo.bar/baz?qux=quux";
    assert_eq!(redact_uri(uri), "https://f**********r/baz?qux=q**********x");
  }

  #[test]
  fn it_should_redact_form_params() {
    let params = [
      ("portal-userauthcookie", "secret-cookie"),
      ("csc-support", "yes"),
      ("prelogin-cookie", ""),
    ];

    assert_eq!(
      redact_form_params(&params),
      "csc-support=yes&portal-userauthcookie=redacted-13&prelogin-cookie="
    );
  }

  #[test]
  fn it_should_redact_data_uri() {
    let uri = "data:text/plain;a";
    assert_eq!(redact_uri(uri), "data:t**********a");

    let uri = "data:text/plain;base64,SGVsbG8sIFdvcmxkIQ==";
    assert_eq!(redact_uri(uri), "data:text/plain;**********=");

    let uri = "about:blank";
    assert_eq!(redact_uri(uri), "about:blank");
  }

  #[test]
  fn it_should_redact_ipv6() {
    let uri = "https://[2001:db8::1]:8080";
    assert_eq!(redact_uri(uri), "https://[**********]/");

    let uri = "https://[2001:db8::1]:8080/";
    assert_eq!(redact_uri(uri), "https://[**********]/");

    let uri = "https://[2001:db8::1]:8080/baz";
    assert_eq!(redact_uri(uri), "https://[**********]/baz");

    let uri = "https://[2001:db8::1]:8080/baz?qux=quux";
    assert_eq!(redact_uri(uri), "https://[**********]/baz?qux=q**********x");
  }
}
