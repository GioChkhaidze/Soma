use std::collections::HashSet;

use rusqlite::{params, Connection, OptionalExtension};
use serde_json::{json, Value};
use time::format_description::well_known::Rfc3339;
use time::OffsetDateTime;
use uuid::Uuid;

use crate::contracts::{
  attach_source_message_id, graph_patch_proposal_count, normalize_graph_patch_for_review, proposal_ref,
  validate_graph_patch_for_review,
};
use crate::error::{CommandError, CommandResult};

#[path = "graph_acceptance.rs"]
mod graph_acceptance;

use graph_acceptance::accept_proposal_into_graph;
pub(crate) use graph_acceptance::{require_active_node, rollback_node_body, update_node_body};

pub(crate) struct PersistPatchOptions<'a> {
  pub(crate) source: &'a str,
  pub(crate) source_message_id: Option<&'a str>,
  pub(crate) job_id: Option<&'a str>,
  pub(crate) proposal_status: &'a str,
}

pub(crate) struct PersistedPatch {
  pub(crate) patch_id: String,
  pub(crate) proposals: Vec<Value>,
}

struct GraphPatchUndoRecord {
  patch_id: String,
  source: String,
  source_message_id: Option<String>,
  changes: Vec<Value>,
  created_at: String,
}

#[cfg(test)]
pub(crate) fn propose_graph_updates(conn: &Connection, message_id: &str, patch: Value) -> CommandResult<Value> {
  propose_graph_updates_from_messages(conn, message_id, message_id, patch)
}

pub(crate) fn propose_graph_updates_with_evidence_message(
  conn: &Connection,
  source_message_id: &str,
  evidence_message_id: &str,
  patch: Value,
) -> CommandResult<Value> {
  propose_graph_updates_from_messages(conn, source_message_id, evidence_message_id, patch)
}

fn propose_graph_updates_from_messages(
  conn: &Connection,
  source_message_id: &str,
  evidence_message_id: &str,
  patch: Value,
) -> CommandResult<Value> {
  let message = require_graph_thread_message(conn, source_message_id)?;
  let evidence_message = require_graph_thread_message(conn, evidence_message_id)?;
  let (patch, repair_warnings) = normalize_graph_patch_for_review(&patch);
  let patch = attach_source_message_id(patch, evidence_message.get("id").and_then(Value::as_str));
  let validation = validate_graph_patch_for_review(&patch, &active_node_ids(conn)?, &active_edge_ids(conn)?);
  let mut warnings = repair_warnings;
  warnings.extend(validation.warnings);
  if !validation.valid {
    return Ok(json!({
      "messageId": message["id"],
      "valid": false,
      "imported": false,
      "trusted": false,
      "errors": validation.errors,
      "warnings": warnings
    }));
  }

  if graph_patch_proposal_count(&patch) == 0 {
    return Ok(json!({
      "messageId": message["id"],
      "valid": true,
      "imported": false,
      "trusted": false,
      "proposalCount": 0,
      "proposals": [],
      "errors": [],
      "warnings": warnings
    }));
  }

  let persisted = persist_graph_patch_proposals(
    conn,
    &patch,
    PersistPatchOptions {
      source: "graph_thread_message",
      source_message_id: message.get("id").and_then(Value::as_str),
      job_id: None,
      proposal_status: "proposed",
    },
  )?;

  Ok(json!({
    "messageId": message["id"],
    "patchId": persisted.patch_id,
    "valid": true,
    "imported": true,
    "trusted": false,
    "proposal_status": "proposed",
    "proposalCount": persisted.proposals.len(),
    "proposals": persisted.proposals,
    "errors": [],
    "warnings": warnings
  }))
}

pub(crate) fn propose_node_updates(
  conn: &Connection,
  source_message_id: &str,
  evidence_message_id: &str,
  patch: Value,
) -> CommandResult<Value> {
  let message = require_node_thread_message(conn, source_message_id)?;
  let evidence_message = require_node_thread_message(conn, evidence_message_id)?;
  if message["node_id"] != evidence_message["node_id"] {
    return Err(CommandError::validation(
      "Node chat patch source and evidence messages must belong to the same node thread.",
    ));
  }
  let (patch, repair_warnings) = normalize_graph_patch_for_review(&patch);
  let patch = attach_source_message_id(patch, evidence_message.get("id").and_then(Value::as_str));
  let validation = validate_graph_patch_for_review(&patch, &active_node_ids(conn)?, &active_edge_ids(conn)?);
  let mut warnings = repair_warnings;
  warnings.extend(validation.warnings);
  if !validation.valid {
    return Ok(json!({
      "messageId": message["id"],
      "valid": false,
      "imported": false,
      "trusted": false,
      "errors": validation.errors,
      "warnings": warnings
    }));
  }

  if graph_patch_proposal_count(&patch) == 0 {
    return Ok(json!({
      "messageId": message["id"],
      "valid": true,
      "imported": false,
      "trusted": false,
      "proposalCount": 0,
      "proposals": [],
      "errors": [],
      "warnings": warnings
    }));
  }

  let persisted = persist_graph_patch_proposals(
    conn,
    &patch,
    PersistPatchOptions {
      source: "node_thread_message",
      source_message_id: message.get("id").and_then(Value::as_str),
      job_id: None,
      proposal_status: "proposed",
    },
  )?;

  Ok(json!({
    "messageId": message["id"],
    "patchId": persisted.patch_id,
    "valid": true,
    "imported": true,
    "trusted": false,
    "proposal_status": "proposed",
    "proposalCount": persisted.proposals.len(),
    "proposals": persisted.proposals,
    "errors": [],
    "warnings": warnings
  }))
}

pub(crate) fn accept_graph_proposal(
  conn: &Connection,
  proposal_id: &str,
  reason: Option<&str>,
) -> CommandResult<Value> {
  let proposal = require_reviewable_proposal(conn, proposal_id)?;
  let result = accept_proposal_into_graph(conn, &proposal)?;
  let decided_at = now_string()?;
  conn.execute(
    concat!(
      "UPDATE graph_proposals SET status = 'accepted', accepted_entity_type = ?1, accepted_entity_id = ?2, ",
      "decided_at = ?3, decision_reason = ?4 WHERE id = ?5"
    ),
    params![result.entity_type, result.entity_id, decided_at, reason, proposal_id],
  )?;
  Ok(json!({
    "proposalId": proposal_id,
    "status": "accepted",
    "entityType": result.entity_type,
    "entityId": result.entity_id
  }))
}

pub(crate) fn accept_graph_patch_proposals(
  conn: &Connection,
  patch_id: &str,
  reason: Option<&str>,
) -> CommandResult<Value> {
  let proposals = reviewable_patch_graph_object_proposals(conn, patch_id)?;
  let decided_at = now_string()?;
  let mut accepted = Vec::new();
  let mut undo_changes = Vec::new();

  for proposal in proposals {
    let proposal_id = proposal["id"].as_str().unwrap_or("").to_string();
    let before = proposal_undo_before(conn, &proposal)?;
    let result = accept_proposal_into_graph(conn, &proposal)?;
    let after = proposal_undo_after(conn, &proposal, &result)?;
    conn.execute(
      concat!(
        "UPDATE graph_proposals SET status = 'accepted', accepted_entity_type = ?1, ",
        "accepted_entity_id = ?2, decided_at = ?3, decision_reason = ?4 WHERE id = ?5"
      ),
      params![result.entity_type, result.entity_id, decided_at, reason, proposal_id],
    )?;
    accepted.push(json!({
      "proposalId": proposal_id,
      "status": "accepted"
    }));
    undo_changes.push(json!({
      "proposal_id": proposal_id,
      "proposal_type": proposal["type"],
      "payload": proposal["payload"],
      "accepted_entity_type": result.entity_type,
      "accepted_entity_id": result.entity_id,
      "undo_entity_type": result.undo_entity_type,
      "undo_entity_id": result.undo_entity_id,
      "before": before,
      "after": after
    }));
  }

  if !undo_changes.is_empty() {
    conn.execute(
      concat!(
        "INSERT OR REPLACE INTO graph_patch_undo (patch_id, status, changes_json, created_at, undone_at) ",
        "VALUES (?1, 'ready', ?2, ?3, NULL)"
      ),
      params![patch_id, Value::Array(undo_changes).to_string(), decided_at],
    )?;
  }

  Ok(json!({
    "patchId": patch_id,
    "acceptedCount": accepted.len(),
    "accepted": accepted,
    "errors": []
  }))
}

pub(crate) fn undo_graph_patch(conn: &Connection, patch_id: &str) -> CommandResult<Value> {
  let patch_id = patch_id.trim();
  if patch_id.is_empty() {
    return Err(CommandError::validation("Graph patch id is required."));
  }
  let record = latest_ready_graph_patch_undo(conn)?
    .ok_or_else(|| CommandError::not_found(format!("Undo record not found: {patch_id}")))?;
  if record.patch_id != patch_id {
    return Err(CommandError::validation("Only the most recent graph update can be undone."));
  }
  require_safe_graph_patch_undo(conn, &record)?;
  let undone_at = now_string()?;
  let mut undone_count = 0;

  for change in record.changes.iter().rev() {
    undo_graph_change(conn, change, &undone_at)?;
    let proposal_id = required_change_string(change, "proposal_id")?;
    conn.execute(
      concat!(
        "UPDATE graph_proposals SET status = 'superseded', decided_at = ?1, decision_reason = 'undone' ",
        "WHERE id = ?2 AND status = 'accepted'"
      ),
      params![undone_at, proposal_id],
    )?;
    undone_count += 1;
  }
  conn.execute(
    "UPDATE graph_patch_undo SET status = 'undone', undone_at = ?1 WHERE patch_id = ?2",
    params![undone_at, patch_id],
  )?;

  Ok(json!({
    "patchId": patch_id,
    "undoneCount": undone_count,
    "status": "undone"
  }))
}

pub(crate) fn latest_undoable_graph_patch(conn: &Connection) -> CommandResult<Option<Value>> {
  let Some(record) = latest_ready_graph_patch_undo(conn)? else {
    return Ok(None);
  };
  if let Err(error) = require_safe_graph_patch_undo(conn, &record) {
    if error.code == "Soma_VALIDATION_ERROR" {
      return Ok(None);
    }
    return Err(error);
  }
  Ok(Some(json!({
    "patch_id": record.patch_id,
    "source": record.source,
    "source_message_id": record.source_message_id,
    "change_count": record.changes.len()
  })))
}

fn latest_ready_graph_patch_undo(conn: &Connection) -> CommandResult<Option<GraphPatchUndoRecord>> {
  let row: Option<(String, String, String, String, Option<String>)> = conn
    .query_row(
      concat!(
        "SELECT graph_patch_undo.patch_id, graph_patch_undo.changes_json, graph_patch_undo.created_at, ",
        "graph_patches.source, graph_patches.source_message_id FROM graph_patch_undo ",
        "JOIN graph_patches ON graph_patches.id = graph_patch_undo.patch_id ",
        "WHERE graph_patch_undo.status = 'ready' ",
        "ORDER BY graph_patch_undo.created_at DESC, graph_patch_undo.rowid DESC LIMIT 1"
      ),
      [],
      |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?)),
    )
    .optional()?;
  let Some((patch_id, changes_json, created_at, source, source_message_id)) = row else {
    return Ok(None);
  };
  let changes = serde_json::from_str::<Value>(&changes_json)
    .ok()
    .and_then(|value| value.as_array().cloned())
    .filter(|changes| !changes.is_empty())
    .ok_or_else(|| CommandError::storage("Graph undo record is invalid."))?;
  Ok(Some(GraphPatchUndoRecord { patch_id, source, source_message_id, changes, created_at }))
}

fn require_safe_graph_patch_undo(conn: &Connection, record: &GraphPatchUndoRecord) -> CommandResult<()> {
  let (proposal_count, accepted_count): (i64, i64) = conn.query_row(
    concat!(
      "SELECT COUNT(*), COALESCE(SUM(CASE WHEN status = 'accepted' THEN 1 ELSE 0 END), 0) ",
      "FROM graph_proposals WHERE patch_id = ?1"
    ),
    params![record.patch_id],
    |row| Ok((row.get(0)?, row.get(1)?)),
  )?;
  let change_count = record.changes.len() as i64;
  let later_accepted: i64 = conn.query_row(
    "SELECT COUNT(*) FROM graph_proposals WHERE patch_id <> ?1 AND status = 'accepted' AND decided_at > ?2",
    params![record.patch_id, record.created_at],
    |row| row.get(0),
  )?;
  if proposal_count != change_count || accepted_count != change_count || later_accepted > 0 {
    return Err(CommandError::validation("The graph changed after this update and cannot be undone safely."));
  }
  for change in &record.changes {
    require_graph_change_undoable(conn, change, &record.created_at)?;
  }
  Ok(())
}

fn require_graph_change_undoable(conn: &Connection, change: &Value, undo_created_at: &str) -> CommandResult<()> {
  let proposal_type = required_change_string(change, "proposal_type")?;
  let entity_id = required_change_string(change, "undo_entity_id")?;
  let safe = match proposal_type {
    "node" => entity_matches_undo_state(conn, "graph_nodes", entity_id, change, undo_created_at)?,
    "edge" => entity_matches_undo_state(conn, "graph_edges", entity_id, change, undo_created_at)?,
    "node_body_update" => {
      let node_id = required_change_string(&change["before"], "node_id")?;
      let expected_updated_at = required_change_string(&change["after"], "updated_at")?;
      conn
        .query_row(
          concat!(
            "SELECT current_body_version_id = ?1 AND updated_at = ?2 FROM graph_nodes ",
            "WHERE id = ?3 AND status = 'active'"
          ),
          params![entity_id, expected_updated_at, node_id],
          |row| row.get(0),
        )
        .optional()?
        .unwrap_or(false)
    }
    "edge_bridge_update" => {
      let edge_id = required_change_string(&change["before"], "edge_id")?;
      let expected_bridge = change["payload"].get("bridge_text").and_then(Value::as_str).unwrap_or("");
      let bridge_matches = conn
        .query_row(
          concat!("SELECT COALESCE(bridge_text, '') = ?1 FROM graph_edges ", "WHERE id = ?2 AND status = 'active'"),
          params![expected_bridge, edge_id],
          |row| row.get(0),
        )
        .optional()?
        .unwrap_or(false);
      bridge_matches && inserted_evidence_exists(conn, change)?
    }
    "message_evidence_attachment" => match required_change_string(change, "undo_entity_type")? {
      "graph_message_evidence" => entity_exists(conn, "graph_message_evidence", entity_id)?,
      "node_message_evidence" => entity_exists(conn, "node_message_evidence", entity_id)?,
      _ => false,
    },
    _ => false,
  };
  if safe {
    Ok(())
  } else {
    Err(CommandError::validation("The graph changed after this update and cannot be undone safely."))
  }
}

fn entity_matches_undo_state(
  conn: &Connection,
  table: &str,
  entity_id: &str,
  change: &Value,
  undo_created_at: &str,
) -> CommandResult<bool> {
  let expected_updated_at = change["after"].get("updated_at").and_then(Value::as_str);
  let sql = if expected_updated_at.is_some() {
    format!("SELECT updated_at = ?1 FROM {table} WHERE id = ?2 AND status = 'active'")
  } else {
    format!("SELECT updated_at <= ?1 FROM {table} WHERE id = ?2 AND status = 'active'")
  };
  let expected = expected_updated_at.unwrap_or(undo_created_at);
  Ok(conn.query_row(&sql, params![expected, entity_id], |row| row.get(0)).optional()?.unwrap_or(false))
}

fn entity_exists(conn: &Connection, table: &str, entity_id: &str) -> CommandResult<bool> {
  let sql = format!("SELECT 1 FROM {table} WHERE id = ?1");
  Ok(conn.query_row(&sql, params![entity_id], |_| Ok(true)).optional()?.unwrap_or(false))
}

fn proposal_undo_before(conn: &Connection, proposal: &Value) -> CommandResult<Value> {
  let payload = &proposal["payload"];
  match proposal.get("type").and_then(Value::as_str).unwrap_or("") {
    "node_body_update" => {
      let node_id = payload
        .get("target_node_id")
        .or_else(|| payload.get("node_id"))
        .and_then(Value::as_str)
        .ok_or_else(|| CommandError::validation("Node body update target is required."))?;
      let current_body_version_id: String = conn.query_row(
        "SELECT current_body_version_id FROM graph_nodes WHERE id = ?1 AND status = 'active'",
        params![node_id],
        |row| row.get(0),
      )?;
      Ok(json!({
        "node_id": node_id,
        "current_body_version_id": current_body_version_id
      }))
    }
    "edge_bridge_update" => {
      let edge_id = payload
        .get("target_edge_id")
        .or_else(|| payload.get("edge_id"))
        .and_then(Value::as_str)
        .ok_or_else(|| CommandError::validation("Edge bridge update target is required."))?;
      let bridge_text: Option<String> = conn.query_row(
        "SELECT bridge_text FROM graph_edges WHERE id = ?1 AND status = 'active'",
        params![edge_id],
        |row| row.get(0),
      )?;
      Ok(json!({
        "edge_id": edge_id,
        "bridge_text": bridge_text
      }))
    }
    _ => Ok(Value::Null),
  }
}

fn proposal_undo_after(
  conn: &Connection,
  proposal: &Value,
  accepted: &graph_acceptance::AcceptedEntity,
) -> CommandResult<Value> {
  let proposal_type = proposal.get("type").and_then(Value::as_str).unwrap_or("");
  if proposal_type == "edge_bridge_update" {
    return Ok(json!({
      "inserted_evidence": accepted.inserted_evidence.iter().map(|evidence| json!({
        "table": evidence.table,
        "id": evidence.id
      })).collect::<Vec<_>>()
    }));
  }
  if proposal_type == "node_body_update" {
    let node_id = proposal["payload"]
      .get("target_node_id")
      .or_else(|| proposal["payload"].get("node_id"))
      .and_then(Value::as_str)
      .ok_or_else(|| CommandError::validation("Node body update target is required."))?;
    let updated_at: String =
      conn.query_row("SELECT updated_at FROM graph_nodes WHERE id = ?1", params![node_id], |row| row.get(0))?;
    return Ok(json!({ "updated_at": updated_at }));
  }
  let table = match proposal_type {
    "node" => "graph_nodes",
    "edge" => "graph_edges",
    _ => return Ok(Value::Null),
  };
  let sql = format!("SELECT updated_at FROM {table} WHERE id = ?1");
  let updated_at: String = conn.query_row(&sql, params![accepted.entity_id], |row| row.get(0))?;
  Ok(json!({ "updated_at": updated_at }))
}

fn undo_graph_change(conn: &Connection, change: &Value, updated_at: &str) -> CommandResult<()> {
  let proposal_type = required_change_string(change, "proposal_type")?;
  let entity_id = required_change_string(change, "undo_entity_id")?;
  match proposal_type {
    "node" => {
      let changed = conn.execute(
        "UPDATE graph_nodes SET status = 'archived', updated_at = ?1 WHERE id = ?2 AND status = 'active'",
        params![updated_at, entity_id],
      )?;
      require_undo_change(changed, "node")
    }
    "edge" => {
      let changed = conn.execute(
        "UPDATE graph_edges SET status = 'archived', updated_at = ?1 WHERE id = ?2 AND status = 'active'",
        params![updated_at, entity_id],
      )?;
      require_undo_change(changed, "edge")
    }
    "node_body_update" => {
      let before = &change["before"];
      let node_id = required_change_string(before, "node_id")?;
      let previous_version_id = required_change_string(before, "current_body_version_id")?;
      let expected_updated_at = required_change_string(&change["after"], "updated_at")?;
      let (current_version_id, current_updated_at): (String, String) = conn.query_row(
        "SELECT current_body_version_id, updated_at FROM graph_nodes WHERE id = ?1 AND status = 'active'",
        params![node_id],
        |row| Ok((row.get(0)?, row.get(1)?)),
      )?;
      if current_version_id != entity_id || current_updated_at != expected_updated_at {
        return Err(CommandError::validation(
          "This node changed after the selected graph update and cannot be undone safely.",
        ));
      }
      conn.execute(
        "UPDATE graph_nodes SET current_body_version_id = ?1, updated_at = ?2 WHERE id = ?3",
        params![previous_version_id, updated_at, node_id],
      )?;
      Ok(())
    }
    "edge_bridge_update" => {
      let before = &change["before"];
      let edge_id = required_change_string(before, "edge_id")?;
      let expected_bridge = change["payload"].get("bridge_text").and_then(Value::as_str).unwrap_or("");
      let current_bridge: Option<String> = conn.query_row(
        "SELECT bridge_text FROM graph_edges WHERE id = ?1 AND status = 'active'",
        params![edge_id],
        |row| row.get(0),
      )?;
      if current_bridge.as_deref().unwrap_or("") != expected_bridge {
        return Err(CommandError::validation(
          "This edge changed after the selected graph update and cannot be undone safely.",
        ));
      }
      delete_inserted_evidence(conn, change)?;
      let previous_bridge = before.get("bridge_text").and_then(Value::as_str);
      conn.execute(
        "UPDATE graph_edges SET bridge_text = ?1, updated_at = ?2 WHERE id = ?3",
        params![previous_bridge, updated_at, edge_id],
      )?;
      Ok(())
    }
    "message_evidence_attachment" => {
      let undo_entity_type = required_change_string(change, "undo_entity_type")?;
      let changed = match undo_entity_type {
        "graph_message_evidence" => {
          conn.execute("DELETE FROM graph_message_evidence WHERE id = ?1", params![entity_id])?
        }
        "node_message_evidence" => {
          conn.execute("DELETE FROM node_message_evidence WHERE id = ?1", params![entity_id])?
        }
        _ => return Err(CommandError::storage("Graph undo record has an unsupported evidence type.")),
      };
      require_undo_change(changed, "message evidence")
    }
    _ => Err(CommandError::validation(format!("Undo is not supported for {proposal_type} graph updates."))),
  }
}

fn inserted_evidence_exists(conn: &Connection, change: &Value) -> CommandResult<bool> {
  let Some(evidence) = change.pointer("/after/inserted_evidence").and_then(Value::as_array) else {
    return Ok(false);
  };
  if evidence.is_empty() {
    return Ok(false);
  }
  for item in evidence {
    let table = required_change_string(item, "table")?;
    let id = required_change_string(item, "id")?;
    if !evidence_row_exists(conn, table, id)? {
      return Ok(false);
    }
  }
  Ok(true)
}

fn delete_inserted_evidence(conn: &Connection, change: &Value) -> CommandResult<()> {
  let evidence = change
    .pointer("/after/inserted_evidence")
    .and_then(Value::as_array)
    .filter(|items| !items.is_empty())
    .ok_or_else(|| CommandError::storage("Graph undo record is missing inserted edge evidence."))?;
  for item in evidence {
    let table = required_change_string(item, "table")?;
    let id = required_change_string(item, "id")?;
    let changed = match table {
      "graph_evidence" => conn.execute("DELETE FROM graph_evidence WHERE id = ?1", params![id])?,
      "graph_message_evidence" => conn.execute("DELETE FROM graph_message_evidence WHERE id = ?1", params![id])?,
      "node_message_evidence" => conn.execute("DELETE FROM node_message_evidence WHERE id = ?1", params![id])?,
      _ => return Err(CommandError::storage("Graph undo record has an unsupported inserted evidence type.")),
    };
    require_undo_change(changed, "edge evidence")?;
  }
  Ok(())
}

fn evidence_row_exists(conn: &Connection, table: &str, id: &str) -> CommandResult<bool> {
  match table {
    "graph_evidence" => entity_exists(conn, "graph_evidence", id),
    "graph_message_evidence" => entity_exists(conn, "graph_message_evidence", id),
    "node_message_evidence" => entity_exists(conn, "node_message_evidence", id),
    _ => Ok(false),
  }
}

fn require_undo_change(changed: usize, label: &str) -> CommandResult<()> {
  if changed == 1 {
    Ok(())
  } else {
    Err(CommandError::validation(format!("The {label} changed after this graph update and cannot be undone safely.")))
  }
}

fn required_change_string<'a>(change: &'a Value, field: &str) -> CommandResult<&'a str> {
  change
    .get(field)
    .and_then(Value::as_str)
    .filter(|value| !value.is_empty())
    .ok_or_else(|| CommandError::storage(format!("Graph undo record is missing {field}.")))
}

pub(crate) fn set_graph_proposal_lifecycle_status(
  conn: &Connection,
  proposal_id: &str,
  status: &str,
  reason: Option<&str>,
) -> CommandResult<Value> {
  require_reviewable_proposal(conn, proposal_id)?;
  let decided_at = now_string()?;
  conn.execute(
    "UPDATE graph_proposals SET status = ?1, decided_at = ?2, decision_reason = ?3 WHERE id = ?4",
    params![status, decided_at, reason, proposal_id],
  )?;
  Ok(json!({ "proposalId": proposal_id, "status": status }))
}

pub(crate) fn persist_graph_patch_proposals(
  conn: &Connection,
  patch: &Value,
  options: PersistPatchOptions<'_>,
) -> CommandResult<PersistedPatch> {
  let patch_id = new_id();
  let created_at = now_string()?;
  let mut proposals = Vec::new();

  conn.execute(
    concat!(
      "INSERT INTO graph_patches (id, job_id, source_message_id, source, status, created_at, errors_json) ",
      "VALUES (?1, ?2, ?3, ?4, 'imported', ?5, '[]')"
    ),
    params![patch_id, options.job_id, options.source_message_id, options.source, created_at],
  )?;

  insert_patch_proposals(
    conn,
    &patch_id,
    "node",
    patch.get("proposed_nodes"),
    &created_at,
    &mut proposals,
    options.proposal_status,
  )?;
  insert_patch_proposals(
    conn,
    &patch_id,
    "edge",
    patch.get("proposed_edges"),
    &created_at,
    &mut proposals,
    options.proposal_status,
  )?;
  insert_patch_proposals(
    conn,
    &patch_id,
    "node_body_update",
    patch.get("proposed_node_body_updates"),
    &created_at,
    &mut proposals,
    options.proposal_status,
  )?;
  insert_patch_proposals(
    conn,
    &patch_id,
    "edge_bridge_update",
    patch.get("proposed_edge_bridge_updates"),
    &created_at,
    &mut proposals,
    options.proposal_status,
  )?;
  insert_patch_proposals(
    conn,
    &patch_id,
    "message_evidence_attachment",
    patch.get("proposed_message_evidence_attachments"),
    &created_at,
    &mut proposals,
    options.proposal_status,
  )?;
  insert_patch_proposals(
    conn,
    &patch_id,
    "path",
    patch.get("proposed_paths"),
    &created_at,
    &mut proposals,
    options.proposal_status,
  )?;
  insert_patch_proposals(
    conn,
    &patch_id,
    "ambiguity",
    patch.get("ambiguities"),
    &created_at,
    &mut proposals,
    "proposed",
  )?;
  insert_patch_proposals(
    conn,
    &patch_id,
    "merge_candidate",
    patch.get("merge_candidates"),
    &created_at,
    &mut proposals,
    "proposed",
  )?;
  Ok(PersistedPatch { patch_id, proposals })
}

pub(crate) fn active_node_ids(conn: &Connection) -> CommandResult<HashSet<String>> {
  let mut stmt = conn.prepare("SELECT id FROM graph_nodes WHERE status = 'active'")?;
  let rows = stmt.query_map([], |row| row.get::<_, String>(0))?;
  rows.collect::<Result<HashSet<_>, _>>().map_err(Into::into)
}

pub(crate) fn active_edge_ids(conn: &Connection) -> CommandResult<HashSet<String>> {
  let mut stmt = conn.prepare("SELECT id FROM graph_edges WHERE status = 'active'")?;
  let rows = stmt.query_map([], |row| row.get::<_, String>(0))?;
  rows.collect::<Result<HashSet<_>, _>>().map_err(Into::into)
}

fn insert_patch_proposals(
  conn: &Connection,
  patch_id: &str,
  proposal_type: &str,
  items: Option<&Value>,
  created_at: &str,
  proposals: &mut Vec<Value>,
  proposal_status: &str,
) -> CommandResult<()> {
  for payload in items.and_then(Value::as_array).into_iter().flatten() {
    let proposal_id = new_id();
    let temp_id = proposal_ref(payload);
    conn.execute(
      concat!(
        "INSERT INTO graph_proposals (id, patch_id, proposal_type, status, temp_id, payload_json, created_at) ",
        "VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)"
      ),
      params![proposal_id, patch_id, proposal_type, proposal_status, temp_id, payload.to_string(), created_at],
    )?;
    proposals.push(json!({
      "id": proposal_id,
      "type": proposal_type,
      "status": proposal_status,
      "temp_id": temp_id
    }));
  }
  Ok(())
}

fn require_reviewable_proposal(conn: &Connection, proposal_id: &str) -> CommandResult<Value> {
  let proposal = conn
    .query_row(
      concat!(
        "SELECT id, patch_id, proposal_type, status, temp_id, payload_json, created_at, decided_at, ",
        "decision_reason FROM graph_proposals WHERE id = ?1"
      ),
      params![proposal_id],
      |row| {
        let payload_json: String = row.get(5)?;
        let payload = serde_json::from_str(&payload_json).unwrap_or_else(|_| json!({}));
        Ok(json!({
          "id": row.get::<_, String>(0)?,
          "patch_id": row.get::<_, String>(1)?,
          "type": row.get::<_, String>(2)?,
          "status": row.get::<_, String>(3)?,
          "temp_id": row.get::<_, Option<String>>(4)?,
          "payload": payload,
          "created_at": row.get::<_, String>(6)?,
          "decided_at": row.get::<_, Option<String>>(7)?,
          "decision_reason": row.get::<_, Option<String>>(8)?
        }))
      },
    )
    .optional()?
    .ok_or_else(|| CommandError::not_found(format!("Graph proposal not found: {proposal_id}")))?;
  let status = proposal["status"].as_str().unwrap_or("");
  if !["draft", "proposed", "deferred"].contains(&status) {
    return Err(CommandError::validation(format!("Graph proposal is already {status}: {proposal_id}")));
  }
  Ok(proposal)
}

fn reviewable_patch_graph_object_proposals(conn: &Connection, patch_id: &str) -> CommandResult<Vec<Value>> {
  let mut stmt = conn.prepare(
    r#"
        SELECT id, patch_id, proposal_type, status, temp_id, payload_json, created_at, decided_at, decision_reason
        FROM graph_proposals
        WHERE patch_id = ?1
          AND status IN ('draft', 'proposed', 'deferred')
          AND proposal_type IN ('node', 'node_body_update', 'edge', 'edge_bridge_update', 'message_evidence_attachment')
        ORDER BY
          CASE proposal_type
            WHEN 'node' THEN 0
            WHEN 'node_body_update' THEN 1
            WHEN 'edge' THEN 2
            WHEN 'edge_bridge_update' THEN 3
            WHEN 'message_evidence_attachment' THEN 4
            ELSE 9
          END,
          created_at,
          id
        "#,
  )?;
  let rows = stmt.query_map(params![patch_id], |row| {
    let payload_json: String = row.get(5)?;
    let payload = serde_json::from_str(&payload_json).unwrap_or_else(|_| json!({}));
    Ok(json!({
      "id": row.get::<_, String>(0)?,
      "patch_id": row.get::<_, String>(1)?,
      "type": row.get::<_, String>(2)?,
      "status": row.get::<_, String>(3)?,
      "temp_id": row.get::<_, Option<String>>(4)?,
      "payload": payload,
      "created_at": row.get::<_, String>(6)?,
      "decided_at": row.get::<_, Option<String>>(7)?,
      "decision_reason": row.get::<_, Option<String>>(8)?
    }))
  })?;
  rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
}

fn require_graph_thread_message(conn: &Connection, message_id: &str) -> CommandResult<Value> {
  conn
    .query_row(
      "SELECT id, role, content, created_at FROM graph_thread_messages WHERE id = ?1",
      params![message_id],
      |row| {
        Ok(json!({
          "id": row.get::<_, String>(0)?,
          "role": row.get::<_, String>(1)?,
          "content": row.get::<_, String>(2)?,
          "created_at": row.get::<_, String>(3)?
        }))
      },
    )
    .optional()?
    .ok_or_else(|| CommandError::not_found(format!("Graph thread message not found: {message_id}")))
}

fn graph_thread_message_exists(conn: &Connection, message_id: &str) -> CommandResult<bool> {
  let exists: Option<String> = conn
    .query_row("SELECT id FROM graph_thread_messages WHERE id = ?1", params![message_id], |row| row.get(0))
    .optional()?;
  Ok(exists.is_some())
}

fn require_node_thread_message(conn: &Connection, message_id: &str) -> CommandResult<Value> {
  conn
    .query_row(
      "SELECT id, node_id, role, content, created_at FROM node_thread_messages WHERE id = ?1",
      params![message_id],
      |row| {
        Ok(json!({
          "id": row.get::<_, String>(0)?,
          "node_id": row.get::<_, String>(1)?,
          "role": row.get::<_, String>(2)?,
          "content": row.get::<_, String>(3)?,
          "created_at": row.get::<_, String>(4)?
        }))
      },
    )
    .optional()?
    .ok_or_else(|| CommandError::not_found(format!("Node thread message not found: {message_id}")))
}

fn required_payload_string<'a>(payload: &'a Value, field: &str) -> CommandResult<&'a str> {
  payload
    .get(field)
    .and_then(Value::as_str)
    .filter(|value| !value.trim().is_empty())
    .ok_or_else(|| CommandError::validation(format!("{field} is required.")))
}

fn now_string() -> CommandResult<String> {
  Ok(OffsetDateTime::now_utc().format(&Rfc3339)?)
}

fn new_id() -> String {
  Uuid::new_v4().to_string()
}
