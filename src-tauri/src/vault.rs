//! Opaque application secrets; native store failures are returned to the caller.
use anyhow::Context;
use hellas_private::SecretStore;
use serde::{Serialize, de::DeserializeOwned};
use std::{path::Path, sync::Arc};
use zeroize::Zeroizing;

#[derive(Clone)]
pub struct Vault(Arc<SecretStore>);
impl Vault {
    pub fn native(directory: &Path) -> anyhow::Result<Self> {
        let namespace = hellas_auth::Digest::hash(directory.as_os_str().as_encoded_bytes());
        Ok(Self(Arc::new(SecretStore::native(format!(
            "hellas-gate-{}",
            &hex::encode(namespace.as_bytes())[..24]
        ))?)))
    }
    #[cfg(test)]
    pub fn fixture(directory: &Path) -> Self {
        hellas_private::restrict_directory(directory).unwrap();
        Self(Arc::new(
            SecretStore::headless_files(directory.to_owned(), "gate-test").unwrap(),
        ))
    }
    pub fn source(&self, key: &str, prefix: &str) -> hellas_providers::HttpSecret {
        hellas_providers::HttpSecret::Store {
            store: (*self.0).clone(),
            key: key.to_owned(),
            prefix: prefix.to_owned(),
        }
    }
    pub fn blocking_get(&self, key: &str) -> anyhow::Result<Option<Zeroizing<Vec<u8>>>> {
        Ok(tokio::task::block_in_place(|| self.0.get(key))?)
    }
    pub fn blocking_delete(&self, key: &str) -> anyhow::Result<()> {
        tokio::task::block_in_place(|| self.0.delete(key))?;
        Ok(())
    }
    pub fn blocking_set(&self, key: &str, value: &[u8]) -> anyhow::Result<()> {
        tokio::task::block_in_place(|| self.0.set(key, value))?;
        Ok(())
    }
    pub async fn get(&self, key: &str) -> anyhow::Result<Option<Zeroizing<Vec<u8>>>> {
        let store = self.0.clone();
        let key = key.to_owned();
        Ok(tokio::task::spawn_blocking(move || store.get(&key)).await??)
    }
    pub async fn required(&self, key: &str) -> anyhow::Result<Zeroizing<Vec<u8>>> {
        self.get(key)
            .await?
            .context("required vault entry missing; owner-approved recovery is required")
    }
    pub async fn set(&self, key: &str, value: &[u8]) -> anyhow::Result<()> {
        let store = self.0.clone();
        let key = key.to_owned();
        let value = Zeroizing::new(value.to_vec());
        tokio::task::spawn_blocking(move || store.set(&key, &value)).await??;
        Ok(())
    }
    pub async fn delete(&self, key: &str) -> anyhow::Result<()> {
        let store = self.0.clone();
        let key = key.to_owned();
        tokio::task::spawn_blocking(move || store.delete(&key)).await??;
        Ok(())
    }
}

pub fn read<T: DeserializeOwned>(path: &Path) -> anyhow::Result<Option<T>> {
    match hellas_private::read_bounded_regular_file(path, 8 << 20) {
        Ok(bytes) => Ok(Some(serde_json::from_slice(&bytes)?)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e.into()),
    }
}
pub fn write<T: Serialize>(path: &Path, value: &T) -> anyhow::Result<()> {
    #[cfg(test)]
    publication_fault::check(path)?;
    let parent = path.parent().context("metadata path needs a parent")?;
    hellas_private::create_dir_all_durable(parent)?;
    hellas_private::write_atomically(path, ".writing", &serde_json::to_vec(value)?)?;
    Ok(())
}

#[cfg(test)]
pub(crate) mod publication_fault {
    use std::{
        collections::BTreeSet,
        path::{Path, PathBuf},
        sync::{Mutex, OnceLock},
    };
    static FAILURES: OnceLock<Mutex<BTreeSet<PathBuf>>> = OnceLock::new();
    pub fn fail_next_write(path: PathBuf) {
        FAILURES
            .get_or_init(Default::default)
            .lock()
            .unwrap()
            .insert(path);
    }
    pub(super) fn check(path: &Path) -> anyhow::Result<()> {
        anyhow::ensure!(
            !FAILURES
                .get_or_init(Default::default)
                .lock()
                .unwrap()
                .remove(path),
            "injected metadata publication failure"
        );
        Ok(())
    }
}
