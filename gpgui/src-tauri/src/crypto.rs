use aes_gcm::{
    aead::{consts::U12, Aead, OsRng},
    AeadCore, Aes256Gcm, Key, KeyInit, Nonce,
};
use keyring::Entry;

const SERVICE_NAME: &str = "GlobalProtect-openconnect";
const ENTRY_KEY: &str = "master-key";

fn get_master_key() -> Result<Key<Aes256Gcm>, anyhow::Error> {
    let key_entry = Entry::new(SERVICE_NAME, ENTRY_KEY)?;

    if let Ok(key) = key_entry.get_password() {
        let key = hex::decode(key)?;
        return Ok(Key::<Aes256Gcm>::clone_from_slice(&key));
    }

    let key = Aes256Gcm::generate_key(OsRng);
    let encoded_key = hex::encode(key);

    key_entry.set_password(&encoded_key)?;

    Ok(key)
}

pub(crate) fn encrypt(data: &str) -> Result<String, anyhow::Error> {
    let master_key = get_master_key()?;
    let cipher = Aes256Gcm::new(&master_key);
    let nonce = Aes256Gcm::generate_nonce(&mut OsRng);
    let cipher_text = cipher.encrypt(&nonce, data.as_bytes())?;

    let mut encrypted = nonce.to_vec();
    encrypted.extend_from_slice(&cipher_text);
    Ok(hex::encode(encrypted))
}

pub(crate) fn decrypt(encrypted: &str) -> Result<String, anyhow::Error> {
    let master_key = get_master_key()?;
    let encrypted = hex::decode(encrypted)?;
    let nonce = Nonce::<U12>::from_slice(&encrypted[..12]);
    let cipher_text = &encrypted[12..];
    let cipher = Aes256Gcm::new(&master_key);
    let plain_text = cipher.decrypt(nonce, cipher_text)?;

    String::from_utf8(plain_text).map_err(|err| err.into())
}
