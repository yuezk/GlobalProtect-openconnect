use base64::{Engine, engine::general_purpose};

pub fn encode(data: &[u8]) -> String {
  let engine = general_purpose::STANDARD;

  engine.encode(data)
}

pub fn decode_to_vec(s: &str) -> anyhow::Result<Vec<u8>> {
  let engine = general_purpose::STANDARD;
  let decoded = engine.decode(s)?;

  Ok(decoded)
}

pub(crate) fn decode_to_string(s: &str) -> anyhow::Result<String> {
  let decoded = decode_to_vec(s)?;
  let decoded = String::from_utf8(decoded)?;

  Ok(decoded)
}
