use std::collections::HashSet;

use rusqlite::{params, Connection, OptionalExtension};
use serde_json::{json, Value};

use crate::contracts::{
  edge_source_ref, edge_target_ref, edge_type, node_type, source_chunk_ids, source_message_ids, word_count,
  NODE_BODY_MAX_CHARS, NODE_BODY_MAX_WORDS,
};
use crate::error::{CommandError, CommandResult};

use super::{graph_thread_message_exists, new_id, now_string, require_node_thread_message, required_payload_string};

pub(super) struct AcceptedEntity {
  pub(super) entity_type: String,
  pub(super) entity_id: String,
  pub(super) undo_entity_type: String,
  pub(super) undo_entity_id: String,
  pub(super) inserted_evidence: Vec<InsertedEvidence>,
}

pub(super) struct InsertedEvidence {
  pub(super) table: &'static str,
  pub(super) id: String,
}

pub(crate) fn update_node_body(conn: &Connection, node_id: &str, compiled_body: &str) -> CommandResult<Value> {
  let node_id = node_id.trim();
  if node_id.is_empty() {
    return Err(CommandError::validation("Node id is required."));
  }
  let compiled_body = compiled_body.trim();
  require_active_node(conn, node_id)?;
  validate_active_body_input(compiled_body, false, true)?;
  let created_at = now_string()?;
  let version_id = insert_node_body_version(conn, node_id, compiled_body, true, &[], &created_at)?;
  let version_number = node_body_version_number(conn, &version_id)?;
  conn.execute(
    "UPDATE graph_nodes SET current_body_version_id = ?1, updated_at = ?2 WHERE id = ?3",
    params![version_id, created_at, node_id],
  )?;
  Ok(json!({
    "nodeId": node_id,
    "bodyVersion": version_number,
    "bodyVersionId": version_id
  }))
}

pub(crate) fn rollback_node_body(conn: &Connection, node_id: &str, version_number: i64) -> CommandResult<Value> {
  let node_id = node_id.trim();
  if node_id.is_empty() {
    return Err(CommandError::validation("Node id is required."));
  }
  if version_number < 1 {
    return Err(CommandError::validation("Node body version number must be positive."));
  }
  require_active_node(conn, node_id)?;
  let version_id: Option<String> = conn
    .query_row(
      "SELECT id FROM node_body_versions WHERE node_id = ?1 AND version_number = ?2",
      params![node_id, version_number],
      |row| row.get(0),
    )
    .optional()?;
  let version_id = version_id
    .ok_or_else(|| CommandError::not_found(format!("Node body version not found: {node_id} v{version_number}")))?;
  let updated_at = now_string()?;
  conn.execute(
    "UPDATE graph_nodes SET current_body_version_id = ?1, updated_at = ?2 WHERE id = ?3",
    params![version_id, updated_at, node_id],
  )?;
  Ok(json!({
    "nodeId": node_id,
    "bodyVersion": version_number,
    "bodyVersionId": version_id
  }))
}

pub(super) fn accept_proposal_into_graph(conn: &Connection, proposal: &Value) -> CommandResult<AcceptedEntity> {
  match proposal.get("type").and_then(Value::as_str).unwrap_or("") {
    "node" => accept_node_proposal(conn, proposal),
    "edge" => accept_edge_proposal(conn, proposal),
    "node_body_update" => accept_node_body_update_proposal(conn, proposal),
    "edge_bridge_update" => accept_edge_bridge_update_proposal(conn, proposal),
    "message_evidence_attachment" => accept_message_evidence_attachment_proposal(conn, proposal),
    "merge_candidate" => Err(CommandError::validation(
      "Merge candidates support Reject or Later, but not Accept until transactional merging is implemented.",
    )),
    proposal_type => Err(CommandError::validation(format!("Accepting {proposal_type} proposals is not implemented."))),
  }
}

fn accept_node_proposal(conn: &Connection, proposal: &Value) -> CommandResult<AcceptedEntity> {
  let payload = &proposal["payload"];
  let chunk_ids = source_chunk_ids(payload);
  let message_ids = proposal_source_message_ids(conn, proposal)?;
  let compiled_body = required_payload_string(payload, "compiled_body")?;
  validate_active_body_input(compiled_body, !chunk_ids.is_empty() || !message_ids.is_empty(), false)?;
  let created_at = now_string()?;
  let node_id = new_id();
  let body_version_id = new_id();
  let node_type = node_type(payload).ok_or_else(|| CommandError::validation("Node proposal is missing type."))?;
  let title = required_payload_string(payload, "title")?;
  let preview = payload.get("preview").and_then(Value::as_str);

  conn.execute(
    concat!(
      "INSERT INTO graph_nodes (id, node_type, title, preview, status, authored_by_user, created_at, updated_at) ",
      "VALUES (?1, ?2, ?3, ?4, 'active', 0, ?5, ?5)"
    ),
    params![node_id, node_type, title, preview, created_at],
  )?;
  insert_evidence_links(conn, "node", &node_id, &chunk_ids, &created_at)?;
  insert_message_evidence_links(conn, "node", &node_id, &message_ids, None, &created_at)?;
  insert_node_body_version_with_id(conn, &body_version_id, &node_id, compiled_body, false, &chunk_ids, &created_at)?;
  insert_message_evidence_links(conn, "node_body_version", &body_version_id, &message_ids, None, &created_at)?;
  conn
    .execute("UPDATE graph_nodes SET current_body_version_id = ?1 WHERE id = ?2", params![body_version_id, node_id])?;

  Ok(AcceptedEntity {
    entity_type: "node".to_string(),
    entity_id: node_id.clone(),
    undo_entity_type: "node".to_string(),
    undo_entity_id: node_id,
    inserted_evidence: Vec::new(),
  })
}

fn accept_edge_proposal(conn: &Connection, proposal: &Value) -> CommandResult<AcceptedEntity> {
  let payload = &proposal["payload"];
  let chunk_ids = source_chunk_ids(payload);
  let message_ids = proposal_source_message_ids(conn, proposal)?;
  if chunk_ids.is_empty() && message_ids.is_empty() {
    return Err(CommandError::validation("Cannot accept edge proposal without evidence or explicit user authorship."));
  }
  let source = resolve_node_ref(conn, proposal["patch_id"].as_str().unwrap_or(""), edge_source_ref(payload))?;
  let target = resolve_node_ref(conn, proposal["patch_id"].as_str().unwrap_or(""), edge_target_ref(payload))?;
  if source == target {
    return Err(CommandError::validation("Cannot accept a self-edge proposal."));
  }
  let edge_type = edge_type(payload).ok_or_else(|| CommandError::validation("Edge proposal is missing type."))?;
  let bridge_text = optional_payload_string(payload, "bridge_text");
  let created_at = now_string()?;
  let edge_id = new_id();
  conn.execute(
    concat!(
      "INSERT INTO graph_edges (id, source_node_id, target_node_id, edge_type, bridge_text, status, ",
      "authored_by_user, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?5, 'active', 0, ?6, ?6)"
    ),
    params![edge_id, source, target, edge_type, bridge_text, created_at],
  )?;
  insert_evidence_links(conn, "edge", &edge_id, &chunk_ids, &created_at)?;
  insert_message_evidence_links(conn, "edge", &edge_id, &message_ids, None, &created_at)?;
  Ok(AcceptedEntity {
    entity_type: "edge".to_string(),
    entity_id: edge_id.clone(),
    undo_entity_type: "edge".to_string(),
    undo_entity_id: edge_id,
    inserted_evidence: Vec::new(),
  })
}

fn optional_payload_string<'a>(payload: &'a Value, field: &str) -> Option<&'a str> {
  payload.get(field).and_then(Value::as_str).map(str::trim).filter(|value| !value.is_empty())
}

fn accept_node_body_update_proposal(conn: &Connection, proposal: &Value) -> CommandResult<AcceptedEntity> {
  let payload = &proposal["payload"];
  let node_id = payload
    .get("target_node_id")
    .or_else(|| payload.get("node_id"))
    .and_then(Value::as_str)
    .ok_or_else(|| CommandError::validation("Node body update target_node_id is required."))?;
  require_active_node(conn, node_id)?;
  require_matching_body_version(conn, node_id, payload)?;
  let chunk_ids = source_chunk_ids(payload);
  let message_ids = proposal_source_message_ids(conn, proposal)?;
  let compiled_body = node_body_update_text(conn, node_id, payload)?;
  validate_active_body_input(&compiled_body, !chunk_ids.is_empty() || !message_ids.is_empty(), false)?;
  let version_id = insert_node_body_version(conn, node_id, &compiled_body, false, &chunk_ids, &now_string()?)?;
  let updated_at = now_string()?;
  insert_message_evidence_links(conn, "node_body_version", &version_id, &message_ids, None, &updated_at)?;
  conn.execute(
    "UPDATE graph_nodes SET current_body_version_id = ?1, updated_at = ?2 WHERE id = ?3",
    params![version_id, updated_at, node_id],
  )?;
  Ok(AcceptedEntity {
    entity_type: "node_body_version".to_string(),
    entity_id: version_id.clone(),
    undo_entity_type: "node_body_version".to_string(),
    undo_entity_id: version_id,
    inserted_evidence: Vec::new(),
  })
}

fn require_matching_body_version(conn: &Connection, node_id: &str, payload: &Value) -> CommandResult<()> {
  let base_version_id = optional_payload_string(payload, "base_body_version_id").ok_or_else(|| {
    CommandError::validation(
      "The node body update has no snapshot precondition. Regenerate or reject the stale update.",
    )
  })?;
  let current_version_id: String = conn.query_row(
    "SELECT current_body_version_id FROM graph_nodes WHERE id = ?1 AND status = 'active'",
    params![node_id],
    |row| row.get(0),
  )?;
  if current_version_id != base_version_id {
    return Err(CommandError::validation(
      "The node body changed after this update was proposed. Regenerate or reject the stale update.",
    ));
  }
  Ok(())
}

fn accept_edge_bridge_update_proposal(conn: &Connection, proposal: &Value) -> CommandResult<AcceptedEntity> {
  let payload = &proposal["payload"];
  let edge_id = payload
    .get("target_edge_id")
    .or_else(|| payload.get("edge_id"))
    .and_then(Value::as_str)
    .ok_or_else(|| CommandError::validation("Edge bridge update target_edge_id is required."))?;
  require_active_edge(conn, edge_id)?;
  require_matching_edge_revision(conn, edge_id, payload)?;
  let chunk_ids = source_chunk_ids(payload);
  let message_ids = proposal_source_message_ids(conn, proposal)?;
  if chunk_ids.is_empty() && message_ids.is_empty() {
    return Err(CommandError::validation(
      "Cannot accept edge bridge update without evidence or explicit user authorship.",
    ));
  }
  let bridge_text = required_payload_string(payload, "bridge_text")?;
  let updated_at = now_string()?;
  conn.execute(
    "UPDATE graph_edges SET bridge_text = ?1, updated_at = ?2 WHERE id = ?3",
    params![bridge_text, updated_at, edge_id],
  )?;
  let mut inserted_evidence = insert_evidence_links(conn, "edge", edge_id, &chunk_ids, &updated_at)?;
  inserted_evidence.extend(insert_message_evidence_links(conn, "edge", edge_id, &message_ids, None, &updated_at)?);
  Ok(AcceptedEntity {
    entity_type: "edge".to_string(),
    entity_id: edge_id.to_string(),
    undo_entity_type: "edge".to_string(),
    undo_entity_id: edge_id.to_string(),
    inserted_evidence,
  })
}

fn require_matching_edge_revision(conn: &Connection, edge_id: &str, payload: &Value) -> CommandResult<()> {
  let base_updated_at = optional_payload_string(payload, "base_edge_updated_at").ok_or_else(|| {
    CommandError::validation(
      "The edge bridge update has no snapshot precondition. Regenerate or reject the stale update.",
    )
  })?;
  let current_updated_at: String = conn.query_row(
    "SELECT updated_at FROM graph_edges WHERE id = ?1 AND status = 'active'",
    params![edge_id],
    |row| row.get(0),
  )?;
  if current_updated_at != base_updated_at {
    return Err(CommandError::validation(
      "The edge changed after this bridge update was proposed. Regenerate or reject the stale update.",
    ));
  }
  Ok(())
}

fn accept_message_evidence_attachment_proposal(conn: &Connection, proposal: &Value) -> CommandResult<AcceptedEntity> {
  let payload = &proposal["payload"];
  let target_type = required_payload_string(payload, "target_entity_type")?;
  let target_id = required_payload_string(payload, "target_entity_id")?;
  let message_id = required_payload_string(payload, "message_id")?;
  require_evidence_target(conn, target_type, target_id)?;
  let quote_excerpt = payload.get("quote_excerpt").and_then(Value::as_str);
  let created_at = now_string()?;
  let evidence_id = new_id();
  let undo_entity_type = if graph_thread_message_exists(conn, message_id)? {
    conn.execute(
      concat!(
        "INSERT INTO graph_message_evidence (id, target_entity_type, target_entity_id, ",
        "graph_thread_message_id, quote_excerpt, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6)"
      ),
      params![evidence_id, target_type, target_id, message_id, quote_excerpt, created_at],
    )?;
    "graph_message_evidence"
  } else {
    require_node_thread_message(conn, message_id)?;
    conn.execute(
      concat!(
        "INSERT INTO node_message_evidence (id, target_entity_type, target_entity_id, ",
        "node_thread_message_id, quote_excerpt, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6)"
      ),
      params![evidence_id, target_type, target_id, message_id, quote_excerpt, created_at],
    )?;
    "node_message_evidence"
  };
  Ok(AcceptedEntity {
    entity_type: target_type.to_string(),
    entity_id: target_id.to_string(),
    undo_entity_type: undo_entity_type.to_string(),
    undo_entity_id: evidence_id,
    inserted_evidence: Vec::new(),
  })
}

pub(crate) fn require_active_node(conn: &Connection, node_id: &str) -> CommandResult<()> {
  let exists: Option<String> = conn
    .query_row("SELECT id FROM graph_nodes WHERE id = ?1 AND status = 'active'", params![node_id], |row| row.get(0))
    .optional()?;
  exists.map(|_| ()).ok_or_else(|| CommandError::not_found(format!("Active node not found: {node_id}")))
}

fn insert_evidence_links(
  conn: &Connection,
  entity_type: &str,
  entity_id: &str,
  chunk_ids: &[String],
  created_at: &str,
) -> CommandResult<Vec<InsertedEvidence>> {
  let mut inserted = Vec::new();
  for chunk_id in chunk_ids {
    let message_id: Option<String> =
      conn.query_row("SELECT message_id FROM chunks WHERE id = ?1", params![chunk_id], |row| row.get(0)).optional()?;
    let message_id =
      message_id.ok_or_else(|| CommandError::not_found(format!("Unknown evidence chunk id: {chunk_id}")))?;
    let evidence_id = new_id();
    conn.execute(
      concat!(
        "INSERT INTO graph_evidence (id, entity_type, entity_id, chunk_id, message_id, quote_excerpt, ",
        "created_at) VALUES (?1, ?2, ?3, ?4, ?5, NULL, ?6)"
      ),
      params![evidence_id, entity_type, entity_id, chunk_id, message_id, created_at],
    )?;
    inserted.push(InsertedEvidence { table: "graph_evidence", id: evidence_id });
  }
  Ok(inserted)
}

fn insert_message_evidence_links(
  conn: &Connection,
  entity_type: &str,
  entity_id: &str,
  message_ids: &[String],
  quote_excerpt: Option<&str>,
  created_at: &str,
) -> CommandResult<Vec<InsertedEvidence>> {
  let mut inserted = Vec::new();
  for message_id in message_ids {
    if graph_thread_message_exists(conn, message_id)? {
      let evidence_id = new_id();
      conn.execute(
        concat!(
          "INSERT INTO graph_message_evidence (id, target_entity_type, target_entity_id, ",
          "graph_thread_message_id, quote_excerpt, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6)"
        ),
        params![evidence_id, entity_type, entity_id, message_id, quote_excerpt, created_at],
      )?;
      inserted.push(InsertedEvidence { table: "graph_message_evidence", id: evidence_id });
    } else {
      require_node_thread_message(conn, message_id)?;
      let evidence_id = new_id();
      conn.execute(
        concat!(
          "INSERT INTO node_message_evidence (id, target_entity_type, target_entity_id, ",
          "node_thread_message_id, quote_excerpt, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6)"
        ),
        params![evidence_id, entity_type, entity_id, message_id, quote_excerpt, created_at],
      )?;
      inserted.push(InsertedEvidence { table: "node_message_evidence", id: evidence_id });
    }
  }
  Ok(inserted)
}

fn proposal_source_message_ids(conn: &Connection, proposal: &Value) -> CommandResult<Vec<String>> {
  let mut seen = HashSet::new();
  let mut ids = Vec::new();
  for id in source_message_ids(&proposal["payload"]) {
    if seen.insert(id.clone()) {
      ids.push(id);
    }
  }
  if !ids.is_empty() || !source_chunk_ids(&proposal["payload"]).is_empty() {
    return Ok(ids);
  }

  let Some(patch_id) = proposal.get("patch_id").and_then(Value::as_str) else {
    return Ok(ids);
  };
  let patch_message_id = conn
    .query_row("SELECT source_message_id FROM graph_patches WHERE id = ?1", params![patch_id], |row| {
      row.get::<_, Option<String>>(0)
    })
    .optional()?
    .flatten();
  if let Some(id) = patch_message_id.filter(|id| seen.insert(id.clone())) {
    ids.push(id);
  }
  Ok(ids)
}

fn insert_node_body_version(
  conn: &Connection,
  node_id: &str,
  compiled_body: &str,
  user_authored: bool,
  chunk_ids: &[String],
  created_at: &str,
) -> CommandResult<String> {
  let version_id = new_id();
  insert_node_body_version_with_id(conn, &version_id, node_id, compiled_body, user_authored, chunk_ids, created_at)?;
  Ok(version_id)
}

fn node_body_version_number(conn: &Connection, version_id: &str) -> CommandResult<i64> {
  conn
    .query_row("SELECT version_number FROM node_body_versions WHERE id = ?1", params![version_id], |row| row.get(0))
    .optional()?
    .ok_or_else(|| CommandError::not_found(format!("Node body version not found: {version_id}")))
}

fn insert_node_body_version_with_id(
  conn: &Connection,
  version_id: &str,
  node_id: &str,
  compiled_body: &str,
  user_authored: bool,
  chunk_ids: &[String],
  created_at: &str,
) -> CommandResult<()> {
  let version_number: i64 = conn.query_row(
    "SELECT COALESCE(MAX(version_number), 0) + 1 FROM node_body_versions WHERE node_id = ?1",
    params![node_id],
    |row| row.get(0),
  )?;
  conn.execute(
    concat!(
      "INSERT INTO node_body_versions ",
      "(id, node_id, version_number, compiled_body, authored_by_user, created_at) ",
      "VALUES (?1, ?2, ?3, ?4, ?5, ?6)"
    ),
    params![version_id, node_id, version_number, compiled_body, if user_authored { 1 } else { 0 }, created_at],
  )?;
  insert_evidence_links(conn, "node_body_version", version_id, chunk_ids, created_at)?;
  Ok(())
}

fn node_body_update_text(conn: &Connection, node_id: &str, payload: &Value) -> CommandResult<String> {
  let kind = payload.get("update_kind").and_then(Value::as_str).unwrap_or("replace_body");
  if kind == "replace_body" {
    return Ok(required_payload_string(payload, "compiled_body")?.to_string());
  }
  if kind == "append_section" {
    let mut sections = split_body_sections(&current_node_body(conn, node_id)?);
    sections.push(required_payload_string(payload, "section_text")?.to_string());
    return Ok(sections.join("\n\n"));
  }
  Err(CommandError::validation(format!("Unsupported node body update kind: {kind}")))
}

fn current_node_body(conn: &Connection, node_id: &str) -> CommandResult<String> {
  conn
    .query_row(
      r#"
      SELECT node_body_versions.compiled_body
      FROM graph_nodes
      JOIN node_body_versions ON graph_nodes.current_body_version_id = node_body_versions.id
      WHERE graph_nodes.id = ?1 AND graph_nodes.status = 'active'
      "#,
      params![node_id],
      |row| row.get(0),
    )
    .optional()?
    .ok_or_else(|| CommandError::not_found(format!("Active node body not found: {node_id}")))
}

fn require_active_edge(conn: &Connection, edge_id: &str) -> CommandResult<()> {
  let exists: Option<String> = conn
    .query_row("SELECT id FROM graph_edges WHERE id = ?1 AND status = 'active'", params![edge_id], |row| row.get(0))
    .optional()?;
  exists.map(|_| ()).ok_or_else(|| CommandError::not_found(format!("Active edge not found: {edge_id}")))
}

fn require_evidence_target(conn: &Connection, entity_type: &str, entity_id: &str) -> CommandResult<()> {
  match entity_type {
    "node" => require_active_node(conn, entity_id),
    "edge" => require_active_edge(conn, entity_id),
    "node_body_version" => {
      let exists: Option<String> = conn
        .query_row(
          r#"
                    SELECT node_body_versions.id
                    FROM node_body_versions
                    JOIN graph_nodes
                      ON graph_nodes.id = node_body_versions.node_id
                     AND graph_nodes.current_body_version_id = node_body_versions.id
                     AND graph_nodes.status = 'active'
                    WHERE node_body_versions.id = ?1
                    "#,
          params![entity_id],
          |row| row.get(0),
        )
        .optional()?;
      exists.map(|_| ()).ok_or_else(|| {
        CommandError::validation(format!(
          "Message evidence can only target the current active node body version: {entity_id}"
        ))
      })
    }
    _ => Err(CommandError::validation(format!("Unsupported message evidence target entity type: {entity_type}"))),
  }
}

fn validate_active_body_input(compiled_body: &str, has_evidence: bool, user_authored: bool) -> CommandResult<()> {
  if compiled_body.trim().is_empty() {
    return Err(CommandError::validation("compiled_body must be a non-empty string."));
  }
  if compiled_body.chars().count() > NODE_BODY_MAX_CHARS {
    return Err(CommandError::validation(format!("compiled_body must not exceed {NODE_BODY_MAX_CHARS} characters.")));
  }
  if word_count(compiled_body) > NODE_BODY_MAX_WORDS {
    return Err(CommandError::validation(format!("compiled_body must not exceed {NODE_BODY_MAX_WORDS} words.")));
  }
  if !user_authored && !has_evidence {
    return Err(CommandError::validation(
      "Cannot create an active compiled body without evidence or explicit user authorship.",
    ));
  }
  Ok(())
}

fn resolve_node_ref(conn: &Connection, patch_id: &str, node_ref: Option<String>) -> CommandResult<String> {
  let node_ref = node_ref.ok_or_else(|| CommandError::validation("Edge proposal is missing a node reference."))?;
  if let Some(active_id) = conn
    .query_row("SELECT id FROM graph_nodes WHERE id = ?1 AND status = 'active'", params![node_ref], |row| row.get(0))
    .optional()?
  {
    return Ok(active_id);
  }
  let accepted_id: Option<String> = conn
    .query_row(
      concat!(
        "SELECT accepted_entity_id FROM graph_proposals WHERE patch_id = ?1 AND temp_id = ?2 ",
        "AND status = 'accepted' AND accepted_entity_type = 'node'"
      ),
      params![patch_id, node_ref],
      |row| row.get(0),
    )
    .optional()?;
  accepted_id.ok_or_else(|| {
    CommandError::validation(format!("Node reference has not been accepted into graph truth: {node_ref}"))
  })
}

fn split_body_sections(compiled_body: &str) -> Vec<String> {
  compiled_body
    .replace("\r\n", "\n")
    .split("\n\n")
    .map(str::trim)
    .filter(|section| !section.is_empty())
    .map(String::from)
    .collect()
}
