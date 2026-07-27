use std::fs;
use std::path::{Path, PathBuf};

#[cfg(test)]
use crate::app_data_io::{atomic_write, lock_app_data_writes};
use crate::app_data_io::{atomic_write_locked, AppDataWriteGuard};
use crate::error::CommandResult;

const SECRET_DIR: &str = "secrets";

#[derive(Debug, Clone)]
pub struct AppDataCredentialStore {
  root: PathBuf,
}

impl AppDataCredentialStore {
  pub fn new(app_data_dir: impl AsRef<Path>) -> Self {
    Self { root: app_data_dir.as_ref().join(SECRET_DIR) }
  }

  #[cfg(test)]
  pub fn save_api_key(&self, provider_id: &str, api_key: &str) -> CommandResult<()> {
    atomic_write(&self.api_key_path(provider_id), api_key.as_bytes())
  }

  pub(crate) fn save_api_key_locked(
    &self,
    provider_id: &str,
    api_key: &str,
    guard: &AppDataWriteGuard,
  ) -> CommandResult<()> {
    atomic_write_locked(guard, &self.api_key_path(provider_id), api_key.as_bytes())
  }

  #[cfg(test)]
  pub fn clear_api_key(&self, provider_id: &str) -> CommandResult<()> {
    let guard = lock_app_data_writes();
    self.clear_api_key_locked(provider_id, &guard)
  }

  pub(crate) fn clear_api_key_locked(&self, provider_id: &str, _guard: &AppDataWriteGuard) -> CommandResult<()> {
    let path = self.api_key_path(provider_id);
    if path.exists() {
      fs::remove_file(path)?;
    }
    Ok(())
  }

  pub fn has_api_key(&self, provider_id: &str) -> bool {
    self.api_key_path(provider_id).exists()
  }

  pub fn read_api_key(&self, provider_id: &str) -> CommandResult<Option<String>> {
    let path = self.api_key_path(provider_id);
    if !path.exists() {
      return Ok(None);
    }
    let key = fs::read_to_string(path)?.trim().to_string();
    Ok((!key.is_empty()).then_some(key))
  }

  fn api_key_path(&self, provider_id: &str) -> PathBuf {
    self.root.join(format!("brain_{}_api_key", safe_id(provider_id)))
  }
}

fn safe_id(value: &str) -> String {
  value.chars().filter(|ch| ch.is_ascii_alphanumeric() || *ch == '_').collect()
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn ignores_dotenv_provider_keys() {
    let root = std::env::temp_dir().join(format!("soma-secret-dotenv-ignore-test-{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&root).unwrap();
    fs::write(root.join(".env"), "OPENROUTER_API_KEY=router-env-key\n").unwrap();
    let store = AppDataCredentialStore::new(&root);

    assert!(!store.has_api_key("openrouter"));
    assert_eq!(store.read_api_key("openrouter").unwrap(), None);
    let _ = fs::remove_dir_all(root);
  }

  #[test]
  fn stores_reads_and_clears_provider_api_key() {
    let root = std::env::temp_dir().join(format!("soma-secret-store-test-{}", uuid::Uuid::new_v4()));
    let store = AppDataCredentialStore::new(&root);

    store.save_api_key("openrouter", "router-secret").unwrap();
    assert!(store.has_api_key("openrouter"));
    assert_eq!(store.read_api_key("openrouter").unwrap(), Some("router-secret".to_string()));

    store.clear_api_key("openrouter").unwrap();
    assert!(!store.has_api_key("openrouter"));
    assert_eq!(store.read_api_key("openrouter").unwrap(), None);
    let _ = fs::remove_dir_all(root);
  }
}
