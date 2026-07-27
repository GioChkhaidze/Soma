use std::fs;
use std::path::{Path, PathBuf};

use serde_json::{json, Value};
use tauri::{AppHandle, Manager};
use time::format_description::well_known::Rfc3339;
use time::OffsetDateTime;

use crate::app_data_io::{atomic_write_locked, lock_app_data_writes, AppDataWriteGuard};
use crate::brain_provider_registry::{brain_provider, is_known_provider, DEFAULT_PROVIDER_ID};
use crate::error::{CommandError, CommandResult};
use crate::secrets::AppDataCredentialStore;

const SETTINGS_FILE: &str = "brain_settings.json";

#[derive(Debug, Clone)]
pub struct BrainSettings {
  pub provider_id: String,
  pub model: String,
  pub endpoint: String,
  pub auth_profile: String,
  pub credential_configured: bool,
  pub updated_at: Option<String>,
}

impl BrainSettings {
  pub fn default() -> Self {
    Self {
      provider_id: DEFAULT_PROVIDER_ID.to_string(),
      model: String::new(),
      endpoint: String::new(),
      auth_profile: String::new(),
      credential_configured: false,
      updated_at: None,
    }
  }

  pub fn to_public_json(&self) -> Value {
    json!({
      "providerId": self.provider_id,
      "model": self.model,
      "endpoint": self.endpoint,
      "authProfile": self.auth_profile,
      "credentialConfigured": self.credential_configured,
      "updatedAt": self.updated_at
    })
  }
}

pub fn get_brain_settings(app: &AppHandle) -> CommandResult<Value> {
  Ok(load_brain_settings(app)?.to_public_json())
}

pub fn save_brain_settings(app: &AppHandle, settings: Value) -> CommandResult<Value> {
  let data_dir = app_data_dir(app)?;
  let saved = save_brain_settings_in_dir(&data_dir, settings)?;
  Ok(saved.to_public_json())
}

pub fn load_brain_settings(app: &AppHandle) -> CommandResult<BrainSettings> {
  load_brain_settings_from_dir(&app_data_dir(app)?)
}

pub(crate) fn load_brain_settings_from_dir(data_dir: &Path) -> CommandResult<BrainSettings> {
  let settings_path = settings_path(data_dir);
  let store = AppDataCredentialStore::new(data_dir);
  if !settings_path.exists() {
    let mut settings = BrainSettings::default();
    settings.credential_configured = store.has_api_key(&settings.provider_id);
    return Ok(settings);
  }

  let raw: Value = serde_json::from_str(&fs::read_to_string(settings_path)?)
    .map_err(|error| CommandError::storage(error.to_string()))?;
  Ok(settings_from_value(&raw, &store))
}

pub(crate) fn save_brain_settings_in_dir(data_dir: &Path, payload: Value) -> CommandResult<BrainSettings> {
  let guard = lock_app_data_writes();
  fs::create_dir_all(data_dir)?;
  let store = AppDataCredentialStore::new(data_dir);
  let mut settings = settings_from_value(&payload, &store);

  if payload.get("clearApiKey").and_then(Value::as_bool).unwrap_or(false) {
    store.clear_api_key_locked(&settings.provider_id, &guard)?;
  }

  if let Some(api_key) = payload.get("apiKey").and_then(Value::as_str) {
    let api_key = api_key.trim();
    if !api_key.is_empty() {
      store.save_api_key_locked(&settings.provider_id, api_key, &guard)?;
    }
  }

  settings.updated_at = Some(now_string()?);
  settings.credential_configured = store.has_api_key(&settings.provider_id);
  write_settings_file_locked(data_dir, &settings, &guard)?;
  Ok(settings)
}

fn app_data_dir(app: &AppHandle) -> CommandResult<PathBuf> {
  let data_dir = app.path().app_data_dir().map_err(|error| CommandError::storage(error.to_string()))?;
  fs::create_dir_all(&data_dir)?;
  Ok(data_dir)
}

fn settings_path(data_dir: &Path) -> PathBuf {
  data_dir.join(SETTINGS_FILE)
}

pub(crate) fn settings_from_value(value: &Value, store: &AppDataCredentialStore) -> BrainSettings {
  let provider_id =
    normalize_provider_id(value.get("providerId").or_else(|| value.get("provider_id")).and_then(Value::as_str));
  let auth_profile = if provider_id == "codex_sdk" {
    value
      .get("authProfile")
      .or_else(|| value.get("auth_profile"))
      .and_then(Value::as_str)
      .unwrap_or("")
      .trim()
      .to_string()
  } else {
    String::new()
  };
  let endpoint = endpoint_override(&provider_id, value);

  BrainSettings {
    credential_configured: store.has_api_key(&provider_id),
    provider_id,
    model: string_field(value, "model"),
    endpoint,
    auth_profile,
    updated_at: value.get("updatedAt").or_else(|| value.get("updated_at")).and_then(Value::as_str).map(str::to_string),
  }
}

#[cfg(test)]
fn write_settings_file(data_dir: &Path, settings: &BrainSettings) -> CommandResult<()> {
  let guard = lock_app_data_writes();
  write_settings_file_locked(data_dir, settings, &guard)
}

fn write_settings_file_locked(
  data_dir: &Path,
  settings: &BrainSettings,
  guard: &AppDataWriteGuard,
) -> CommandResult<()> {
  let value = json!({
    "providerId": settings.provider_id,
    "model": settings.model,
    "endpoint": settings.endpoint,
    "authProfile": settings.auth_profile,
    "updatedAt": settings.updated_at
  });
  let contents =
    format!("{}\n", serde_json::to_string_pretty(&value).map_err(|error| CommandError::storage(error.to_string()))?);
  atomic_write_locked(guard, &settings_path(data_dir), contents.as_bytes())
}

fn string_field(value: &Value, key: &str) -> String {
  value.get(key).and_then(Value::as_str).unwrap_or("").trim().to_string()
}

fn endpoint_override(provider_id: &str, value: &Value) -> String {
  let endpoint = string_field(value, "endpoint");
  let is_provider_default = brain_provider(provider_id)
    .and_then(|provider| provider.default_endpoint)
    .is_some_and(|default| endpoint == default);
  if is_provider_default {
    String::new()
  } else {
    endpoint
  }
}

fn normalize_provider_id(value: Option<&str>) -> String {
  let provider_id = value.unwrap_or(DEFAULT_PROVIDER_ID);
  if is_known_provider(provider_id) {
    provider_id.to_string()
  } else {
    DEFAULT_PROVIDER_ID.to_string()
  }
}

fn now_string() -> CommandResult<String> {
  Ok(OffsetDateTime::now_utc().format(&Rfc3339)?)
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn saves_brain_settings_without_persisting_raw_api_key_in_settings_json() {
    let root = std::env::temp_dir().join(format!("soma-brain-settings-test-{}", uuid::Uuid::new_v4()));
    let saved = save_brain_settings_in_dir(
      &root,
      json!({
        "providerId": "openai",
        "model": "gpt-test",
        "endpoint": "https://api.example.test",
        "authProfile": "default",
        "apiKey": "sk-secret-test"
      }),
    )
    .unwrap();

    assert_eq!(saved.provider_id, "openai");
    assert!(saved.credential_configured);
    let settings_json = fs::read_to_string(root.join(SETTINGS_FILE)).unwrap();
    assert!(!settings_json.contains("sk-secret-test"));
    assert!(!settings_json.contains("apiKey"));

    let loaded = load_brain_settings_from_dir(&root).unwrap();
    assert_eq!(loaded.model, "gpt-test");
    assert!(loaded.credential_configured);
    let _ = fs::remove_dir_all(root);
  }

  #[test]
  fn clears_provider_api_key_without_clearing_runtime_settings() {
    let root = std::env::temp_dir().join(format!("soma-brain-settings-clear-test-{}", uuid::Uuid::new_v4()));
    save_brain_settings_in_dir(
      &root,
      json!({
        "providerId": "openrouter",
        "model": "router-model",
        "endpoint": "",
        "authProfile": "",
        "apiKey": "router-secret"
      }),
    )
    .unwrap();
    let cleared = save_brain_settings_in_dir(
      &root,
      json!({
        "providerId": "openrouter",
        "model": "router-model",
        "endpoint": "",
        "authProfile": "",
        "clearApiKey": true
      }),
    )
    .unwrap();

    assert_eq!(cleared.model, "router-model");
    assert!(!cleared.credential_configured);
    let _ = fs::remove_dir_all(root);
  }

  #[test]
  fn legacy_job_folder_preference_is_ignored() {
    let root = std::env::temp_dir().join(format!("soma-brain-settings-legacy-test-{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&root).unwrap();
    fs::write(
      root.join(SETTINGS_FILE),
      r#"{
        "providerId": "openrouter",
        "model": "legacy-model",
        "endpoint": "",
        "authProfile": "",
        "useJobFolderCompiler": false,
        "updatedAt": "2026-07-04T00:00:00Z"
      }"#,
    )
    .unwrap();

    let loaded = load_brain_settings_from_dir(&root).unwrap();

    assert_eq!(loaded.provider_id, "openrouter");
    assert_eq!(loaded.model, "legacy-model");
    assert!(loaded.to_public_json().get("useJobFolderCompiler").is_none());
    write_settings_file(&root, &loaded).unwrap();
    assert!(!fs::read_to_string(root.join(SETTINGS_FILE)).unwrap().contains("useJobFolderCompiler"));
    let _ = fs::remove_dir_all(root);
  }

  #[test]
  fn legacy_claude_code_auth_profile_is_ignored() {
    let root = std::env::temp_dir().join(format!("soma-brain-settings-claude-profile-{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&root).unwrap();
    fs::write(
      root.join(SETTINGS_FILE),
      r#"{
        "providerId": "claude_code",
        "model": "sonnet",
        "endpoint": "",
        "authProfile": "legacy-profile",
        "updatedAt": "2026-07-04T00:00:00Z"
      }"#,
    )
    .unwrap();

    let loaded = load_brain_settings_from_dir(&root).unwrap();

    assert_eq!(loaded.provider_id, "claude_code");
    assert!(loaded.auth_profile.is_empty());
    assert_eq!(loaded.to_public_json()["authProfile"], "");
    let _ = fs::remove_dir_all(root);
  }

  #[test]
  fn canonical_provider_endpoint_is_not_stored_as_a_custom_override() {
    let root = std::env::temp_dir().join(format!("soma-brain-settings-endpoint-{}", uuid::Uuid::new_v4()));
    let saved = save_brain_settings_in_dir(
      &root,
      json!({
        "providerId": "vercel_ai_gateway",
        "model": "xai/grok-4.3",
        "endpoint": "https://ai-gateway.vercel.sh/v1",
        "authProfile": ""
      }),
    )
    .unwrap();

    assert!(saved.endpoint.is_empty());
    assert_eq!(saved.to_public_json()["endpoint"], "");
    assert_eq!(
      serde_json::from_str::<Value>(&fs::read_to_string(root.join(SETTINGS_FILE)).unwrap()).unwrap()["endpoint"],
      ""
    );
    let _ = fs::remove_dir_all(root);
  }

  #[test]
  fn concurrent_settings_and_credential_saves_stay_correlated() {
    let root = std::env::temp_dir().join(format!("soma-brain-settings-concurrency-{}", uuid::Uuid::new_v4()));
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(12));
    let threads: Vec<_> = (0..12)
      .map(|index| {
        let barrier = std::sync::Arc::clone(&barrier);
        let root = root.clone();
        std::thread::spawn(move || {
          barrier.wait();
          save_brain_settings_in_dir(
            &root,
            json!({
              "providerId": "openrouter",
              "model": format!("model-{index}"),
              "endpoint": "",
              "authProfile": "",
              "apiKey": format!("key-{index}")
            }),
          )
          .unwrap();
        })
      })
      .collect();
    for thread in threads {
      thread.join().unwrap();
    }

    let settings = load_brain_settings_from_dir(&root).unwrap();
    let index = settings.model.strip_prefix("model-").unwrap();
    let key = AppDataCredentialStore::new(&root).read_api_key("openrouter").unwrap();
    assert_eq!(key, Some(format!("key-{index}")));
    let _ = fs::remove_dir_all(root);
  }
}
