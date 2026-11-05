use super::*;
fn derive_uuid_from_seed(seed: &str, parts: &[&str]) -> String {
  use uuid::Uuid;

  let namespace = Uuid::NAMESPACE_DNS;
  let name = format!("{}-{}", seed, parts.join("-"));

  Uuid::new_v5(&namespace, name.as_bytes()).hyphenated().to_string()
}

fn derived_profile_mac(seed: &str, client_os: ClientOs) -> String {
  let uuid = derive_uuid_from_seed(seed, &["os-profile", client_os.as_str(), "mac"]);
  let compact = uuid.replace('-', "");
  let bytes = compact
    .as_bytes()
    .chunks(2)
    .take(6)
    .map(|chunk| std::str::from_utf8(chunk).unwrap_or("00").to_lowercase())
    .collect::<Vec<_>>();

  match client_os {
    ClientOs::Linux | ClientOs::Windows => bytes.join("-"),
    ClientOs::Mac | ClientOs::Ios => bytes.join(":"),
  }
}

fn compact_uuid(seed: &str, client_os: ClientOs, suffix: &str) -> String {
  derive_uuid_from_seed(seed, &["os-profile", client_os.as_str(), suffix])
    .replace('-', "")
    .to_uppercase()
}

pub(super) fn project(
  runtime_os: ClientOs,
  client_os: ClientOs,
  host_identity: &HostIdentity,
  computer_override: Option<String>,
) -> ProfileHostIdentity {
  if client_os == runtime_os {
    return native::project(client_os, host_identity, computer_override);
  }

  match client_os {
    ClientOs::Linux => linux::project(host_identity, computer_override),
    ClientOs::Windows => windows::project(host_identity, computer_override),
    ClientOs::Mac => macos::project(host_identity, computer_override),
    ClientOs::Ios => native::project(client_os, host_identity, computer_override),
  }
}

fn computer(host_identity: &HostIdentity, computer_override: Option<String>) -> String {
  computer_override
    .filter(|value| !value.trim().is_empty())
    .unwrap_or_else(|| host_identity.computer().to_string())
}

fn profile_mac_for(client_os: ClientOs, mac_address: &str) -> String {
  match client_os {
    ClientOs::Linux | ClientOs::Windows => mac_address.replace(':', "-"),
    ClientOs::Mac | ClientOs::Ios => mac_address.replace('-', ":"),
  }
}

mod native {
  use super::*;

  pub(super) fn project(
    client_os: ClientOs,
    host_identity: &HostIdentity,
    computer_override: Option<String>,
  ) -> ProfileHostIdentity {
    ProfileHostIdentity {
      computer: computer(host_identity, computer_override),
      host_id: match client_os {
        ClientOs::Mac => host_identity.host_id().replace('-', ":"),
        ClientOs::Linux | ClientOs::Windows | ClientOs::Ios => host_identity.host_id().to_string(),
      },
      serial_number: host_identity.serialno().to_string(),
      mac_address: profile_mac_for(client_os, host_identity.mac_addr()),
    }
  }
}

mod linux {
  use super::*;

  pub(super) fn project(host_identity: &HostIdentity, computer_override: Option<String>) -> ProfileHostIdentity {
    let seed = host_identity.host_id();
    let host_id = derive_uuid_from_seed(seed, &["os-profile", ClientOs::Linux.as_str(), "host-id"]);
    ProfileHostIdentity {
      computer: computer(host_identity, computer_override),
      serial_number: serial_number::vmware_from_uuid(&host_id).expect("projected Linux host ID should be a UUID"),
      host_id,
      mac_address: derived_profile_mac(seed, ClientOs::Linux),
    }
  }
}

mod windows {
  use super::*;

  pub(super) fn project(host_identity: &HostIdentity, computer_override: Option<String>) -> ProfileHostIdentity {
    let seed = host_identity.host_id();
    ProfileHostIdentity {
      computer: computer(host_identity, computer_override),
      host_id: derive_uuid_from_seed(seed, &["os-profile", ClientOs::Windows.as_str(), "host-id"]),
      serial_number: serial_number::vmware_from_compact_hex(&compact_uuid(seed, ClientOs::Windows, "serialno"))
        .expect("projected Windows serial seed should be compact hex"),
      mac_address: derived_profile_mac(seed, ClientOs::Windows),
    }
  }
}

mod macos {
  use super::*;

  pub(super) fn project(host_identity: &HostIdentity, computer_override: Option<String>) -> ProfileHostIdentity {
    let seed = host_identity.host_id();
    let mac_address = derived_profile_mac(seed, ClientOs::Mac);
    ProfileHostIdentity {
      computer: computer(host_identity, computer_override),
      host_id: mac_address.clone(),
      serial_number: compact_uuid(seed, ClientOs::Mac, "serialno").chars().take(10).collect(),
      mac_address,
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  fn host_identity() -> HostIdentity {
    HostIdentity::from_parts(
      "real-computer".to_string(),
      "real-host-id".to_string(),
      "real-serial".to_string(),
      "aa:bb:cc:dd:ee:ff".to_string(),
    )
  }

  #[test]
  fn native_linux_projection_uses_real_identity_with_dash_mac() {
    let profile = native::project(ClientOs::Linux, &host_identity(), None);

    assert_eq!(profile.computer(), "real-computer");
    assert_eq!(profile.host_id(), "real-host-id");
    assert_eq!(profile.serialno(), "real-serial");
    assert_eq!(profile.mac_addr(), "aa-bb-cc-dd-ee-ff");
  }

  #[test]
  fn native_windows_projection_uses_real_identity_with_dash_mac() {
    let profile = native::project(ClientOs::Windows, &host_identity(), None);

    assert_eq!(profile.computer(), "real-computer");
    assert_eq!(profile.host_id(), "real-host-id");
    assert_eq!(profile.serialno(), "real-serial");
    assert_eq!(profile.mac_addr(), "aa-bb-cc-dd-ee-ff");
  }

  #[test]
  fn native_macos_projection_uses_real_identity_with_colon_mac() {
    let profile = native::project(ClientOs::Mac, &host_identity(), None);

    assert_eq!(profile.computer(), "real-computer");
    assert_eq!(profile.host_id(), "real:host:id");
    assert_eq!(profile.serialno(), "real-serial");
    assert_eq!(profile.mac_addr(), "aa:bb:cc:dd:ee:ff");
  }
}
