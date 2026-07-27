use std::path::Path;
use std::sync::{Mutex, MutexGuard, OnceLock};
use std::time::Duration;

use rusqlite::{Connection, OpenFlags, OptionalExtension};

use crate::error::{CommandError, CommandResult};

type SchemaMigration = (i64, fn(&Connection) -> CommandResult<()>);

#[derive(Clone, Copy)]
struct FtsRepair {
  chunks: bool,
  node_bodies: bool,
}

const SQLITE_BUSY_TIMEOUT_MS: u64 = 5_000;
const LATEST_SCHEMA_VERSION: i64 = 2;
const SCHEMA_MIGRATIONS: &[SchemaMigration] = &[(1, apply_schema_v1), (2, apply_schema_v2)];
const REQUIRED_SCHEMA_TABLES: &[&str] = &[
  "sources",
  "conversations",
  "messages",
  "chunks",
  "chunks_fts",
  "graph_patches",
  "graph_patch_undo",
  "graph_proposals",
  "graph_nodes",
  "node_body_versions",
  "node_body_versions_fts",
  "graph_edges",
  "graph_evidence",
  "graph_node_layout",
  "graph_thread_messages",
  "node_thread_messages",
  "workspace_settings",
  "graph_message_evidence",
  "node_message_evidence",
];
const REQUIRED_SCHEMA_INDEXES: &[&str] = &[
  "idx_graph_evidence_entity",
  "idx_graph_nodes_startup",
  "idx_graph_edges_startup",
  "idx_graph_edges_target_lookup",
  "idx_graph_message_evidence_target",
  "idx_node_message_evidence_target",
];
const REQUIRED_SCHEMA_TRIGGERS: &[&str] = &["node_body_versions_ai", "node_body_versions_ad", "node_body_versions_au"];
const REQUIRED_SCHEMA_COLUMNS: &[(&str, &str)] = &[("graph_patches", "source_message_id")];

static SQLITE_WRITE_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

pub(crate) fn open_database(database_path: impl AsRef<Path>) -> CommandResult<Connection> {
  let database_path = database_path.as_ref();
  let conn = Connection::open(database_path)?;
  configure_initialized_database_connection(&conn)?;
  if schema_needs_initialization(&conn)? {
    with_write_transaction(&conn, initialize_schema)?;
  }
  Ok(conn)
}

pub(crate) fn open_existing_database(database_path: impl AsRef<Path>) -> CommandResult<Connection> {
  let database_path = database_path.as_ref();
  let conn = Connection::open_with_flags(database_path, OpenFlags::SQLITE_OPEN_READ_WRITE)?;
  configure_initialized_database_connection(&conn)?;
  if schema_needs_initialization(&conn)? {
    with_write_transaction(&conn, initialize_schema)?;
  }
  Ok(conn)
}

pub(crate) fn open_existing_database_readonly(database_path: impl AsRef<Path>) -> CommandResult<Connection> {
  let database_path = database_path.as_ref();
  let conn = Connection::open_with_flags(database_path, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
  configure_readonly_database_connection(&conn)?;
  Ok(conn)
}

pub(crate) fn validate_existing_soma_database(database_path: impl AsRef<Path>) -> CommandResult<()> {
  let invalid_workspace =
    || CommandError::new("SOMA_WORKSPACE_NOT_FOUND", "Selected folder is not a supported Soma workspace.");
  let conn = open_existing_database_readonly(database_path).map_err(|_| invalid_workspace())?;
  let version = schema_version(&conn).map_err(|_| invalid_workspace())?;
  if version > LATEST_SCHEMA_VERSION {
    return Err(CommandError::new(
      "SOMA_UNSUPPORTED_SCHEMA",
      format!("Workspace schema version {version} is newer than this Soma build supports ({LATEST_SCHEMA_VERSION})."),
    ));
  }
  if version < 1
    || !database_object_exists(&conn, "table", "sources")?
    || !database_object_exists(&conn, "table", "graph_nodes")?
  {
    return Err(invalid_workspace());
  }
  Ok(())
}

pub(crate) fn with_write_transaction<T>(
  conn: &Connection,
  action: impl FnOnce(&Connection) -> CommandResult<T>,
) -> CommandResult<T> {
  let _guard = sqlite_write_guard()?;
  conn.execute_batch("BEGIN IMMEDIATE")?;
  let result = action(conn);
  match result {
    Ok(value) => match conn.execute_batch("COMMIT") {
      Ok(()) => Ok(value),
      Err(error) => {
        let _ = conn.execute_batch("ROLLBACK");
        Err(error.into())
      }
    },
    Err(error) => {
      let _ = conn.execute_batch("ROLLBACK");
      Err(error)
    }
  }
}

fn configure_existing_database_connection(conn: &Connection) -> CommandResult<()> {
  conn.busy_timeout(Duration::from_millis(SQLITE_BUSY_TIMEOUT_MS))?;
  conn.execute_batch(
    r#"
        PRAGMA synchronous = NORMAL;
        PRAGMA foreign_keys = ON;
        "#,
  )?;
  Ok(())
}

fn configure_readonly_database_connection(conn: &Connection) -> CommandResult<()> {
  conn.busy_timeout(Duration::from_millis(SQLITE_BUSY_TIMEOUT_MS))?;
  conn.execute_batch(
    r#"
        PRAGMA foreign_keys = ON;
        PRAGMA query_only = ON;
        "#,
  )?;
  Ok(())
}

fn configure_initialized_database_connection(conn: &Connection) -> CommandResult<()> {
  configure_existing_database_connection(conn)?;
  ensure_wal_journal_mode(conn)?;
  Ok(())
}

fn ensure_wal_journal_mode(conn: &Connection) -> CommandResult<()> {
  let journal_mode: String = conn.query_row("PRAGMA journal_mode", [], |row| row.get(0))?;
  if journal_mode.eq_ignore_ascii_case("wal") {
    return Ok(());
  }

  let _guard = sqlite_write_guard()?;
  let journal_mode: String = conn.query_row("PRAGMA journal_mode", [], |row| row.get(0))?;
  if !journal_mode.eq_ignore_ascii_case("wal") {
    conn.execute_batch("PRAGMA journal_mode = WAL;")?;
  }
  Ok(())
}

fn sqlite_write_guard() -> CommandResult<MutexGuard<'static, ()>> {
  SQLITE_WRITE_LOCK
    .get_or_init(|| Mutex::new(()))
    .lock()
    .map_err(|_| CommandError::storage("SQLite write lock was poisoned."))
}

fn schema_needs_initialization(conn: &Connection) -> CommandResult<bool> {
  let version = schema_version(conn)?;
  if version > LATEST_SCHEMA_VERSION {
    return Err(CommandError::storage(format!(
      "Workspace schema version {version} is newer than this Soma build supports ({LATEST_SCHEMA_VERSION})."
    )));
  }
  Ok(version < LATEST_SCHEMA_VERSION || schema_objects_missing(conn)?)
}

fn schema_version(conn: &Connection) -> CommandResult<i64> {
  Ok(conn.pragma_query_value(None, "user_version", |row| row.get(0))?)
}

fn schema_objects_missing(conn: &Connection) -> CommandResult<bool> {
  for (object_type, names) in
    [("table", REQUIRED_SCHEMA_TABLES), ("index", REQUIRED_SCHEMA_INDEXES), ("trigger", REQUIRED_SCHEMA_TRIGGERS)]
  {
    for name in names {
      if !database_object_exists(conn, object_type, name)? {
        return Ok(true);
      }
    }
  }
  for &(table_name, column_name) in REQUIRED_SCHEMA_COLUMNS {
    if !table_has_column(conn, table_name, column_name)? {
      return Ok(true);
    }
  }
  Ok(false)
}

fn required_fts_repair(conn: &Connection) -> CommandResult<FtsRepair> {
  let chunks = !database_object_exists(conn, "table", "chunks_fts")?;
  let mut node_bodies = !database_object_exists(conn, "table", "node_body_versions_fts")?;
  for trigger in REQUIRED_SCHEMA_TRIGGERS {
    node_bodies |= !database_object_exists(conn, "trigger", trigger)?;
  }
  Ok(FtsRepair { chunks, node_bodies })
}

fn database_object_exists(conn: &Connection, object_type: &str, name: &str) -> CommandResult<bool> {
  Ok(
    conn
      .query_row("SELECT 1 FROM sqlite_master WHERE type = ?1 AND name = ?2 LIMIT 1", [object_type, name], |_| Ok(()))
      .optional()?
      .is_some(),
  )
}

fn table_has_column(conn: &Connection, table_name: &str, column_name: &str) -> CommandResult<bool> {
  Ok(conn.query_row(
    "SELECT EXISTS(SELECT 1 FROM pragma_table_info(?1) WHERE name = ?2)",
    [table_name, column_name],
    |row| row.get(0),
  )?)
}

fn initialize_schema(conn: &Connection) -> CommandResult<()> {
  let current_version = schema_version(conn)?;
  if current_version > LATEST_SCHEMA_VERSION {
    return Err(CommandError::storage(format!(
      "Workspace schema version {current_version} is newer than this Soma build supports ({LATEST_SCHEMA_VERSION})."
    )));
  }
  let fts_repair = required_fts_repair(conn)?;

  for &(version, migrate) in SCHEMA_MIGRATIONS {
    if version <= current_version {
      continue;
    }
    migrate(conn)?;
    conn.pragma_update(None, "user_version", version)?;
  }

  if schema_objects_missing(conn)? {
    apply_schema_v1(conn)?;
    apply_schema_v2(conn)?;
  }
  rebuild_repaired_fts_content(conn, fts_repair)?;
  Ok(())
}

fn rebuild_repaired_fts_content(conn: &Connection, repair: FtsRepair) -> CommandResult<()> {
  if repair.chunks {
    conn.execute_batch(
      r#"
      DELETE FROM chunks_fts;
      INSERT INTO chunks_fts(rowid, content, chunk_id)
      SELECT rowid, content, id
      FROM chunks;
      "#,
    )?;
  }
  if repair.node_bodies {
    conn.execute_batch(
      r#"
      DELETE FROM node_body_versions_fts;
      INSERT INTO node_body_versions_fts(rowid, compiled_body, body_version_id, node_id)
      SELECT rowid, compiled_body, id, node_id
      FROM node_body_versions;
      "#,
    )?;
  }
  Ok(())
}

fn apply_schema_v1(conn: &Connection) -> CommandResult<()> {
  conn.execute_batch(
    r#"
    CREATE TABLE IF NOT EXISTS sources (
      id TEXT PRIMARY KEY,
      source_type TEXT NOT NULL,
      title TEXT NOT NULL,
      original_path TEXT NOT NULL,
      raw_path TEXT NOT NULL,
      imported_at TEXT NOT NULL
    );

    CREATE TABLE IF NOT EXISTS conversations (
      id TEXT PRIMARY KEY,
      source_id TEXT NOT NULL,
      provider TEXT NOT NULL DEFAULT 'manual',
      title TEXT NOT NULL,
      created_at TEXT,
      FOREIGN KEY (source_id) REFERENCES sources(id)
    );

    CREATE TABLE IF NOT EXISTS messages (
      id TEXT PRIMARY KEY,
      conversation_id TEXT NOT NULL,
      role TEXT NOT NULL CHECK (role IN ('user', 'assistant', 'system', 'tool')),
      content TEXT NOT NULL,
      order_index INTEGER NOT NULL,
      created_at TEXT,
      FOREIGN KEY (conversation_id) REFERENCES conversations(id)
    );

    CREATE TABLE IF NOT EXISTS chunks (
      id TEXT PRIMARY KEY,
      message_id TEXT NOT NULL,
      content TEXT NOT NULL,
      chunk_index INTEGER NOT NULL,
      token_count INTEGER NOT NULL,
      FOREIGN KEY (message_id) REFERENCES messages(id)
    );

    CREATE VIRTUAL TABLE IF NOT EXISTS chunks_fts USING fts5(
      content,
      chunk_id UNINDEXED
    );

    CREATE TABLE IF NOT EXISTS graph_patches (
      id TEXT PRIMARY KEY,
      job_id TEXT,
      source TEXT NOT NULL,
      status TEXT NOT NULL CHECK (status IN ('imported', 'rejected')),
      created_at TEXT NOT NULL,
      errors_json TEXT NOT NULL DEFAULT '[]'
    );

    CREATE TABLE IF NOT EXISTS graph_proposals (
      id TEXT PRIMARY KEY,
      patch_id TEXT NOT NULL,
      proposal_type TEXT NOT NULL CHECK (proposal_type IN (
        'node',
        'edge',
        'node_body_update',
        'edge_bridge_update',
        'message_evidence_attachment',
        'path',
        'ambiguity',
        'merge_candidate',
        'warning'
      )),
      status TEXT NOT NULL CHECK (status IN ('draft', 'proposed', 'accepted', 'rejected', 'deferred', 'superseded')),
      temp_id TEXT,
      payload_json TEXT NOT NULL,
      accepted_entity_type TEXT,
      accepted_entity_id TEXT,
      created_at TEXT NOT NULL,
      decided_at TEXT,
      decision_reason TEXT,
      FOREIGN KEY (patch_id) REFERENCES graph_patches(id)
    );

    CREATE TABLE IF NOT EXISTS graph_nodes (
      id TEXT PRIMARY KEY,
      node_type TEXT NOT NULL,
      title TEXT NOT NULL,
      preview TEXT,
      current_body_version_id TEXT,
      status TEXT NOT NULL CHECK (status IN ('active', 'hidden', 'archived')) DEFAULT 'active',
      authored_by_user INTEGER NOT NULL DEFAULT 0,
      created_at TEXT NOT NULL,
      updated_at TEXT NOT NULL
    );

    CREATE TABLE IF NOT EXISTS node_body_versions (
      id TEXT PRIMARY KEY,
      node_id TEXT NOT NULL,
      version_number INTEGER NOT NULL,
      compiled_body TEXT NOT NULL,
      authored_by_user INTEGER NOT NULL DEFAULT 0,
      created_at TEXT NOT NULL,
      FOREIGN KEY (node_id) REFERENCES graph_nodes(id),
      UNIQUE (node_id, version_number)
    );

    CREATE VIRTUAL TABLE IF NOT EXISTS node_body_versions_fts USING fts5(
      compiled_body,
      body_version_id UNINDEXED,
      node_id UNINDEXED
    );

    CREATE TRIGGER IF NOT EXISTS node_body_versions_ai
      AFTER INSERT ON node_body_versions
      BEGIN
        INSERT INTO node_body_versions_fts(rowid, compiled_body, body_version_id, node_id)
        VALUES (new.rowid, new.compiled_body, new.id, new.node_id);
      END;

    CREATE TRIGGER IF NOT EXISTS node_body_versions_ad
      AFTER DELETE ON node_body_versions
      BEGIN
        DELETE FROM node_body_versions_fts WHERE rowid = old.rowid;
      END;

    CREATE TRIGGER IF NOT EXISTS node_body_versions_au
      AFTER UPDATE OF compiled_body ON node_body_versions
      BEGIN
        DELETE FROM node_body_versions_fts WHERE rowid = old.rowid;
        INSERT INTO node_body_versions_fts(rowid, compiled_body, body_version_id, node_id)
        VALUES (new.rowid, new.compiled_body, new.id, new.node_id);
      END;

    CREATE TABLE IF NOT EXISTS graph_edges (
      id TEXT PRIMARY KEY,
      source_node_id TEXT NOT NULL,
      target_node_id TEXT NOT NULL,
      edge_type TEXT NOT NULL,
      bridge_text TEXT,
      status TEXT NOT NULL CHECK (status IN ('active', 'hidden', 'archived')) DEFAULT 'active',
      authored_by_user INTEGER NOT NULL DEFAULT 0,
      created_at TEXT NOT NULL,
      updated_at TEXT NOT NULL,
      FOREIGN KEY (source_node_id) REFERENCES graph_nodes(id),
      FOREIGN KEY (target_node_id) REFERENCES graph_nodes(id)
    );

    CREATE TABLE IF NOT EXISTS graph_evidence (
      id TEXT PRIMARY KEY,
      entity_type TEXT NOT NULL CHECK (entity_type IN ('node', 'edge', 'node_body_version')),
      entity_id TEXT NOT NULL,
      chunk_id TEXT NOT NULL,
      message_id TEXT,
      quote_excerpt TEXT,
      created_at TEXT NOT NULL,
      FOREIGN KEY (chunk_id) REFERENCES chunks(id)
    );

    CREATE TABLE IF NOT EXISTS graph_node_layout (
      node_id TEXT PRIMARY KEY,
      x REAL NOT NULL,
      y REAL NOT NULL,
      pinned INTEGER NOT NULL DEFAULT 0,
      updated_at TEXT NOT NULL,
      FOREIGN KEY (node_id) REFERENCES graph_nodes(id)
    );

    CREATE TABLE IF NOT EXISTS graph_thread_messages (
      id TEXT PRIMARY KEY,
      role TEXT NOT NULL CHECK (role IN ('user', 'assistant', 'system')),
      content TEXT NOT NULL,
      context_json TEXT,
      created_at TEXT NOT NULL
    );

    CREATE TABLE IF NOT EXISTS node_thread_messages (
      id TEXT PRIMARY KEY,
      node_id TEXT NOT NULL,
      role TEXT NOT NULL CHECK (role IN ('user', 'assistant', 'system')),
      content TEXT NOT NULL,
      context_json TEXT,
      created_at TEXT NOT NULL,
      FOREIGN KEY (node_id) REFERENCES graph_nodes(id)
    );

    CREATE TABLE IF NOT EXISTS workspace_settings (
      key TEXT PRIMARY KEY,
      value_json TEXT NOT NULL,
      updated_at TEXT NOT NULL
    );

    CREATE INDEX IF NOT EXISTS idx_graph_evidence_entity
      ON graph_evidence(entity_type, entity_id);

    CREATE INDEX IF NOT EXISTS idx_graph_nodes_startup
      ON graph_nodes(status, title, id);

    CREATE INDEX IF NOT EXISTS idx_graph_edges_startup
      ON graph_edges(status, source_node_id, target_node_id, id);

    CREATE INDEX IF NOT EXISTS idx_graph_edges_target_lookup
      ON graph_edges(status, target_node_id, source_node_id, id);

    INSERT INTO node_body_versions_fts(rowid, compiled_body, body_version_id, node_id)
    SELECT node_body_versions.rowid, node_body_versions.compiled_body, node_body_versions.id, node_body_versions.node_id
    FROM node_body_versions
    WHERE NOT EXISTS (
      SELECT 1
      FROM node_body_versions_fts
      WHERE node_body_versions_fts.rowid = node_body_versions.rowid
    );
    "#,
  )?;
  Ok(())
}

fn apply_schema_v2(conn: &Connection) -> CommandResult<()> {
  if !table_has_column(conn, "graph_patches", "source_message_id")? {
    conn.execute_batch("ALTER TABLE graph_patches ADD COLUMN source_message_id TEXT;")?;
  }
  conn.execute_batch(
    r#"
    CREATE TABLE IF NOT EXISTS graph_patch_undo (
      patch_id TEXT PRIMARY KEY,
      status TEXT NOT NULL CHECK (status IN ('ready', 'undone')),
      changes_json TEXT NOT NULL,
      created_at TEXT NOT NULL,
      undone_at TEXT,
      FOREIGN KEY (patch_id) REFERENCES graph_patches(id)
    );

    CREATE TABLE IF NOT EXISTS graph_message_evidence (
      id TEXT PRIMARY KEY,
      target_entity_type TEXT NOT NULL CHECK (target_entity_type IN ('node', 'edge', 'node_body_version')),
      target_entity_id TEXT NOT NULL,
      graph_thread_message_id TEXT NOT NULL,
      quote_excerpt TEXT,
      created_at TEXT NOT NULL,
      FOREIGN KEY (graph_thread_message_id) REFERENCES graph_thread_messages(id)
    );

    CREATE TABLE IF NOT EXISTS node_message_evidence (
      id TEXT PRIMARY KEY,
      target_entity_type TEXT NOT NULL CHECK (target_entity_type IN ('node', 'edge', 'node_body_version')),
      target_entity_id TEXT NOT NULL,
      node_thread_message_id TEXT NOT NULL,
      quote_excerpt TEXT,
      created_at TEXT NOT NULL,
      FOREIGN KEY (node_thread_message_id) REFERENCES node_thread_messages(id)
    );

    CREATE INDEX IF NOT EXISTS idx_graph_message_evidence_target
      ON graph_message_evidence(target_entity_type, target_entity_id);

    CREATE INDEX IF NOT EXISTS idx_node_message_evidence_target
      ON node_message_evidence(target_entity_type, target_entity_id);
    "#,
  )?;
  Ok(())
}

#[cfg(test)]
mod tests {
  use std::fs;

  use super::*;

  const DB_FILE: &str = "soma.sqlite";

  #[test]
  fn schema_indexes_evidence_lookup_columns() {
    let conn = Connection::open_in_memory().unwrap();
    initialize_schema(&conn).unwrap();

    assert!(index_exists(&conn, "idx_graph_evidence_entity"));
    assert!(index_exists(&conn, "idx_graph_nodes_startup"));
    assert!(index_exists(&conn, "idx_graph_edges_startup"));
    assert!(index_exists(&conn, "idx_graph_edges_target_lookup"));
    assert!(index_exists(&conn, "idx_graph_message_evidence_target"));
    assert!(index_exists(&conn, "idx_node_message_evidence_target"));
    assert!(table_exists(&conn, "node_body_versions_fts"));
  }

  #[test]
  fn node_body_versions_fts_tracks_body_version_inserts() {
    let conn = Connection::open_in_memory().unwrap();
    initialize_schema(&conn).unwrap();

    conn
      .execute(
        concat!(
          "INSERT INTO graph_nodes ",
          "(id, node_type, title, current_body_version_id, status, authored_by_user, created_at, updated_at) ",
          "VALUES ('node_a', 'concept', 'Node A', 'body_a', 'active', 0, ",
          "'2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z')"
        ),
        [],
      )
      .unwrap();
    conn
      .execute(
        concat!(
          "INSERT INTO node_body_versions ",
          "(id, node_id, version_number, compiled_body, authored_by_user, created_at) ",
          "VALUES ('body_a', 'node_a', 1, 'semantic retrieval sentinel', 0, '2026-01-01T00:00:00Z')"
        ),
        [],
      )
      .unwrap();

    let body_id: String = conn
      .query_row(
        "SELECT body_version_id FROM node_body_versions_fts WHERE node_body_versions_fts MATCH 'sentinel'",
        [],
        |row| row.get(0),
      )
      .unwrap();
    assert_eq!(body_id, "body_a");
  }

  #[test]
  fn open_existing_database_rebuilds_node_body_fts_after_table_repair() {
    let root = std::env::temp_dir().join(format!("soma-body-fts-backfill-test-{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&root).unwrap();
    let db_path = root.join(DB_FILE);
    {
      let conn = Connection::open(&db_path).unwrap();
      initialize_schema(&conn).unwrap();
      conn
        .execute_batch(
          r#"
        INSERT INTO graph_nodes (
          id, node_type, title, current_body_version_id, status, authored_by_user, created_at, updated_at
        )
        VALUES (
          'node_old', 'concept', 'Old Node', 'body_old', 'active', 0,
          '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z'
        );
        INSERT INTO node_body_versions (
          id, node_id, version_number, compiled_body, authored_by_user, created_at
        )
        VALUES (
          'body_old', 'node_old', 1, 'legacy body backfill sentinel', 0, '2026-01-01T00:00:00Z'
        );
        DROP TRIGGER node_body_versions_ai;
        DROP TRIGGER node_body_versions_ad;
        DROP TRIGGER node_body_versions_au;
        DROP TABLE node_body_versions_fts;
        "#,
        )
        .unwrap();
    }

    let conn = open_existing_database(&db_path).unwrap();
    let body_id: String = conn
      .query_row(
        "SELECT body_version_id FROM node_body_versions_fts WHERE node_body_versions_fts MATCH 'backfill'",
        [],
        |row| row.get(0),
      )
      .unwrap();

    assert_eq!(body_id, "body_old");
    assert_node_body_fts_matches_canonical(&conn);
    let _ = fs::remove_dir_all(root);
  }

  #[test]
  fn schema_uses_startup_indexes_for_canvas_queries() {
    let conn = Connection::open_in_memory().unwrap();
    initialize_schema(&conn).unwrap();

    let node_plan = query_plan(
      &conn,
      r#"
            SELECT graph_nodes.id
            FROM graph_nodes
            JOIN node_body_versions ON graph_nodes.current_body_version_id = node_body_versions.id
            WHERE graph_nodes.status = 'active'
            ORDER BY graph_nodes.title, graph_nodes.id
            LIMIT 160
            "#,
    );
    let edge_plan = query_plan(
      &conn,
      r#"
            SELECT id
            FROM graph_edges
            WHERE status = 'active'
              AND source_node_id IN ('node_a', 'node_b')
              AND target_node_id IN ('node_a', 'node_b')
            ORDER BY source_node_id, target_node_id, id
            LIMIT 320
            "#,
    );

    assert!(node_plan.contains("idx_graph_nodes_startup"), "{node_plan}");
    assert!(edge_plan.contains("idx_graph_edges_startup"), "{edge_plan}");
  }

  #[test]
  fn schema_uses_directional_indexes_for_node_context_queries() {
    let conn = Connection::open_in_memory().unwrap();
    initialize_schema(&conn).unwrap();

    let source_plan = query_plan(
      &conn,
      r#"
            SELECT target_node_id
            FROM graph_edges
            WHERE status = 'active' AND source_node_id = 'focus'
            ORDER BY id
            LIMIT 16
            "#,
    );
    let target_plan = query_plan(
      &conn,
      r#"
            SELECT source_node_id
            FROM graph_edges
            WHERE status = 'active' AND target_node_id = 'focus'
            ORDER BY id
            LIMIT 16
            "#,
    );

    assert!(source_plan.contains("idx_graph_edges_startup"), "{source_plan}");
    assert!(target_plan.contains("idx_graph_edges_target_lookup"), "{target_plan}");
  }

  #[test]
  fn open_database_configures_wal_and_busy_timeout() {
    let root = std::env::temp_dir().join(format!("soma-db-config-test-{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&root).unwrap();
    let db_path = root.join(DB_FILE);

    let conn = open_database(&db_path).unwrap();
    let journal_mode: String = conn.query_row("PRAGMA journal_mode", [], |row| row.get(0)).unwrap();
    let busy_timeout: i64 = conn.query_row("PRAGMA busy_timeout", [], |row| row.get(0)).unwrap();

    assert_eq!(journal_mode.to_ascii_lowercase(), "wal");
    assert_eq!(busy_timeout, SQLITE_BUSY_TIMEOUT_MS as i64);
    let _ = fs::remove_dir_all(root);
  }

  #[test]
  fn open_existing_database_enables_wal_and_runs_missing_schema_migrations() {
    let root = std::env::temp_dir().join(format!("soma-existing-db-config-test-{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&root).unwrap();
    let db_path = root.join(DB_FILE);
    drop(Connection::open(&db_path).unwrap());

    let conn = open_existing_database(&db_path).unwrap();
    let journal_mode: String = conn.query_row("PRAGMA journal_mode", [], |row| row.get(0)).unwrap();
    let busy_timeout: i64 = conn.query_row("PRAGMA busy_timeout", [], |row| row.get(0)).unwrap();
    let version = schema_version(&conn).unwrap();
    let has_sources_table = conn
      .query_row("SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'sources' LIMIT 1", [], |_| Ok(()))
      .optional()
      .unwrap()
      .is_some();

    assert_eq!(journal_mode.to_ascii_lowercase(), "wal");
    assert_eq!(busy_timeout, SQLITE_BUSY_TIMEOUT_MS as i64);
    assert_eq!(version, LATEST_SCHEMA_VERSION);
    assert!(has_sources_table);
    let _ = fs::remove_dir_all(root);
  }

  #[test]
  fn open_existing_database_migrates_version_one_data_to_latest_schema() {
    let root = std::env::temp_dir().join(format!("soma-v1-migration-test-{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&root).unwrap();
    let db_path = root.join(DB_FILE);
    {
      let conn = Connection::open(&db_path).unwrap();
      apply_schema_v1(&conn).unwrap();
      conn.pragma_update(None, "user_version", 1).unwrap();
      conn
        .execute_batch(
          r#"
          INSERT INTO sources (id, source_type, title, original_path, raw_path, imported_at)
          VALUES ('source_v1', 'conversation', 'Existing workspace', 'old.json', 'raw/old.json', '2026-01-01');

          INSERT INTO graph_thread_messages (id, role, content, created_at)
          VALUES ('message_v1', 'user', 'Keep this message', '2026-01-01');

          INSERT INTO graph_patches (id, job_id, source, status, created_at, errors_json)
          VALUES ('patch_v1', NULL, 'chat', 'imported', '2026-01-01', '[]');
          "#,
        )
        .unwrap();

      assert_eq!(schema_version(&conn).unwrap(), 1);
      assert!(!table_has_column(&conn, "graph_patches", "source_message_id").unwrap());
      assert!(!table_exists(&conn, "graph_patch_undo"));
      assert!(!table_exists(&conn, "graph_message_evidence"));
    }

    validate_existing_soma_database(&db_path).unwrap();
    let conn = open_existing_database(&db_path).unwrap();
    let title: String =
      conn.query_row("SELECT title FROM sources WHERE id = 'source_v1'", [], |row| row.get(0)).unwrap();
    let message: String = conn
      .query_row("SELECT content FROM graph_thread_messages WHERE id = 'message_v1'", [], |row| row.get(0))
      .unwrap();

    assert_eq!(schema_version(&conn).unwrap(), LATEST_SCHEMA_VERSION);
    assert_eq!(title, "Existing workspace");
    assert_eq!(message, "Keep this message");
    assert!(table_has_column(&conn, "graph_patches", "source_message_id").unwrap());
    assert!(table_exists(&conn, "graph_patch_undo"));
    assert!(table_exists(&conn, "graph_message_evidence"));
    assert!(index_exists(&conn, "idx_graph_message_evidence_target"));
    conn.execute("UPDATE graph_patches SET source_message_id = 'message_v1' WHERE id = 'patch_v1'", []).unwrap();
    let source_message_id: String = conn
      .query_row("SELECT source_message_id FROM graph_patches WHERE id = 'patch_v1'", [], |row| row.get(0))
      .unwrap();
    assert_eq!(source_message_id, "message_v1");

    drop(conn);
    let _ = fs::remove_dir_all(root);
  }

  #[test]
  fn workspace_validation_rejects_unrelated_sqlite_without_mutating_it() {
    let root = std::env::temp_dir().join(format!("soma-db-identity-test-{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&root).unwrap();
    let db_path = root.join(DB_FILE);
    {
      let conn = Connection::open(&db_path).unwrap();
      conn.execute_batch("CREATE TABLE unrelated (id INTEGER PRIMARY KEY, value TEXT NOT NULL);").unwrap();
      conn.execute("INSERT INTO unrelated (value) VALUES ('keep')", []).unwrap();
    }
    let before = fs::read(&db_path).unwrap();

    let error = validate_existing_soma_database(&db_path).unwrap_err();

    assert_eq!(error.code, "SOMA_WORKSPACE_NOT_FOUND");
    assert_eq!(fs::read(&db_path).unwrap(), before);
    let _ = fs::remove_dir_all(root);
  }

  #[test]
  fn open_existing_database_rebuilds_chunks_fts_after_table_repair() {
    let root = std::env::temp_dir().join(format!("soma-chunks-fts-repair-test-{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&root).unwrap();
    let db_path = root.join(DB_FILE);
    {
      let conn = Connection::open(&db_path).unwrap();
      initialize_schema(&conn).unwrap();
      conn
        .execute_batch(
          r#"
          INSERT INTO sources (id, source_type, title, original_path, raw_path, imported_at)
          VALUES ('source_a', 'conversation', 'Source A', 'source.json', 'raw/source.json', '2026-01-01');

          INSERT INTO conversations (id, source_id, provider, title)
          VALUES ('conversation_a', 'source_a', 'manual', 'Conversation A');

          INSERT INTO messages (id, conversation_id, role, content, order_index)
          VALUES ('message_a', 'conversation_a', 'user', 'Message A', 0);

          INSERT INTO chunks (id, message_id, content, chunk_index, token_count)
          VALUES ('chunk_a', 'message_a', 'chunkrepair searchable sentinel', 0, 3);

          INSERT INTO chunks_fts (content, chunk_id)
          VALUES ('chunkrepair searchable sentinel', 'chunk_a');

          DROP TABLE chunks_fts;
          "#,
        )
        .unwrap();
    }

    let conn = open_existing_database(&db_path).unwrap();
    let chunk_id: String = conn
      .query_row("SELECT chunk_id FROM chunks_fts WHERE chunks_fts MATCH 'chunkrepair'", [], |row| row.get(0))
      .unwrap();

    assert_eq!(chunk_id, "chunk_a");
    assert_chunks_fts_matches_canonical(&conn);
    drop(conn);
    let _ = fs::remove_dir_all(root);
  }

  #[test]
  fn open_existing_database_repairs_missing_required_index() {
    assert_open_repairs_schema_object(
      "index",
      "idx_graph_edges_target_lookup",
      "DROP INDEX idx_graph_edges_target_lookup",
    );
  }

  #[test]
  fn open_existing_database_rebuilds_node_body_fts_after_each_trigger_repair() {
    for trigger in REQUIRED_SCHEMA_TRIGGERS {
      let root = std::env::temp_dir().join(format!("soma-body-fts-{trigger}-test-{}", uuid::Uuid::new_v4()));
      fs::create_dir_all(&root).unwrap();
      let db_path = root.join(DB_FILE);
      {
        let conn = Connection::open(&db_path).unwrap();
        initialize_schema(&conn).unwrap();
        conn
          .execute_batch(
            r#"
            INSERT INTO graph_nodes (
              id, node_type, title, current_body_version_id, status, authored_by_user, created_at, updated_at
            )
            VALUES (
              'node_a', 'concept', 'Node A', 'body_existing', 'active', 0,
              '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z'
            );

            INSERT INTO node_body_versions (
              id, node_id, version_number, compiled_body, authored_by_user, created_at
            )
            VALUES (
              'body_existing', 'node_a', 1, 'stale trigger sentinel', 0, '2026-01-01T00:00:00Z'
            );
            "#,
          )
          .unwrap();
        conn.execute_batch(&format!("DROP TRIGGER {trigger};")).unwrap();
        match *trigger {
          "node_body_versions_ai" => conn
            .execute(
              concat!(
                "INSERT INTO node_body_versions ",
                "(id, node_id, version_number, compiled_body, authored_by_user, created_at) ",
                "VALUES ('body_inserted', 'node_a', 2, 'insertrepair current sentinel', 0, ",
                "'2026-01-02T00:00:00Z')"
              ),
              [],
            )
            .unwrap(),
          "node_body_versions_au" => conn
            .execute(
              concat!(
                "UPDATE node_body_versions SET compiled_body = 'updaterepair current sentinel' ",
                "WHERE id = 'body_existing'"
              ),
              [],
            )
            .unwrap(),
          "node_body_versions_ad" => {
            conn.execute("DELETE FROM node_body_versions WHERE id = 'body_existing'", []).unwrap()
          }
          _ => unreachable!(),
        };
      }

      let conn = open_existing_database(&db_path).unwrap();

      assert_node_body_fts_matches_canonical(&conn);
      match *trigger {
        "node_body_versions_ai" => assert_eq!(node_body_fts_match_count(&conn, "insertrepair"), 1),
        "node_body_versions_au" => {
          assert_eq!(node_body_fts_match_count(&conn, "updaterepair"), 1);
          assert_eq!(node_body_fts_match_count(&conn, "stale"), 0);
        }
        "node_body_versions_ad" => assert_eq!(node_body_fts_match_count(&conn, "stale"), 0),
        _ => unreachable!(),
      }

      drop(conn);
      let _ = fs::remove_dir_all(root);
    }
  }

  #[test]
  fn open_existing_database_readonly_does_not_enable_wal_or_allow_writes() {
    let root = std::env::temp_dir().join(format!("soma-existing-db-readonly-test-{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&root).unwrap();
    let db_path = root.join(DB_FILE);
    {
      let conn = Connection::open(&db_path).unwrap();
      conn
        .execute_batch(
          r#"
                PRAGMA journal_mode = DELETE;
                CREATE TABLE items (id INTEGER PRIMARY KEY, value TEXT NOT NULL);
                INSERT INTO items (value) VALUES ('reader');
                "#,
        )
        .unwrap();
    }

    let conn = open_existing_database_readonly(&db_path).unwrap();
    let journal_mode: String = conn.query_row("PRAGMA journal_mode", [], |row| row.get(0)).unwrap();
    let query_only: i64 = conn.query_row("PRAGMA query_only", [], |row| row.get(0)).unwrap();
    let write_error = conn.execute("INSERT INTO items (value) VALUES ('writer')", []).unwrap_err();

    assert_eq!(journal_mode.to_ascii_lowercase(), "delete");
    assert_eq!(query_only, 1);
    assert!(write_error.to_string().contains("readonly"));
    let _ = fs::remove_dir_all(root);
  }

  #[test]
  fn open_existing_database_readonly_does_not_wait_for_wal_write_guard() {
    let root = std::env::temp_dir().join(format!("soma-existing-db-readonly-guard-test-{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&root).unwrap();
    let db_path = root.join(DB_FILE);
    {
      let conn = Connection::open(&db_path).unwrap();
      conn
        .execute_batch(
          r#"
                PRAGMA journal_mode = DELETE;
                CREATE TABLE items (id INTEGER PRIMARY KEY, value TEXT NOT NULL);
                "#,
        )
        .unwrap();
    }

    let guard = sqlite_write_guard().unwrap();
    let open_path = db_path.clone();
    let (done_tx, done_rx) = std::sync::mpsc::channel();
    let handle = std::thread::spawn(move || {
      done_tx.send(open_existing_database_readonly(&open_path).map(|_| ())).unwrap();
    });

    let result =
      done_rx.recv_timeout(Duration::from_millis(200)).expect("read-only open should not wait for the WAL write guard");
    drop(guard);
    handle.join().unwrap();

    result.unwrap();
    let _ = fs::remove_dir_all(root);
  }

  #[test]
  fn open_existing_database_wal_allows_writer_while_reader_is_active() {
    let root = std::env::temp_dir().join(format!("soma-existing-db-wal-reader-test-{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&root).unwrap();
    let db_path = root.join(DB_FILE);
    {
      let conn = Connection::open(&db_path).unwrap();
      conn
        .execute_batch(
          r#"
                PRAGMA journal_mode = DELETE;
                CREATE TABLE items (id INTEGER PRIMARY KEY, value TEXT NOT NULL);
                INSERT INTO items (value) VALUES ('reader');
                "#,
        )
        .unwrap();
    }

    let writer = open_existing_database(&db_path).unwrap();
    let reader = open_existing_database(&db_path).unwrap();
    let mut stmt = reader.prepare("SELECT value FROM items").unwrap();
    let mut rows = stmt.query([]).unwrap();
    assert!(rows.next().unwrap().is_some());

    with_write_transaction(&writer, |conn| {
      conn.execute("INSERT INTO items (value) VALUES ('writer')", [])?;
      Ok(())
    })
    .unwrap();

    drop(rows);
    drop(stmt);
    drop(reader);
    drop(writer);
    let _ = fs::remove_dir_all(root);
  }

  #[test]
  fn open_existing_database_does_not_create_missing_database() {
    let root = std::env::temp_dir().join(format!("soma-existing-db-missing-{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&root).unwrap();
    let db_path = root.join(DB_FILE);

    let error = open_existing_database(&db_path).unwrap_err();

    assert_eq!(error.code, "Soma_STORAGE_ERROR");
    assert!(!db_path.exists());
    let _ = fs::remove_dir_all(root);
  }

  #[test]
  fn open_database_waits_while_enabling_wal() {
    let root = std::env::temp_dir().join(format!("soma-db-wal-wait-test-{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&root).unwrap();
    let db_path = root.join(DB_FILE);
    let blocker = Connection::open(&db_path).unwrap();
    blocker
      .execute_batch(
        r#"
                PRAGMA journal_mode = DELETE;
                CREATE TABLE sources (id TEXT PRIMARY KEY);
                BEGIN EXCLUSIVE;
                "#,
      )
      .unwrap();

    let open_path = db_path.clone();
    let handle = std::thread::spawn(move || open_database(&open_path).unwrap());
    std::thread::sleep(Duration::from_millis(100));
    blocker.execute_batch("COMMIT").unwrap();
    let conn = handle.join().unwrap();
    let journal_mode: String = conn.query_row("PRAGMA journal_mode", [], |row| row.get(0)).unwrap();

    assert_eq!(journal_mode.to_ascii_lowercase(), "wal");
    let _ = fs::remove_dir_all(root);
  }

  fn assert_open_repairs_schema_object(object_type: &str, name: &str, drop_sql: &str) {
    let root = std::env::temp_dir().join(format!("soma-schema-repair-test-{name}-{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&root).unwrap();
    let db_path = root.join(DB_FILE);
    {
      let conn = Connection::open(&db_path).unwrap();
      initialize_schema(&conn).unwrap();
      conn.execute_batch(drop_sql).unwrap();
      assert!(!schema_object_exists(&conn, object_type, name));
    }

    let conn = open_existing_database(&db_path).unwrap();
    assert!(schema_object_exists(&conn, object_type, name));

    drop(conn);
    let _ = fs::remove_dir_all(root);
  }

  fn schema_object_exists(conn: &Connection, object_type: &str, name: &str) -> bool {
    conn
      .query_row("SELECT 1 FROM sqlite_master WHERE type = ?1 AND name = ?2 LIMIT 1", [object_type, name], |_| Ok(()))
      .optional()
      .unwrap()
      .is_some()
  }

  fn index_exists(conn: &Connection, name: &str) -> bool {
    schema_object_exists(conn, "index", name)
  }

  fn table_exists(conn: &Connection, name: &str) -> bool {
    schema_object_exists(conn, "table", name)
  }

  fn assert_chunks_fts_matches_canonical(conn: &Connection) {
    let canonical = conn
      .prepare("SELECT rowid, content, id FROM chunks ORDER BY rowid")
      .unwrap()
      .query_map([], |row| Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?, row.get::<_, String>(2)?)))
      .unwrap()
      .collect::<Result<Vec<_>, _>>()
      .unwrap();
    let indexed = conn
      .prepare("SELECT rowid, content, chunk_id FROM chunks_fts ORDER BY rowid")
      .unwrap()
      .query_map([], |row| Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?, row.get::<_, String>(2)?)))
      .unwrap()
      .collect::<Result<Vec<_>, _>>()
      .unwrap();

    assert_eq!(indexed, canonical);
  }

  fn assert_node_body_fts_matches_canonical(conn: &Connection) {
    let canonical = conn
      .prepare("SELECT rowid, compiled_body, id, node_id FROM node_body_versions ORDER BY rowid")
      .unwrap()
      .query_map([], |row| {
        Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?, row.get::<_, String>(2)?, row.get::<_, String>(3)?))
      })
      .unwrap()
      .collect::<Result<Vec<_>, _>>()
      .unwrap();
    let indexed = conn
      .prepare("SELECT rowid, compiled_body, body_version_id, node_id FROM node_body_versions_fts ORDER BY rowid")
      .unwrap()
      .query_map([], |row| {
        Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?, row.get::<_, String>(2)?, row.get::<_, String>(3)?))
      })
      .unwrap()
      .collect::<Result<Vec<_>, _>>()
      .unwrap();

    assert_eq!(indexed, canonical);
  }

  fn node_body_fts_match_count(conn: &Connection, term: &str) -> i64 {
    conn
      .query_row("SELECT COUNT(*) FROM node_body_versions_fts WHERE node_body_versions_fts MATCH ?1", [term], |row| {
        row.get(0)
      })
      .unwrap()
  }

  fn query_plan(conn: &Connection, sql: &str) -> String {
    let mut stmt = conn.prepare(&format!("EXPLAIN QUERY PLAN {sql}")).unwrap();
    let rows = stmt.query_map([], |row| row.get::<_, String>(3)).unwrap().collect::<Result<Vec<_>, _>>().unwrap();
    rows.join("\n")
  }
}
