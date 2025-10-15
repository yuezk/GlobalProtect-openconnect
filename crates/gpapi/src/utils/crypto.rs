use chacha20poly1305::{
  aead::{Aead, OsRng},
  AeadCore, ChaCha20Poly1305, Key, KeyInit,
};
use serde::{de::DeserializeOwned, Serialize};

pub fn generate_key() -> Key {
  ChaCha20Poly1305::generate_key(&mut OsRng)
}

pub fn encrypt<T>(key: &Key, value: &T) -> anyhow::Result<Vec<u8>>
where
  T: Serialize,
{
  let cipher = ChaCha20Poly1305::new(key);
  let nonce = ChaCha20Poly1305::generate_nonce(&mut OsRng);

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

  let plaintext = cipher.decrypt(nonce.into(), cipher_text)?;

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
    let encrypted_data = encrypt(key.into(), &plain)?;

    Ok(encrypted_data)
  }

  pub fn decrypt<T: DeserializeOwned>(&self, encrypted: Vec<u8>) -> anyhow::Result<T> {
    let key: &[u8] = &self.key;
    decrypt(key.into(), encrypted)
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
}
