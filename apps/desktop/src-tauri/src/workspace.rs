use std::fs;
use std::path::{Path, PathBuf};

use serde_json::{json, Value};
use tauri::{AppHandle, Manager};

use crate::app_data_io::{atomic_write, lock_app_data_writes};
use crate::database::{open_database, open_existing_database_readonly, validate_existing_soma_database};
use crate::error::{CommandError, CommandResult};

pub const DB_FILE: &str = "soma.sqlite";
pub const RAW_IMPORT_DIR: &str = "raw/imports";
pub const JOB_DIR: &str = "jobs";
const CURRENT_WORKSPACE_FILE: &str = "current_workspace.json";
const DEFAULT_WORKSPACE_DIR: &str = "Soma Workspace";
const MANAGED_WORKSPACES_DIR: &str = "workspaces";

#[derive(Debug, Clone)]
pub struct WorkspacePaths {
  pub workspace_dir: PathBuf,
  pub database_path: PathBuf,
}

pub fn create_workspace_dir(workspace_dir: impl AsRef<Path>) -> CommandResult<WorkspacePaths> {
  let workspace_dir = workspace_dir.as_ref();
  fs::create_dir_all(workspace_dir.join(RAW_IMPORT_DIR))?;
  fs::create_dir_all(workspace_dir.join(JOB_DIR))?;
  fs::create_dir_all(workspace_dir.join("exports"))?;
  let database_path = workspace_dir.join(DB_FILE);
  let conn = open_database(&database_path)?;
  drop(conn);
  let workspace_dir = workspace_dir.canonicalize().unwrap_or_else(|_| workspace_dir.to_path_buf());
  Ok(WorkspacePaths { database_path: workspace_dir.join(DB_FILE), workspace_dir })
}

pub fn open_workspace_dir(workspace_dir: impl AsRef<Path>) -> CommandResult<WorkspacePaths> {
  create_workspace_dir(workspace_dir)
}

pub fn create_auto_workspace(app: &AppHandle) -> CommandResult<Value> {
  let data_dir = app.path().app_data_dir().map_err(|error| CommandError::storage(error.to_string()))?;
  let workspace_dir = new_managed_workspace_dir(&data_dir);
  set_current_workspace(app, &workspace_dir)
}

pub fn open_existing_workspace(app: &AppHandle, workspace_dir: &Path) -> CommandResult<Value> {
  let workspace_dir = resolve_workspace_dir(app, workspace_dir)?;
  validate_existing_workspace_dir(&workspace_dir)?;
  set_current_workspace(app, &workspace_dir)
}

pub fn set_current_workspace(app: &AppHandle, workspace_dir: &Path) -> CommandResult<Value> {
  let workspace_dir = resolve_workspace_dir(app, workspace_dir)?;
  let paths = open_workspace_dir(&workspace_dir)?;
  let state_path = current_workspace_state_path(app)?;
  let state = format!(
    "{}\n",
    serde_json::to_string_pretty(&json!({
      "workspace_dir": paths.workspace_dir.to_string_lossy(),
      "database_path": paths.database_path.to_string_lossy()
    }))
    .map_err(|error| CommandError::storage(error.to_string()))?
  );
  atomic_write(&state_path, state.as_bytes())?;
  Ok(workspace_state_from_paths(Some(&paths)))
}

pub fn get_current_workspace(app: &AppHandle) -> CommandResult<Value> {
  let paths = current_workspace_shell_paths(app)?;
  Ok(workspace_state_from_paths(paths.as_ref()))
}

pub fn current_workspace_paths(app: &AppHandle) -> CommandResult<Option<WorkspacePaths>> {
  let state_path = current_workspace_state_path(app)?;
  if !state_path.exists() {
    return Ok(None);
  }
  let value: Value =
    serde_json::from_str(&fs::read_to_string(state_path)?).map_err(|error| CommandError::storage(error.to_string()))?;
  let Some(workspace_dir) = value.get("workspace_dir").and_then(Value::as_str) else {
    return Ok(None);
  };
  let workspace_dir = resolve_workspace_dir(app, Path::new(workspace_dir))?;
  if !is_existing_workspace_dir(&workspace_dir) {
    return Ok(None);
  }
  validate_existing_soma_database(workspace_dir.join(DB_FILE))?;

  let workspace_dir = workspace_dir.canonicalize().unwrap_or_else(|_| workspace_dir.to_path_buf());
  Ok(Some(WorkspacePaths { database_path: workspace_dir.join(DB_FILE), workspace_dir }))
}

fn current_workspace_shell_paths(app: &AppHandle) -> CommandResult<Option<WorkspacePaths>> {
  let state_path = current_workspace_state_path(app)?;
  if !state_path.exists() {
    return Ok(None);
  }
  let value: Value =
    serde_json::from_str(&fs::read_to_string(state_path)?).map_err(|error| CommandError::storage(error.to_string()))?;
  let data_dir = app.path().app_data_dir().map_err(|error| CommandError::storage(error.to_string()))?;
  current_workspace_shell_paths_for_base(&value, &data_dir, std::env::current_dir().ok().as_deref())
}

fn current_workspace_shell_paths_for_base(
  value: &Value,
  app_data_dir: &Path,
  current_dir: Option<&Path>,
) -> CommandResult<Option<WorkspacePaths>> {
  let workspace_dir = value
    .get("workspace_dir")
    .and_then(Value::as_str)
    .filter(|value| !value.trim().is_empty())
    .map(Path::new)
    .ok_or_else(|| CommandError::storage("Current workspace state does not contain a valid workspace path."))?;
  let resolved = resolve_workspace_dir_for_base(app_data_dir, workspace_dir, current_dir);
  let resolved_exists = validate_workspace_if_present(&resolved)?;
  if !resolved_exists && !legacy_dev_workspace_exists(workspace_dir, &resolved)? {
    return Ok(None);
  }

  let workspace_dir = resolved.canonicalize().unwrap_or(resolved);
  Ok(Some(WorkspacePaths { database_path: workspace_dir.join(DB_FILE), workspace_dir }))
}

fn legacy_dev_workspace_exists(workspace_dir: &Path, resolved: &Path) -> CommandResult<bool> {
  if resolved == workspace_dir || !is_default_workspace_under_tauri_source_dir(workspace_dir) {
    return Ok(false);
  }
  validate_workspace_if_present(workspace_dir)
}

fn validate_workspace_if_present(workspace_dir: &Path) -> CommandResult<bool> {
  if !workspace_dir.join(DB_FILE).try_exists()? {
    return Ok(false);
  }
  validate_existing_workspace_dir(workspace_dir)?;
  Ok(true)
}

pub fn require_current_workspace(app: &AppHandle) -> CommandResult<WorkspacePaths> {
  current_workspace_paths(app)?.ok_or_else(|| {
    CommandError::new("Soma_WORKSPACE_NOT_OPEN", "No Soma workspace is open. Create or open a workspace first.")
  })
}

fn current_workspace_state_path(app: &AppHandle) -> CommandResult<PathBuf> {
  let data_dir = app.path().app_data_dir().map_err(|error| CommandError::storage(error.to_string()))?;
  fs::create_dir_all(&data_dir)?;
  Ok(data_dir.join(CURRENT_WORKSPACE_FILE))
}

fn resolve_workspace_dir(app: &AppHandle, workspace_dir: &Path) -> CommandResult<PathBuf> {
  let data_dir = app.path().app_data_dir().map_err(|error| CommandError::storage(error.to_string()))?;
  let resolved = resolve_workspace_dir_for_base(&data_dir, workspace_dir, std::env::current_dir().ok().as_deref());

  if resolved != workspace_dir && is_default_workspace_under_tauri_source_dir(workspace_dir) {
    migrate_dev_default_workspace(workspace_dir, &resolved)?;
  }

  Ok(resolved)
}

fn resolve_workspace_dir_for_base(app_data_dir: &Path, workspace_dir: &Path, current_dir: Option<&Path>) -> PathBuf {
  if !workspace_dir.is_absolute() {
    return app_data_dir.join(workspace_dir);
  }

  if is_default_workspace_under_current_dir(workspace_dir, current_dir)
    || is_default_workspace_under_tauri_source_dir(workspace_dir)
  {
    return app_data_dir.join(DEFAULT_WORKSPACE_DIR);
  }

  workspace_dir.to_path_buf()
}

fn new_managed_workspace_dir(app_data_dir: &Path) -> PathBuf {
  managed_workspace_dir_for_base(app_data_dir, &format!("workspace-{}", uuid::Uuid::new_v4()))
}

fn managed_workspace_dir_for_base(app_data_dir: &Path, workspace_id: &str) -> PathBuf {
  app_data_dir.join(MANAGED_WORKSPACES_DIR).join(workspace_id)
}

fn is_existing_workspace_dir(workspace_dir: &Path) -> bool {
  workspace_dir.join(DB_FILE).is_file()
}

fn validate_existing_workspace_dir(workspace_dir: &Path) -> CommandResult<()> {
  if !is_existing_workspace_dir(workspace_dir) {
    return Err(CommandError::new("SOMA_WORKSPACE_NOT_FOUND", "Selected folder is not a Soma workspace."));
  }
  validate_existing_soma_database(workspace_dir.join(DB_FILE))
}

fn is_default_workspace_under_current_dir(workspace_dir: &Path, current_dir: Option<&Path>) -> bool {
  if workspace_dir.file_name().and_then(|value| value.to_str()) != Some(DEFAULT_WORKSPACE_DIR) {
    return false;
  }
  let Some(current_dir) = current_dir else {
    return false;
  };
  paths_match(workspace_dir, &current_dir.join(DEFAULT_WORKSPACE_DIR))
}

fn is_default_workspace_under_tauri_source_dir(workspace_dir: &Path) -> bool {
  if workspace_dir.file_name().and_then(|value| value.to_str()) != Some(DEFAULT_WORKSPACE_DIR) {
    return false;
  }

  workspace_dir.parent().and_then(Path::file_name).and_then(|value| value.to_str()) == Some("src-tauri")
}

fn migrate_dev_default_workspace(source: &Path, target: &Path) -> CommandResult<()> {
  migrate_dev_default_workspace_with(source, target, copy_dir_contents)
}

fn migrate_dev_default_workspace_with(
  source: &Path,
  target: &Path,
  copy: impl FnOnce(&Path, &Path) -> CommandResult<()>,
) -> CommandResult<()> {
  let source_db = source.join(DB_FILE);
  if !source_db.exists() || !directory_is_empty_or_missing(target)? {
    return Ok(());
  }
  if database_source_count(&source_db)? < 1 {
    return Ok(());
  }

  let parent =
    target.parent().ok_or_else(|| CommandError::storage("Legacy workspace target must have a parent directory."))?;
  fs::create_dir_all(parent)?;
  let target_name = target.file_name().and_then(|value| value.to_str()).unwrap_or("workspace");
  let staging = parent.join(format!(".{target_name}.migration-{}", uuid::Uuid::new_v4()));
  fs::create_dir(&staging)?;

  let result = (|| -> CommandResult<()> {
    copy(source, &staging)?;
    let _guard = lock_app_data_writes();
    if !directory_is_empty_or_missing(target)? {
      return Ok(());
    }
    if target.exists() {
      fs::remove_dir(target)?;
    }
    fs::rename(&staging, target)?;
    Ok(())
  })();

  if staging.exists() {
    let cleanup = fs::remove_dir_all(&staging);
    if result.is_ok() {
      cleanup?;
    }
  }
  result
}

fn directory_is_empty_or_missing(path: &Path) -> CommandResult<bool> {
  if !path.exists() {
    return Ok(true);
  }
  if !path.is_dir() {
    return Ok(false);
  }
  Ok(fs::read_dir(path)?.next().transpose()?.is_none())
}

fn copy_dir_contents(source: &Path, target: &Path) -> CommandResult<()> {
  fs::create_dir_all(target)?;
  for entry in fs::read_dir(source)? {
    let entry = entry?;
    let from = entry.path();
    let to = target.join(entry.file_name());
    if entry.file_type()?.is_dir() {
      copy_dir_contents(&from, &to)?;
    } else {
      if let Some(parent) = to.parent() {
        fs::create_dir_all(parent)?;
      }
      fs::copy(&from, &to)?;
    }
  }
  Ok(())
}

fn database_source_count(database_path: &Path) -> CommandResult<i64> {
  let conn = open_existing_database_readonly(database_path)?;
  Ok(conn.query_row("SELECT COUNT(*) FROM sources", [], |row| row.get(0))?)
}

fn paths_match(left: &Path, right: &Path) -> bool {
  let left = left.canonicalize().unwrap_or_else(|_| left.to_path_buf());
  let right = right.canonicalize().unwrap_or_else(|_| right.to_path_buf());
  left == right
}

pub fn workspace_state_from_paths(paths: Option<&WorkspacePaths>) -> Value {
  match paths {
    Some(paths) => json!({
      "has_workspace": true,
      "workspace_dir": paths.workspace_dir.to_string_lossy(),
      "database_path": paths.database_path.to_string_lossy()
    }),
    None => json!({
      "has_workspace": false,
      "workspace_dir": null,
      "database_path": null
    }),
  }
}

#[cfg(test)]
mod workspace_path_tests {
  use super::*;
  use rusqlite::Connection;

  const INSERT_SOURCE_SQL: &str = concat!(
    "INSERT INTO sources (id, source_type, title, original_path, raw_path, imported_at) ",
    "VALUES (?1, ?2, ?3, ?4, ?5, ?6)"
  );

  #[test]
  fn resolves_relative_workspace_under_app_data() {
    let app_data_dir = PathBuf::from("C:/Users/test/AppData/Roaming/soma");
    let resolved = resolve_workspace_dir_for_base(
      &app_data_dir,
      Path::new(DEFAULT_WORKSPACE_DIR),
      Some(Path::new("D:/project/apps/desktop/src-tauri")),
    );

    assert_eq!(resolved, app_data_dir.join(DEFAULT_WORKSPACE_DIR));
  }

  #[test]
  fn preserves_explicit_absolute_workspace_paths() {
    let app_data_dir = PathBuf::from("C:/Users/test/AppData/Roaming/soma");
    let workspace_dir = PathBuf::from("D:/workspaces/brain.soma");

    let resolved = resolve_workspace_dir_for_base(
      &app_data_dir,
      &workspace_dir,
      Some(Path::new("D:/project/apps/desktop/src-tauri")),
    );

    assert_eq!(resolved, workspace_dir);
  }

  #[test]
  fn redirects_old_dev_default_workspace_out_of_watched_source_dir() {
    let root = std::env::temp_dir().join(format!("soma-workspace-path-test-{}", uuid::Uuid::new_v4()));
    let current_dir = root.join("apps/desktop/src-tauri");
    let app_data_dir = root.join("app-data");
    let dev_default = current_dir.join(DEFAULT_WORKSPACE_DIR);
    fs::create_dir_all(&dev_default).unwrap();

    let resolved = resolve_workspace_dir_for_base(&app_data_dir, &dev_default, Some(&current_dir));

    assert_eq!(resolved, app_data_dir.join(DEFAULT_WORKSPACE_DIR));
    let _ = fs::remove_dir_all(root);
  }

  #[test]
  fn redirects_old_dev_default_workspace_even_when_current_dir_is_repo_root() {
    let root = std::env::temp_dir().join(format!("soma-workspace-path-test-{}", uuid::Uuid::new_v4()));
    let current_dir = root.clone();
    let app_data_dir = root.join("app-data");
    let dev_default = root.join("apps/desktop/src-tauri").join(DEFAULT_WORKSPACE_DIR);
    fs::create_dir_all(&dev_default).unwrap();

    let resolved = resolve_workspace_dir_for_base(&app_data_dir, &dev_default, Some(&current_dir));

    assert_eq!(resolved, app_data_dir.join(DEFAULT_WORKSPACE_DIR));
    let _ = fs::remove_dir_all(root);
  }

  #[test]
  fn shell_lookup_redirects_old_dev_workspace_without_migrating() {
    let root = std::env::temp_dir().join(format!("soma-workspace-shell-test-{}", uuid::Uuid::new_v4()));
    let current_dir = root.join("apps/desktop/src-tauri");
    let app_data_dir = root.join("app-data");
    let source = current_dir.join(DEFAULT_WORKSPACE_DIR);
    let source_paths = create_workspace_dir(&source).unwrap();
    let imported_at = "2026-01-01T00:00:00Z";
    let conn = open_database(&source_paths.database_path).unwrap();
    conn
      .execute(
        INSERT_SOURCE_SQL,
        ("source_1", "text", "chat.txt", "D:/chat.txt", "raw/imports/source_1-chat.txt", imported_at),
      )
      .unwrap();
    drop(conn);

    let state = serde_json::json!({
        "workspace_dir": source.to_string_lossy()
    });
    let shell_paths =
      current_workspace_shell_paths_for_base(&state, &app_data_dir, Some(&current_dir)).unwrap().unwrap();

    assert_eq!(shell_paths.workspace_dir, app_data_dir.join(DEFAULT_WORKSPACE_DIR));
    assert!(!shell_paths.database_path.exists());
    assert!(!app_data_dir.join(DEFAULT_WORKSPACE_DIR).exists());
    let _ = fs::remove_dir_all(root);
  }

  #[test]
  fn shell_lookup_rejects_a_corrupt_legacy_workspace_without_migrating() {
    let root = std::env::temp_dir().join(format!("soma-workspace-shell-legacy-error-test-{}", uuid::Uuid::new_v4()));
    let current_dir = root.join("apps/desktop/src-tauri");
    let app_data_dir = root.join("app-data");
    let source = current_dir.join(DEFAULT_WORKSPACE_DIR);
    let database_path = source.join(DB_FILE);
    fs::create_dir_all(&source).unwrap();
    fs::write(&database_path, b"not a sqlite database").unwrap();
    let before = fs::read(&database_path).unwrap();
    let state = serde_json::json!({
      "workspace_dir": source.to_string_lossy()
    });

    let error = current_workspace_shell_paths_for_base(&state, &app_data_dir, Some(&current_dir)).unwrap_err();

    assert_eq!(error.code, "SOMA_WORKSPACE_NOT_FOUND");
    assert_eq!(fs::read(&database_path).unwrap(), before);
    assert!(!app_data_dir.join(DEFAULT_WORKSPACE_DIR).exists());
    let _ = fs::remove_dir_all(root);
  }

  #[test]
  fn shell_lookup_returns_none_when_the_saved_workspace_database_is_absent() {
    let root = std::env::temp_dir().join(format!("soma-workspace-shell-missing-test-{}", uuid::Uuid::new_v4()));
    let app_data_dir = root.join("app-data");
    let workspace_dir = root.join("missing-workspace");
    let state = serde_json::json!({
      "workspace_dir": workspace_dir.to_string_lossy()
    });

    let shell_paths = current_workspace_shell_paths_for_base(&state, &app_data_dir, None).unwrap();

    assert!(shell_paths.is_none());
    assert!(!workspace_dir.exists());
    assert!(!app_data_dir.exists());
    let _ = fs::remove_dir_all(root);
  }

  #[test]
  fn shell_lookup_rejects_a_corrupt_database_without_mutating_it() {
    let root = std::env::temp_dir().join(format!("soma-workspace-shell-corrupt-test-{}", uuid::Uuid::new_v4()));
    let app_data_dir = root.join("app-data");
    let workspace_dir = root.join("workspace");
    let database_path = workspace_dir.join(DB_FILE);
    fs::create_dir_all(&workspace_dir).unwrap();
    fs::write(&database_path, b"not a sqlite database").unwrap();
    let before = fs::read(&database_path).unwrap();
    let state = serde_json::json!({
      "workspace_dir": workspace_dir.to_string_lossy()
    });

    let error = current_workspace_shell_paths_for_base(&state, &app_data_dir, None).unwrap_err();

    assert_eq!(error.code, "SOMA_WORKSPACE_NOT_FOUND");
    assert_eq!(fs::read(&database_path).unwrap(), before);
    assert!(!workspace_dir.join(RAW_IMPORT_DIR).exists());
    assert!(!workspace_dir.join(JOB_DIR).exists());
    assert!(!workspace_dir.join("exports").exists());
    let _ = fs::remove_dir_all(root);
  }

  #[test]
  fn shell_lookup_rejects_an_unrelated_database_without_mutating_it() {
    let root = std::env::temp_dir().join(format!("soma-workspace-shell-unrelated-test-{}", uuid::Uuid::new_v4()));
    let app_data_dir = root.join("app-data");
    let workspace_dir = root.join("workspace");
    let database_path = workspace_dir.join(DB_FILE);
    fs::create_dir_all(&workspace_dir).unwrap();
    Connection::open(&database_path).unwrap().execute("CREATE TABLE unrelated (id INTEGER PRIMARY KEY)", []).unwrap();
    let before = fs::read(&database_path).unwrap();
    let state = serde_json::json!({
      "workspace_dir": workspace_dir.to_string_lossy()
    });

    let error = current_workspace_shell_paths_for_base(&state, &app_data_dir, None).unwrap_err();

    assert_eq!(error.code, "SOMA_WORKSPACE_NOT_FOUND");
    assert_eq!(fs::read(&database_path).unwrap(), before);
    assert!(!workspace_dir.join(RAW_IMPORT_DIR).exists());
    assert!(!workspace_dir.join(JOB_DIR).exists());
    assert!(!workspace_dir.join("exports").exists());
    let _ = fs::remove_dir_all(root);
  }

  #[test]
  fn shell_lookup_rejects_a_newer_schema_without_mutating_it() {
    let root = std::env::temp_dir().join(format!("soma-workspace-shell-newer-test-{}", uuid::Uuid::new_v4()));
    let app_data_dir = root.join("app-data");
    let paths = create_workspace_dir(root.join("workspace")).unwrap();
    let conn = Connection::open(&paths.database_path).unwrap();
    conn.pragma_update(None, "user_version", 10_000).unwrap();
    drop(conn);
    let before = fs::read(&paths.database_path).unwrap();
    let state = serde_json::json!({
      "workspace_dir": paths.workspace_dir.to_string_lossy()
    });

    let error = current_workspace_shell_paths_for_base(&state, &app_data_dir, None).unwrap_err();

    assert_eq!(error.code, "SOMA_UNSUPPORTED_SCHEMA");
    assert_eq!(fs::read(&paths.database_path).unwrap(), before);
    let _ = fs::remove_dir_all(root);
  }

  #[test]
  fn places_auto_workspaces_under_managed_app_data_dir() {
    let app_data_dir = PathBuf::from("C:/Users/test/AppData/Roaming/soma");

    assert_eq!(
      managed_workspace_dir_for_base(&app_data_dir, "workspace-test"),
      app_data_dir.join(MANAGED_WORKSPACES_DIR).join("workspace-test")
    );
  }

  #[test]
  fn separate_workspaces_have_separate_graph_state() {
    let root = std::env::temp_dir().join(format!("soma-workspace-isolation-test-{}", uuid::Uuid::new_v4()));
    let first = create_workspace_dir(root.join("first")).unwrap();
    let second = create_workspace_dir(root.join("second")).unwrap();
    let created_at = "2026-01-01T00:00:00Z";

    let first_conn = open_database(&first.database_path).unwrap();
    first_conn
      .execute(
        concat!(
          "INSERT INTO graph_thread_messages (id, role, content, created_at) ",
          "VALUES ('message_1', 'user', 'First graph only.', ?1)"
        ),
        [created_at],
      )
      .unwrap();
    first_conn
      .execute(
        concat!(
          "INSERT INTO graph_nodes ",
          "(id, node_type, title, status, authored_by_user, created_at, updated_at) ",
          "VALUES ('node_1', 'concept', 'First Graph', 'active', 0, ?1, ?1)"
        ),
        [created_at],
      )
      .unwrap();
    drop(first_conn);

    let second_conn = open_database(&second.database_path).unwrap();
    assert_eq!(count_table(&second_conn, "graph_thread_messages"), 0);
    assert_eq!(count_table(&second_conn, "graph_nodes"), 0);
    let _ = fs::remove_dir_all(root);
  }

  #[test]
  fn existing_workspace_requires_workspace_database() {
    let root = std::env::temp_dir().join(format!("soma-workspace-open-test-{}", uuid::Uuid::new_v4()));
    assert!(!is_existing_workspace_dir(&root));

    create_workspace_dir(&root).unwrap();

    assert!(is_existing_workspace_dir(&root));
    validate_existing_workspace_dir(&root).unwrap();
    let _ = fs::remove_dir_all(root);
  }

  #[test]
  fn workspace_validation_does_not_initialize_an_unrelated_sqlite_database() {
    let root = std::env::temp_dir().join(format!("soma-workspace-identity-test-{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&root).unwrap();
    let database_path = root.join(DB_FILE);
    Connection::open(&database_path).unwrap().execute("CREATE TABLE unrelated (id INTEGER)", []).unwrap();
    let before = fs::read(&database_path).unwrap();

    let error = validate_existing_workspace_dir(&root).unwrap_err();

    assert_eq!(error.code, "SOMA_WORKSPACE_NOT_FOUND");
    assert_eq!(fs::read(&database_path).unwrap(), before);
    assert!(!root.join(RAW_IMPORT_DIR).exists());
    assert!(!root.join(JOB_DIR).exists());
    assert!(!root.join("exports").exists());
    let _ = fs::remove_dir_all(root);
  }

  #[test]
  fn migrates_imported_old_dev_default_workspace_to_app_data_when_target_is_empty() {
    let root = std::env::temp_dir().join(format!("soma-workspace-migrate-test-{}", uuid::Uuid::new_v4()));
    let source = root.join("apps/desktop/src-tauri").join(DEFAULT_WORKSPACE_DIR);
    let target = root.join("app-data").join(DEFAULT_WORKSPACE_DIR);
    let source_paths = create_workspace_dir(&source).unwrap();
    fs::create_dir_all(&target).unwrap();
    fs::write(source.join(RAW_IMPORT_DIR).join("source_1-chat.txt"), "hello").unwrap();
    let imported_at = "2026-01-01T00:00:00Z";
    let conn = open_database(&source_paths.database_path).unwrap();
    conn
      .execute(
        INSERT_SOURCE_SQL,
        ("source_1", "text", "chat.txt", "D:/chat.txt", "raw/imports/source_1-chat.txt", imported_at),
      )
      .unwrap();
    drop(conn);

    assert!(!target.join(DB_FILE).exists());
    migrate_dev_default_workspace(&source, &target).unwrap();

    assert_eq!(database_source_count(&target.join(DB_FILE)).unwrap(), 1);
    assert!(target.join(RAW_IMPORT_DIR).join("source_1-chat.txt").exists());
    let _ = fs::remove_dir_all(root);
  }

  #[test]
  fn preserves_existing_zero_source_workspace_during_legacy_migration() {
    let root = std::env::temp_dir().join(format!("soma-workspace-migrate-preserve-test-{}", uuid::Uuid::new_v4()));
    let source = root.join("apps/desktop/src-tauri").join(DEFAULT_WORKSPACE_DIR);
    let target = root.join("app-data").join(DEFAULT_WORKSPACE_DIR);
    let source_paths = create_workspace_dir(&source).unwrap();
    let target_paths = create_workspace_dir(&target).unwrap();
    let imported_at = "2026-01-01T00:00:00Z";
    open_database(&source_paths.database_path)
      .unwrap()
      .execute(
        INSERT_SOURCE_SQL,
        ("source_1", "text", "chat.txt", "D:/chat.txt", "raw/imports/source_1-chat.txt", imported_at),
      )
      .unwrap();
    open_database(&target_paths.database_path)
      .unwrap()
      .execute(
        concat!(
          "INSERT INTO graph_thread_messages (id, role, content, created_at) ",
          "VALUES ('message_1', 'user', 'Keep this graph.', ?1)"
        ),
        [imported_at],
      )
      .unwrap();

    migrate_dev_default_workspace(&source, &target).unwrap();

    let target_conn = open_database(&target_paths.database_path).unwrap();
    assert_eq!(database_source_count(&target_paths.database_path).unwrap(), 0);
    assert_eq!(count_table(&target_conn, "graph_thread_messages"), 1);
    assert!(!target.join(RAW_IMPORT_DIR).join("source_1-chat.txt").exists());
    let _ = fs::remove_dir_all(root);
  }

  #[test]
  fn failed_legacy_copy_leaves_source_intact_and_no_partial_target() {
    let root = std::env::temp_dir().join(format!("soma-workspace-migrate-failure-test-{}", uuid::Uuid::new_v4()));
    let source = root.join("apps/desktop/src-tauri").join(DEFAULT_WORKSPACE_DIR);
    let target = root.join("app-data").join(DEFAULT_WORKSPACE_DIR);
    let source_paths = create_workspace_dir(&source).unwrap();
    let source_file = source.join(RAW_IMPORT_DIR).join("source_1-chat.txt");
    fs::write(&source_file, "source remains intact").unwrap();
    open_database(&source_paths.database_path)
      .unwrap()
      .execute(
        INSERT_SOURCE_SQL,
        ("source_1", "text", "chat.txt", "D:/chat.txt", "raw/imports/source_1-chat.txt", "2026-01-01T00:00:00Z"),
      )
      .unwrap();

    let result = migrate_dev_default_workspace_with(&source, &target, |_, staging| {
      fs::write(staging.join("partial-file"), "partial")?;
      Err(CommandError::storage("injected copy failure"))
    });

    assert!(result.is_err());
    assert_eq!(fs::read_to_string(source_file).unwrap(), "source remains intact");
    assert_eq!(database_source_count(&source_paths.database_path).unwrap(), 1);
    assert!(!target.exists());
    assert_eq!(fs::read_dir(target.parent().unwrap()).unwrap().count(), 0);
    let _ = fs::remove_dir_all(root);
  }

  #[test]
  fn unreadable_legacy_database_fails_closed_without_creating_a_target() {
    let root = std::env::temp_dir().join(format!("soma-workspace-migrate-corrupt-test-{}", uuid::Uuid::new_v4()));
    let source = root.join("apps/desktop/src-tauri").join(DEFAULT_WORKSPACE_DIR);
    let target = root.join("app-data").join(DEFAULT_WORKSPACE_DIR);
    fs::create_dir_all(&source).unwrap();
    let database_path = source.join(DB_FILE);
    let original = b"not a sqlite database";
    fs::write(&database_path, original).unwrap();

    let error = migrate_dev_default_workspace(&source, &target).unwrap_err();

    assert_eq!(error.code, "Soma_STORAGE_ERROR");
    assert_eq!(fs::read(&database_path).unwrap(), original);
    assert!(!target.exists());
    let _ = fs::remove_dir_all(root);
  }

  fn count_table(conn: &Connection, table: &str) -> i64 {
    conn.query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| row.get(0)).unwrap()
  }
}
