use chacha20poly1305::{
  ChaCha20Poly1305, Key, KeyInit, Nonce,
  aead::{Aead, Generate},
};
use serde::{Serialize, de::DeserializeOwned};

pub fn generate_key() -> Key {
  Key::generate()
}

pub fn encrypt<T>(key: &Key, value: &T) -> anyhow::Result<Vec<u8>>
where
  T: Serialize,
{
  let cipher = ChaCha20Poly1305::new(key);
  let nonce = Nonce::generate();

  let data = serde_json::to_vec(value)?;
  let cipher_text = cipher.encrypt(&nonce, data.as_ref())?;

  let mut encrypted = Vec::new();
  encrypted.extend_from_slice(&nonce);
  encrypted.extend_from_slice(&cipher_text);

  Ok(encrypted)
}

pub fn decrypt<T>(key: &Key, encrypted: Vec<u8>) -> anyhow::Result<T>
where
  T: DeserializeOwned,
{
  let cipher = ChaCha20Poly1305::new(key);

  let nonce = &encrypted[..12];
  let cipher_text = &encrypted[12..];

  let plaintext = cipher.decrypt(nonce.try_into()?, cipher_text)?;

  let value = serde_json::from_slice(&plaintext)?;

  Ok(value)
}

pub struct Crypto {
  key: Vec<u8>,
}

impl Crypto {
  pub fn new(key: Vec<u8>) -> Self {
    Self { key }
  }

  pub fn encrypt<T: Serialize>(&self, plain: T) -> anyhow::Result<Vec<u8>> {
    let key: &[u8] = &self.key;
    let encrypted_data = encrypt(key.try_into()?, &plain)?;

    Ok(encrypted_data)
  }

  pub fn decrypt<T: DeserializeOwned>(&self, encrypted: Vec<u8>) -> anyhow::Result<T> {
    let key: &[u8] = &self.key;
    decrypt(key.try_into()?, encrypted)
  }

  pub fn encrypt_to<T: Serialize>(&self, path: &std::path::Path, plain: T) -> anyhow::Result<()> {
    let encrypted_data = self.encrypt(plain)?;
    std::fs::write(path, encrypted_data)?;

    Ok(())
  }

  pub fn decrypt_from<T: DeserializeOwned>(&self, path: &std::path::Path) -> anyhow::Result<T> {
    let encrypted_data = std::fs::read(path)?;
    self.decrypt(encrypted_data)
  }
}

#[cfg(test)]
mod tests {
  use serde::Deserialize;

  use super::*;

  #[derive(Serialize, Deserialize)]
  struct User {
    name: String,
    age: u8,
  }

  #[test]
  fn it_works() -> anyhow::Result<()> {
    let key = generate_key();

    let user = User {
      name: "test".to_string(),
      age: 18,
    };

    let encrypted = encrypt(&key, &user)?;

    let decrypted_user = decrypt::<User>(&key, encrypted)?;

    assert_eq!(user.name, decrypted_user.name);
    assert_eq!(user.age, decrypted_user.age);

    Ok(())
  }

  #[test]
  fn decrypts_settings_written_with_chacha20poly1305_0_10() -> anyhow::Result<()> {
    // Fixed key and ciphertext are test data produced with version 0.10.1.
    let key = [7_u8; 32];
    let encrypted =
      crate::utils::base64::decode_to_vec("AwMDAwMDAwMDAwMDkIQjBvvxesN7T/Suv8tEE53zfpGGup/QlB6Dbj1ah+NIXCDMOv/4eg==")?;
    let user: User = decrypt((&key).into(), encrypted.clone())?;
    assert_eq!(user.name, "test");
    assert_eq!(user.age, 18);

    // The nonce prefix and appended authentication tag retain their existing format.
    let cipher = ChaCha20Poly1305::new((&key).into());
    let ciphertext = cipher.encrypt(encrypted[..12].try_into()?, serde_json::to_vec(&user)?.as_slice())?;
    assert_eq!(ciphertext, encrypted[12..]);

    let mut tampered = encrypted;
    *tampered.last_mut().unwrap() ^= 1;
    assert!(decrypt::<User>((&key).into(), tampered).is_err());
    Ok(())
  }
}
