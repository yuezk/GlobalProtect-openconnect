use crate::crypto::{decrypt, encrypt};
use log::warn;
use serde::{de::DeserializeOwned, Deserialize};
use std::fmt::Debug;
use tauri::{AppHandle, Manager, Wry};
use tauri_plugin_store::{with_store, Error, StoreCollection};

const STORE_PATH: &str = ".data.json";

#[derive(Debug, Deserialize)]
pub(crate) struct KeyHint<'a> {
    key: &'a str,
    encrypted: bool,
}

impl<'a> KeyHint<'a> {
    pub(crate) fn new(key: &'a str, encrypted: bool) -> Self {
        Self { key, encrypted }
    }
}

pub(crate) struct AppStorage<'a> {
    path: &'a str,
    app_handle: AppHandle<Wry>,
}

impl AppStorage<'_> {
    pub(crate) fn new(app_handle: AppHandle<Wry>) -> Self {
        Self {
            path: STORE_PATH,
            app_handle,
        }
    }

    pub fn get<T: DeserializeOwned + Debug>(&self, hint: KeyHint) -> Option<T> {
        let stores = self.app_handle.state::<StoreCollection<Wry>>();
        with_store(self.app_handle.clone(), stores, self.path, |store| {
            store
                .get(hint.key)
                .ok_or_else(|| Error::Deserialize("Value not found".into()))
                .and_then(|value| {
                    if !hint.encrypted {
                        return Ok(serde_json::from_value::<T>(value.clone())?);
                    }

                    let value = value
                        .as_str()
                        .ok_or_else(|| Error::Deserialize("Value is not a string".into()))?;
                    let value = decrypt(value).map_err(|err| {
                        Error::Deserialize(format!("Failed to decrypt value: {}", err).into())
                    })?;

                    Ok(serde_json::from_str::<T>(&value)?)
                })
        })
        .map_err(|err| warn!("Error getting value: {:?}", err))
        .ok()
    }

    pub fn set<T: serde::Serialize>(&self, hint: KeyHint, value: &T) -> Result<(), Error> {
        let stores = self.app_handle.state::<StoreCollection<Wry>>();

        with_store(self.app_handle.clone(), stores, self.path, |store| {
            let value = if hint.encrypted {
                let json_str = serde_json::to_string(value)?;
                let encrypted = encrypt(&json_str).map_err(|err| {
                    Error::Serialize(format!("Failed to encrypt value: {}", err).into())
                })?;
                serde_json::to_value(encrypted)?
            } else {
                serde_json::to_value(value)?
            };

            store.insert(hint.key.to_string(), value)?;
            Ok(())
        })
    }

    pub fn save(&self) -> Result<(), Error> {
        let stores = self.app_handle.state::<StoreCollection<Wry>>();
        
        with_store(self.app_handle.clone(), stores, self.path, |store| {
            store.save()?;
            Ok(())
        })
    }
}
