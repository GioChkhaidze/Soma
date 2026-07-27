use serde_json::Value;
use tauri::{AppHandle, Manager};
use tauri_plugin_dialog::DialogExt;

use crate::brain_settings::{
  get_brain_settings as read_brain_settings, load_brain_settings, save_brain_settings as persist_brain_settings,
  settings_from_value,
};
use crate::chat_turns::{
  send_graph_chat_turn_with_reading_context_and_credentials, send_node_chat_turn_with_credentials,
};
use crate::error::CommandResult;
use crate::jobs::{
  clear_job_history as remove_job_history,
  compile_graph_workspace_with_runtime_and_credentials as compile_workspace_graph,
  import_graph_patch_for_review as import_job_patch, list_jobs as read_jobs, open_job_folder as reveal_job_folder,
  run_compile_job_with_credentials as run_job,
};
use crate::repository::WorkspaceStore;
use soma_ai_runtime::{AiRuntimeError, CredentialRef, CredentialResolver};

use crate::runtime_adapters::list_runtime_models;
use crate::runtime_adapters::{
  authorize_codex_brain_status, codex_brain_status, runtime_descriptor, StoredCredentialResolver,
};
use crate::secrets::AppDataCredentialStore;
use crate::source_import::{import_source_file as import_source, workspace_stats};
use crate::workspace::{
  create_auto_workspace, current_workspace_paths, get_current_workspace as current_workspace_state,
  open_existing_workspace, require_current_workspace, workspace_state_from_paths, WorkspacePaths,
};

#[tauri::command]
pub async fn create_workspace_auto(app: AppHandle) -> CommandResult<Value> {
  tauri::async_runtime::spawn_blocking(move || create_auto_workspace(&app))
    .await
    .map_err(|error| crate::error::CommandError::storage(format!("Create workspace worker failed: {error}")))?
}

#[tauri::command]
pub async fn open_workspace_picker(app: AppHandle) -> CommandResult<Option<Value>> {
  let picker_app = app.clone();
  let selected = tauri::async_runtime::spawn_blocking(move || {
    picker_app.dialog().file().set_title("Open Soma workspace").blocking_pick_folder()
  })
  .await
  .map_err(|error| crate::error::CommandError::storage(format!("Workspace picker failed: {error}")))?;

  let Some(selected) = selected else {
    return Ok(None);
  };
  let workspace_dir =
    selected.into_path().map_err(|_| crate::error::CommandError::storage("Selected folder is not a local path."))?;
  tauri::async_runtime::spawn_blocking(move || open_existing_workspace(&app, &workspace_dir).map(Some))
    .await
    .map_err(|error| crate::error::CommandError::storage(format!("Open workspace worker failed: {error}")))?
}

#[tauri::command]
pub async fn get_current_workspace(app: AppHandle) -> CommandResult<Value> {
  blocking_command("Current workspace lookup failed", move || current_workspace_state(&app)).await
}

#[tauri::command]
pub async fn get_current_workspace_with_stats(app: AppHandle) -> CommandResult<Value> {
  let Some(paths) = current_workspace_paths(&app)? else {
    return Ok(workspace_state_from_paths(None));
  };

  captured_workspace_command(paths, "Workspace stats worker failed", workspace_state_with_stats).await
}

#[tauri::command]
pub fn get_brain_settings(app: AppHandle) -> CommandResult<Value> {
  read_brain_settings(&app)
}

#[tauri::command(rename_all = "snake_case")]
pub async fn list_brain_models(app: AppHandle, settings: Option<Value>) -> CommandResult<Value> {
  blocking_command("List brain models worker failed", move || {
    let data_dir = app.path().app_data_dir().map_err(|error| crate::error::CommandError::storage(error.to_string()))?;
    let store = AppDataCredentialStore::new(data_dir);
    let draft_key = settings
      .as_ref()
      .and_then(|value| value.get("apiKey"))
      .and_then(Value::as_str)
      .map(str::trim)
      .filter(|value| !value.is_empty())
      .map(str::to_string);
    let brain_settings = match settings {
      Some(value) => settings_from_value(&value, &store),
      None => load_brain_settings(&app)?,
    };
    let runtime = runtime_descriptor(&brain_settings);
    let credentials = ModelListCredentialResolver { store, draft_key };
    list_runtime_models(&runtime, &credentials)
  })
  .await
}

#[tauri::command(rename_all = "snake_case")]
pub fn save_brain_settings(app: AppHandle, settings: Value) -> CommandResult<Value> {
  persist_brain_settings(&app, settings)
}

#[tauri::command]
pub async fn authorize_codex_brain() -> CommandResult<Value> {
  blocking_command("Codex authorization worker failed", move || Ok(authorize_codex_brain_status())).await
}

#[tauri::command]
pub async fn enable_codex_brain(app: AppHandle, settings: Option<Value>) -> CommandResult<Value> {
  blocking_command("Enable Codex brain worker failed", move || {
    let status = codex_brain_status();
    if status.get("status").and_then(Value::as_str) != Some("ready") {
      return Ok(status);
    }

    let current = load_brain_settings(&app)?;
    let draft = settings.unwrap_or(Value::Null);
    let saved = persist_brain_settings(&app, codex_enable_settings_payload(&current, &draft))?;
    let mut status = status;
    status["settings"] = saved;
    Ok(status)
  })
  .await
}

#[tauri::command(rename_all = "snake_case")]
pub async fn import_source_file(app: AppHandle, source_path: String) -> CommandResult<Value> {
  let paths = require_current_workspace(&app)?;
  let source_path = normalize_source_path(&source_path);
  captured_workspace_command(paths, "Import source worker failed", move |paths| import_source(&paths, source_path))
    .await
}

#[tauri::command]
pub async fn compile_graph_workspace(app: AppHandle) -> CommandResult<Value> {
  let paths = require_current_workspace(&app)?;
  let runtime = runtime_descriptor(&load_brain_settings(&app)?);
  let credentials = stored_credentials(&app)?;
  captured_workspace_command(paths, "Compile worker failed", move |paths| {
    compile_workspace_graph(&paths, &runtime, &credentials)
  })
  .await
}

#[tauri::command]
pub async fn list_jobs(app: AppHandle) -> CommandResult<Value> {
  let paths = require_current_workspace(&app)?;
  captured_workspace_command(paths, "List jobs worker failed", move |paths| read_jobs(&paths)).await
}

#[tauri::command]
pub async fn clear_job_history(app: AppHandle) -> CommandResult<Value> {
  let paths = require_current_workspace(&app)?;
  captured_workspace_command(paths, "Clear job history worker failed", move |paths| remove_job_history(&paths)).await
}

#[tauri::command(rename_all = "snake_case")]
pub async fn open_job_folder(app: AppHandle, job_id: String) -> CommandResult<Value> {
  let paths = require_current_workspace(&app)?;
  captured_workspace_command(paths, "Open job folder worker failed", move |paths| reveal_job_folder(&paths, &job_id))
    .await
}

#[tauri::command(rename_all = "snake_case")]
pub async fn run_compile_job(app: AppHandle, job_id: String) -> CommandResult<Value> {
  let paths = require_current_workspace(&app)?;
  let runtime = runtime_descriptor(&load_brain_settings(&app)?);
  let credentials = stored_credentials(&app)?;
  captured_workspace_command(paths, "Compile worker failed", move |paths| {
    run_job(&paths, &job_id, &runtime, &credentials)
  })
  .await
}

#[tauri::command(rename_all = "snake_case")]
pub async fn import_graph_patch_for_review(app: AppHandle, job_id: String) -> CommandResult<Value> {
  let paths = require_current_workspace(&app)?;
  captured_workspace_command(paths, "Import graph patch worker failed", move |paths| import_job_patch(&paths, &job_id))
    .await
}

#[tauri::command]
pub async fn load_graph_canvas_snapshot(app: AppHandle) -> CommandResult<Value> {
  let paths = require_current_workspace(&app)?;
  captured_store_command(paths, "Graph canvas worker failed", |store| store.load_graph_canvas_snapshot()).await
}

#[tauri::command]
pub async fn load_workspace_bootstrap(app: AppHandle) -> CommandResult<Value> {
  let paths = require_current_workspace(&app)?;
  captured_workspace_command(paths, "Workspace bootstrap failed", |paths| {
    let store = WorkspaceStore::open_readonly(paths.database_path)?;
    store.load_workspace_bootstrap()
  })
  .await
}

#[tauri::command(rename_all = "snake_case")]
pub async fn load_graph_node_detail(app: AppHandle, node_id: String) -> CommandResult<Value> {
  let paths = require_current_workspace(&app)?;
  captured_store_command(paths, "Graph node detail worker failed", move |store| store.load_graph_node_detail(&node_id))
    .await
}

#[tauri::command(rename_all = "snake_case")]
pub async fn search_graph_node_cards(app: AppHandle, query: String, limit: usize) -> CommandResult<Value> {
  let paths = require_current_workspace(&app)?;
  captured_store_command(paths, "Graph node search worker failed", move |store| {
    store.search_graph_node_cards(&query, limit)
  })
  .await
}

#[tauri::command]
pub async fn load_review_queue(app: AppHandle) -> CommandResult<Value> {
  let paths = require_current_workspace(&app)?;
  captured_store_command(paths, "Review queue worker failed", |store| store.load_review_queue()).await
}

#[tauri::command(rename_all = "snake_case")]
pub async fn persist_node_position(
  app: AppHandle,
  node_id: String,
  x: f64,
  y: f64,
  pinned: Option<bool>,
) -> CommandResult<Value> {
  let paths = require_current_workspace(&app)?;
  captured_store_command(paths, "Persist node position worker failed", move |store| {
    store.persist_node_position(&node_id, x, y, pinned.unwrap_or(true))
  })
  .await
}

#[tauri::command(rename_all = "snake_case")]
pub async fn send_graph_chat_turn(
  app: AppHandle,
  content: String,
  focus_node_ids: Option<Vec<String>>,
  reading_context: Option<Value>,
  capture_graph_changes: Option<bool>,
) -> CommandResult<Value> {
  let paths = require_current_workspace(&app)?;
  let runtime = runtime_descriptor(&load_brain_settings(&app)?);
  let credentials = stored_credentials(&app)?;
  captured_workspace_command(paths, "Graph chat worker failed", move |paths| {
    send_graph_chat_turn_with_reading_context_and_credentials(
      &paths,
      &runtime,
      &content,
      focus_node_ids.unwrap_or_default(),
      reading_context,
      capture_graph_changes_or_default(capture_graph_changes),
      &credentials,
    )
  })
  .await
}

#[tauri::command]
pub async fn list_graph_messages(app: AppHandle) -> CommandResult<Value> {
  let paths = require_current_workspace(&app)?;
  captured_store_command(paths, "List graph messages worker failed", |store| store.list_graph_messages()).await
}

#[tauri::command(rename_all = "snake_case")]
pub async fn send_node_chat_turn(
  app: AppHandle,
  node_id: String,
  content: String,
  capture_graph_changes: Option<bool>,
) -> CommandResult<Value> {
  let paths = require_current_workspace(&app)?;
  let runtime = runtime_descriptor(&load_brain_settings(&app)?);
  let credentials = stored_credentials(&app)?;
  captured_workspace_command(paths, "Node chat worker failed", move |paths| {
    send_node_chat_turn_with_credentials(
      &paths,
      &runtime,
      &node_id,
      &content,
      capture_graph_changes_or_default(capture_graph_changes),
      &credentials,
    )
  })
  .await
}

#[tauri::command(rename_all = "snake_case")]
pub async fn list_node_messages(app: AppHandle, node_id: String) -> CommandResult<Value> {
  let paths = require_current_workspace(&app)?;
  captured_store_command(paths, "List node messages worker failed", move |store| store.list_node_messages(&node_id))
    .await
}

#[tauri::command(rename_all = "snake_case")]
pub async fn update_node_body(app: AppHandle, node_id: String, compiled_body: String) -> CommandResult<Value> {
  let paths = require_current_workspace(&app)?;
  captured_store_command(paths, "Update node body worker failed", move |store| {
    store.update_node_body(&node_id, &compiled_body)
  })
  .await
}

#[tauri::command(rename_all = "snake_case")]
pub async fn rollback_node_body(app: AppHandle, node_id: String, version_number: i64) -> CommandResult<Value> {
  let paths = require_current_workspace(&app)?;
  captured_store_command(paths, "Rollback node body worker failed", move |store| {
    store.rollback_node_body(&node_id, version_number)
  })
  .await
}

#[tauri::command(rename_all = "snake_case")]
pub async fn undo_graph_patch(app: AppHandle, patch_id: String) -> CommandResult<Value> {
  let paths = require_current_workspace(&app)?;
  captured_store_command(paths, "Undo graph update worker failed", move |store| store.undo_graph_patch(&patch_id)).await
}

#[tauri::command(rename_all = "snake_case")]
pub async fn accept_graph_proposal(app: AppHandle, proposal_id: String) -> CommandResult<Value> {
  let paths = require_current_workspace(&app)?;
  captured_store_command(paths, "Accept graph proposal worker failed", move |store| {
    store.accept_graph_proposal(&proposal_id, None)
  })
  .await
}

#[tauri::command(rename_all = "snake_case")]
pub async fn reject_graph_proposal(
  app: AppHandle,
  proposal_id: String,
  reason: Option<String>,
) -> CommandResult<Value> {
  let paths = require_current_workspace(&app)?;
  captured_store_command(paths, "Reject graph proposal worker failed", move |store| {
    store.reject_graph_proposal(&proposal_id, reason.as_deref())
  })
  .await
}

#[tauri::command(rename_all = "snake_case")]
pub async fn defer_graph_proposal(app: AppHandle, proposal_id: String, reason: Option<String>) -> CommandResult<Value> {
  let paths = require_current_workspace(&app)?;
  captured_store_command(paths, "Defer graph proposal worker failed", move |store| {
    store.defer_graph_proposal(&proposal_id, reason.as_deref())
  })
  .await
}

async fn blocking_command<T: Send + 'static>(
  failure: &'static str,
  action: impl FnOnce() -> CommandResult<T> + Send + 'static,
) -> CommandResult<T> {
  tauri::async_runtime::spawn_blocking(action)
    .await
    .map_err(|error| crate::error::CommandError::storage(format!("{failure}: {error}")))?
}

async fn captured_workspace_command(
  paths: WorkspacePaths,
  failure: &'static str,
  action: impl FnOnce(WorkspacePaths) -> CommandResult<Value> + Send + 'static,
) -> CommandResult<Value> {
  blocking_command(failure, move || action(paths)).await
}

async fn captured_store_command(
  paths: WorkspacePaths,
  failure: &'static str,
  action: impl FnOnce(&mut WorkspaceStore) -> CommandResult<Value> + Send + 'static,
) -> CommandResult<Value> {
  captured_workspace_command(paths, failure, move |paths| {
    let mut store = WorkspaceStore::open(paths.database_path)?;
    action(&mut store)
  })
  .await
}

fn stored_credentials(app: &AppHandle) -> CommandResult<StoredCredentialResolver> {
  let data_dir = app.path().app_data_dir().map_err(|error| crate::error::CommandError::storage(error.to_string()))?;
  Ok(StoredCredentialResolver::new(AppDataCredentialStore::new(data_dir)))
}

fn codex_enable_settings_payload(current: &crate::brain_settings::BrainSettings, draft: &Value) -> Value {
  serde_json::json!({
    "providerId": "codex_sdk",
    "model": draft.get("model").and_then(Value::as_str).unwrap_or(&current.model),
    "endpoint": "",
    "authProfile": draft.get("authProfile").and_then(Value::as_str).unwrap_or(&current.auth_profile)
  })
}

struct ModelListCredentialResolver {
  store: AppDataCredentialStore,
  draft_key: Option<String>,
}

impl CredentialResolver for ModelListCredentialResolver {
  fn resolve(&self, credential: &CredentialRef) -> Result<Option<String>, AiRuntimeError> {
    let CredentialRef::ApiKey { provider, .. } = credential else {
      return Ok(None);
    };
    if let Some(key) = self.draft_key.as_deref().filter(|value| !value.is_empty()) {
      return Ok(Some(key.to_string()));
    }
    self
      .store
      .read_api_key(provider.as_str())
      .map_err(|error| AiRuntimeError::CredentialResolution { credential: credential.clone(), message: error.message })
  }
}

fn workspace_state_with_stats(paths: WorkspacePaths) -> CommandResult<Value> {
  let mut state = workspace_state_from_paths(Some(&paths));
  state["stats"] = workspace_stats(&paths)?;
  Ok(state)
}

fn normalize_source_path(value: &str) -> String {
  value.trim().trim_matches('"').trim().to_string()
}

fn capture_graph_changes_or_default(value: Option<bool>) -> bool {
  value.unwrap_or(false)
}

#[cfg(test)]
mod tests {
  use super::{capture_graph_changes_or_default, codex_enable_settings_payload, normalize_source_path};
  use crate::brain_settings::BrainSettings;
  use serde_json::json;

  #[test]
  fn normalize_source_path_removes_windows_copy_path_quotes() {
    assert_eq!(normalize_source_path(r#"  "D:\exports\conversation.json"  "#), r#"D:\exports\conversation.json"#);
  }

  #[test]
  fn omitted_chat_capture_defaults_off_at_the_command_boundary() {
    assert!(!capture_graph_changes_or_default(None));
    assert!(!capture_graph_changes_or_default(Some(false)));
    assert!(capture_graph_changes_or_default(Some(true)));
  }

  #[test]
  fn codex_enable_payload_uses_visible_draft_model_and_profile() {
    let current = BrainSettings {
      provider_id: "openrouter".to_string(),
      model: "openai/gpt-5.5".to_string(),
      endpoint: "https://openrouter.ai/api/v1".to_string(),
      auth_profile: "router".to_string(),
      credential_configured: true,
      updated_at: Some("2026-07-04T00:00:00Z".to_string()),
    };
    let payload = codex_enable_settings_payload(
      &current,
      &json!({
        "model": "gpt-5.4",
        "authProfile": "work"
      }),
    );

    assert_eq!(payload["providerId"], "codex_sdk");
    assert_eq!(payload["model"], "gpt-5.4");
    assert_eq!(payload["authProfile"], "work");
    assert_eq!(payload["endpoint"], "");
  }
}
