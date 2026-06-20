pub(super) fn vmware_from_uuid(uuid: &str) -> Option<String> {
  let parts: Vec<&str> = uuid.split('-').collect();
  if parts.len() != 5
    || parts[0].len() != 8
    || parts[1].len() != 4
    || parts[2].len() != 4
    || parts[3].len() != 4
    || parts[4].len() != 12
    || !parts.iter().all(|part| part.chars().all(|ch| ch.is_ascii_hexdigit()))
  {
    return None;
  }

  let compact = [
    reverse_hex_bytes(parts[0]),
    reverse_hex_bytes(parts[1]),
    reverse_hex_bytes(parts[2]),
    parts[3].to_lowercase(),
    parts[4].to_lowercase(),
  ]
  .concat();

  vmware_from_compact_hex(&compact)
}

pub(super) fn vmware_from_compact_hex(compact: &str) -> Option<String> {
  let compact = compact.trim().to_lowercase();
  if compact.len() != 32 || !compact.chars().all(|ch| ch.is_ascii_hexdigit()) {
    return None;
  }

  Some(format!(
    "VMware-{} {} {} {} {} {} {} {}-{} {} {} {} {} {} {} {}",
    &compact[0..2],
    &compact[2..4],
    &compact[4..6],
    &compact[6..8],
    &compact[8..10],
    &compact[10..12],
    &compact[12..14],
    &compact[14..16],
    &compact[16..18],
    &compact[18..20],
    &compact[20..22],
    &compact[22..24],
    &compact[24..26],
    &compact[26..28],
    &compact[28..30],
    &compact[30..32]
  ))
}

fn reverse_hex_bytes(value: &str) -> String {
  value
    .as_bytes()
    .chunks(2)
    .rev()
    .map(|chunk| std::str::from_utf8(chunk).unwrap_or_default().to_lowercase())
    .collect()
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn vmware_serial_from_uuid_uses_smbios_byte_order() {
    assert_eq!(
      vmware_from_uuid("5a784d56-6461-19ac-9ea9-d36a3b9c6cef").unwrap(),
      "VMware-56 4d 78 5a 61 64 ac 19-9e a9 d3 6a 3b 9c 6c ef"
    );
  }

  #[test]
  fn invalid_uuid_is_rejected() {
    assert_eq!(vmware_from_uuid("not-a-uuid"), None);
  }
}
