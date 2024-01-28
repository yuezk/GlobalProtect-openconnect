use std::sync::RwLock;

use redact_engine::{Pattern, Redaction as RedactEngine};
use regex::Regex;
use url::{form_urlencoded, Url};

pub struct Redaction {
  redact_engine: RwLock<Option<RedactEngine>>,
}

impl Default for Redaction {
  fn default() -> Self {
    Self::new()
  }
}

impl Redaction {
  pub fn new() -> Self {
    let redact_engine = RedactEngine::custom("[**********]").add_pattern(Pattern {
      test: Regex::new("(((25[0-5]|(2[0-4]|1\\d|[1-9]|)\\d)\\.?\\b){4})").unwrap(),
      group: 1,
    });

    Self {
      redact_engine: RwLock::new(Some(redact_engine)),
    }
  }

  pub fn add_value(&self, text: &str) -> anyhow::Result<()> {
    let mut redact_engine = self
      .redact_engine
      .write()
      .map_err(|_| anyhow::anyhow!("Failed to acquire write lock on redact engine"))?;

    *redact_engine = Some(
      redact_engine
        .take()
        .ok_or_else(|| anyhow::anyhow!("Failed to take redact engine"))?
        .add_value(text)?,
    );

    Ok(())
  }

  pub fn add_values(&self, texts: &[&str]) -> anyhow::Result<()> {
    let mut redact_engine = self
      .redact_engine
      .write()
      .map_err(|_| anyhow::anyhow!("Failed to acquire write lock on redact engine"))?;

    *redact_engine = Some(
      redact_engine
        .take()
        .ok_or_else(|| anyhow::anyhow!("Failed to take redact engine"))?
        .add_values(texts.to_vec())?,
    );

    Ok(())
  }

  pub fn redact_str(&self, text: &str) -> String {
    self
      .redact_engine
      .read()
      .expect("Failed to acquire read lock on redact engine")
      .as_ref()
      .expect("Failed to get redact engine")
      .redact_str(text)
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
