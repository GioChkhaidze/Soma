use rusqlite::{params, Connection};
use serde_json::{json, Value};
use time::format_description::well_known::Rfc3339;
use time::OffsetDateTime;
use uuid::Uuid;

use crate::error::CommandResult;

pub(crate) fn append_graph_user_message(conn: &Connection, content: &str) -> CommandResult<Value> {
  let message_id = new_id();
  let created_at = now_string()?;
  conn.execute(
    "INSERT INTO graph_thread_messages (id, role, content, context_json, created_at) VALUES (?1, 'user', ?2, NULL, ?3)",
    params![message_id, content, created_at],
  )?;

  Ok(json!({
    "id": message_id,
    "role": "user",
    "content": content,
    "created_at": created_at
  }))
}

pub(crate) fn append_graph_assistant_message(
  conn: &Connection,
  content: &str,
  context_packet: &Value,
) -> CommandResult<Value> {
  let message_id = new_id();
  let created_at = now_string()?;
  conn.execute(
    concat!(
      "INSERT INTO graph_thread_messages (id, role, content, context_json, created_at) ",
      "VALUES (?1, 'assistant', ?2, ?3, ?4)",
    ),
    params![message_id, content, context_packet.to_string(), created_at],
  )?;
  Ok(json!({
    "id": message_id,
    "role": "assistant",
    "content": content,
    "created_at": created_at,
    "context_packet": context_packet
  }))
}

pub(crate) fn attach_graph_message_context(
  conn: &Connection,
  message_id: &str,
  context_packet: &Value,
) -> CommandResult<()> {
  conn.execute(
    "UPDATE graph_thread_messages SET context_json = ?1 WHERE id = ?2",
    params![context_packet.to_string(), message_id],
  )?;
  Ok(())
}

pub(crate) fn recent_graph_thread_messages(conn: &Connection, limit: i64) -> CommandResult<Vec<Value>> {
  let mut stmt = conn.prepare(
    "SELECT id, role, content, created_at FROM graph_thread_messages ORDER BY created_at DESC, id DESC LIMIT ?1",
  )?;
  let rows = stmt.query_map(params![limit], |row| {
    Ok(json!({
      "id": row.get::<_, String>(0)?,
      "role": row.get::<_, String>(1)?,
      "content": row.get::<_, String>(2)?,
      "created_at": row.get::<_, String>(3)?
    }))
  })?;
  let mut messages = rows.collect::<Result<Vec<_>, _>>()?;
  messages.reverse();
  Ok(messages)
}

pub(crate) fn append_node_user_message(conn: &Connection, node_id: &str, content: &str) -> CommandResult<Value> {
  let message_id = new_id();
  let created_at = now_string()?;
  conn.execute(
    concat!(
      "INSERT INTO node_thread_messages (id, node_id, role, content, context_json, created_at) ",
      "VALUES (?1, ?2, 'user', ?3, NULL, ?4)",
    ),
    params![message_id, node_id, content, created_at],
  )?;

  Ok(json!({
      "id": message_id,
      "node_id": node_id,
      "role": "user",
      "content": content,
      "created_at": created_at
  }))
}

pub(crate) fn append_node_assistant_message(
  conn: &Connection,
  node_id: &str,
  content: &str,
  context_packet: &Value,
) -> CommandResult<Value> {
  let message_id = new_id();
  let created_at = now_string()?;
  conn.execute(
    concat!(
      "INSERT INTO node_thread_messages (id, node_id, role, content, context_json, created_at) ",
      "VALUES (?1, ?2, 'assistant', ?3, ?4, ?5)",
    ),
    params![message_id, node_id, content, context_packet.to_string(), created_at],
  )?;
  Ok(json!({
    "id": message_id,
    "node_id": node_id,
    "role": "assistant",
    "content": content,
    "created_at": created_at,
    "context_packet": context_packet
  }))
}

pub(crate) fn attach_node_message_context(
  conn: &Connection,
  message_id: &str,
  context_packet: &Value,
) -> CommandResult<()> {
  conn.execute(
    "UPDATE node_thread_messages SET context_json = ?1 WHERE id = ?2",
    params![context_packet.to_string(), message_id],
  )?;
  Ok(())
}

pub(crate) fn recent_node_thread_messages(conn: &Connection, node_id: &str, limit: i64) -> CommandResult<Vec<Value>> {
  let mut stmt = conn.prepare(concat!(
    "SELECT id, node_id, role, content, created_at FROM node_thread_messages WHERE node_id = ?1 ",
    "ORDER BY created_at DESC, id DESC LIMIT ?2",
  ))?;
  let rows = stmt.query_map(params![node_id, limit], |row| {
    Ok(json!({
      "id": row.get::<_, String>(0)?,
      "node_id": row.get::<_, String>(1)?,
      "role": row.get::<_, String>(2)?,
      "content": row.get::<_, String>(3)?,
      "created_at": row.get::<_, String>(4)?
    }))
  })?;
  let mut messages = rows.collect::<Result<Vec<_>, _>>()?;
  messages.reverse();
  Ok(messages)
}

fn now_string() -> CommandResult<String> {
  Ok(OffsetDateTime::now_utc().format(&Rfc3339)?)
}

fn new_id() -> String {
  Uuid::new_v4().to_string()
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn graph_recent_messages_omit_context_packets() {
    let conn = Connection::open_in_memory().unwrap();
    conn
      .execute_batch(
        r#"
            CREATE TABLE graph_thread_messages (
              id TEXT PRIMARY KEY,
              role TEXT NOT NULL,
              content TEXT NOT NULL,
              context_json TEXT,
              created_at TEXT NOT NULL
            );
            "#,
      )
      .unwrap();
    conn
      .execute(
        concat!(
          "INSERT INTO graph_thread_messages (id, role, content, context_json, created_at) ",
          "VALUES ('a', 'user', 'first', '{\"mode\":\"graph_chat\"}', '2026-01-01T00:00:00Z')",
        ),
        [],
      )
      .unwrap();
    conn
      .execute(
        concat!(
          "INSERT INTO graph_thread_messages (id, role, content, context_json, created_at) ",
          "VALUES ('b', 'assistant', 'second', '{\"mode\":\"graph_chat\"}', '2026-01-01T00:00:01Z')",
        ),
        [],
      )
      .unwrap();

    let messages = recent_graph_thread_messages(&conn, 30).unwrap();

    assert_eq!(messages[0]["id"], "a");
    assert!(messages[0].get("context_packet").is_none());
    assert!(messages[1].get("context_packet").is_none());
  }

  #[test]
  fn node_recent_messages_omit_context_packets() {
    let conn = Connection::open_in_memory().unwrap();
    conn
      .execute_batch(
        r#"
            CREATE TABLE node_thread_messages (
              id TEXT PRIMARY KEY,
              node_id TEXT NOT NULL,
              role TEXT NOT NULL,
              content TEXT NOT NULL,
              context_json TEXT,
              created_at TEXT NOT NULL
            );
            "#,
      )
      .unwrap();
    conn
      .execute(
        concat!(
          "INSERT INTO node_thread_messages (id, node_id, role, content, context_json, created_at) ",
          "VALUES ('a', 'node_1', 'user', 'first', '{\"mode\":\"node_chat\"}', '2026-01-01T00:00:00Z')",
        ),
        [],
      )
      .unwrap();

    let messages = recent_node_thread_messages(&conn, "node_1", 30).unwrap();

    assert_eq!(messages[0]["id"], "a");
    assert_eq!(messages[0]["node_id"], "node_1");
    assert_eq!(messages[0]["role"], "user");
    assert_eq!(messages[0]["content"], "first");
    assert_eq!(messages[0]["created_at"], "2026-01-01T00:00:00Z");
    assert!(messages[0].get("context_packet").is_none());
  }
}
