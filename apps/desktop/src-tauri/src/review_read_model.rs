use std::collections::HashSet;

use rusqlite::{Connection, Row};
use serde_json::{json, Map, Value};

use crate::contracts::{edge_source_ref, edge_target_ref, source_chunk_ids};
use crate::error::CommandResult;

const GROUP_STATUSES: [&str; 5] = ["draft", "proposed", "deferred", "superseded", "rejected"];
pub(crate) const TERMINAL_REVIEW_HISTORY_LIMIT: usize = 100;

pub(crate) fn load_review_queue_read_model(
  conn: &Connection,
  latest_undoable_patch: Option<Value>,
  generated_at: &str,
) -> CommandResult<Value> {
  let proposals = list_graph_review_proposals(conn)?;
  Ok(build_review_queue_read_model(&proposals, latest_undoable_patch, generated_at))
}

fn build_review_queue_read_model(
  proposals: &[Value],
  latest_undoable_patch: Option<Value>,
  generated_at: &str,
) -> Value {
  let items: Vec<Value> = proposals.iter().map(summarize_proposal).collect();
  let mut counts = serde_json::Map::new();
  let mut groups = serde_json::Map::new();

  for status in GROUP_STATUSES {
    groups.insert(
      status.to_string(),
      json!({
        "status": status,
        "title": group_title(status),
        "count": 0,
        "items": []
      }),
    );
  }

  for item in &items {
    let status = item.get("status").and_then(Value::as_str).unwrap_or("proposed");
    let next_count = counts.get(status).and_then(Value::as_i64).unwrap_or(0) + 1;
    counts.insert(status.to_string(), json!(next_count));
    if let Some(group) = groups.get_mut(status) {
      if let Some(group_items) = group.get_mut("items").and_then(Value::as_array_mut) {
        group_items.push(item.clone());
      }
    }
  }

  let mut total_count = 0;
  for group in groups.values_mut() {
    let count = group.get("items").and_then(Value::as_array).map(Vec::len).unwrap_or(0);
    total_count += count;
    group["count"] = json!(count);
  }

  json!({
    "generated_at": generated_at,
    "total_count": total_count,
    "counts_by_status": counts,
    "groups": groups,
    "items": items,
    "latest_undoable_patch": latest_undoable_patch
  })
}

fn summarize_proposal(proposal: &Value) -> Value {
  let proposal_type =
    proposal.get("type").or_else(|| proposal.get("proposal_type")).and_then(Value::as_str).unwrap_or("unknown");
  let payload = proposal.get("payload").cloned().unwrap_or_else(|| {
    proposal
      .get("payload_json")
      .and_then(Value::as_str)
      .and_then(|text| serde_json::from_str(text).ok())
      .unwrap_or_else(|| json!({}))
  });
  let shape = proposal_shape(proposal_type, &payload);
  let evidence = evidence_refs(proposal, &payload);
  let status = proposal.get("status").and_then(Value::as_str).unwrap_or("proposed");

  json!({
    "id": proposal.get("id").and_then(Value::as_str).or_else(|| shape.get("id").and_then(Value::as_str)),
    "patch_id": proposal.get("patch_id").and_then(Value::as_str),
    "job_id": proposal.get("job_id").and_then(Value::as_str),
    "source_message_id": proposal.get("source_message_id").and_then(Value::as_str),
    "type": proposal_type,
    "status": status,
    "temp_id": proposal.get("temp_id").and_then(Value::as_str).or_else(|| shape.get("id").and_then(Value::as_str)),
    "title": shape["title"].clone(),
    "target": shape["target"].clone(),
    "reason": shape["reason"].clone(),
    "mutation_payload": review_mutation_payload(proposal_type, &payload),
    "related_node_ids": related_node_ids(proposal_type, &payload),
    "evidence_count": evidence.len(),
    "evidence_refs": evidence,
    "risk_markers": risk_markers(proposal_type, status, &payload, &evidence),
    "source": proposal_source(proposal),
    "created_at": proposal.get("created_at").and_then(Value::as_str),
    "decided_at": proposal.get("decided_at").and_then(Value::as_str),
    "decision_reason": proposal.get("decision_reason").and_then(Value::as_str)
  })
}

fn list_graph_review_proposals(conn: &Connection) -> CommandResult<Vec<Value>> {
  let mut stmt = conn.prepare(
    r#"
    WITH terminal_history AS (
      SELECT id
      FROM (
        SELECT
          id,
          ROW_NUMBER() OVER (
            PARTITION BY status
            ORDER BY COALESCE(decided_at, created_at) DESC, created_at DESC, id DESC
          ) AS history_rank
        FROM graph_proposals
        WHERE status IN ('accepted', 'rejected', 'superseded')
      )
      WHERE history_rank <= ?1
    )
    SELECT
      graph_proposals.id,
      graph_proposals.patch_id,
      graph_patches.job_id,
      graph_patches.source_message_id,
      graph_patches.source,
      graph_proposals.proposal_type,
      graph_proposals.status,
      graph_proposals.temp_id,
      json_extract(graph_proposals.payload_json, '$.temp_id'),
      json_extract(graph_proposals.payload_json, '$.id'),
      json_extract(graph_proposals.payload_json, '$.title'),
      json_extract(graph_proposals.payload_json, '$.type'),
      json_extract(graph_proposals.payload_json, '$.edge_type'),
      json_extract(graph_proposals.payload_json, '$.reason'),
      json_extract(graph_proposals.payload_json, '$.bridge_text'),
      json_extract(graph_proposals.payload_json, '$.update_kind'),
      json_extract(graph_proposals.payload_json, '$.target_node_id'),
      json_extract(graph_proposals.payload_json, '$.node_id'),
      json_extract(graph_proposals.payload_json, '$.target_edge_id'),
      json_extract(graph_proposals.payload_json, '$.edge_id'),
      json_extract(graph_proposals.payload_json, '$.target_entity_type'),
      json_extract(graph_proposals.payload_json, '$.target_entity_id'),
      json_extract(graph_proposals.payload_json, '$.target'),
      json_extract(graph_proposals.payload_json, '$.message'),
      json_extract(graph_proposals.payload_json, '$.prompt'),
      json_extract(graph_proposals.payload_json, '$.kind'),
      json_extract(graph_proposals.payload_json, '$.source_node_id'),
      json_extract(graph_proposals.payload_json, '$.source_temp_id'),
      json_extract(graph_proposals.payload_json, '$.source_node_ref'),
      json_extract(graph_proposals.payload_json, '$.source'),
      json_extract(graph_proposals.payload_json, '$.target_temp_id'),
      json_extract(graph_proposals.payload_json, '$.target_node_ref'),
      json_extract(graph_proposals.payload_json, '$.node_ids'),
      json_extract(graph_proposals.payload_json, '$.candidate_node_ids'),
      json_extract(graph_proposals.payload_json, '$.candidate_edge_ids'),
      json_extract(graph_proposals.payload_json, '$.candidate_node_refs'),
      json_extract(graph_proposals.payload_json, '$.node_refs'),
      COALESCE(
        json_extract(graph_proposals.payload_json, '$.source_chunk_ids'),
        json_extract(graph_proposals.payload_json, '$.sourceChunkIds')
      ),
      COALESCE(
        json_extract(graph_proposals.payload_json, '$.source_message_ids'),
        json_extract(graph_proposals.payload_json, '$.sourceMessageIds')
      ),
      json_extract(graph_proposals.payload_json, '$.message_id'),
      json_extract(graph_proposals.payload_json, '$.compiled_body'),
      json_extract(graph_proposals.payload_json, '$.section_text'),
      graph_proposals.accepted_entity_type,
      graph_proposals.accepted_entity_id,
      graph_proposals.created_at,
      graph_proposals.decided_at,
      graph_proposals.decision_reason
    FROM graph_proposals
    JOIN graph_patches ON graph_proposals.patch_id = graph_patches.id
    WHERE graph_proposals.status IN ('draft', 'proposed', 'deferred')
       OR graph_proposals.id IN (SELECT id FROM terminal_history)
    ORDER BY graph_proposals.created_at, graph_proposals.id
    "#,
  )?;
  let rows = stmt.query_map([TERMINAL_REVIEW_HISTORY_LIMIT as i64], |row| {
    Ok(json!({
      "id": row.get::<_, String>(0)?,
      "patch_id": row.get::<_, String>(1)?,
      "job_id": row.get::<_, Option<String>>(2)?,
      "source_message_id": row.get::<_, Option<String>>(3)?,
      "source": row.get::<_, String>(4)?,
      "type": row.get::<_, String>(5)?,
      "status": row.get::<_, String>(6)?,
      "temp_id": row.get::<_, Option<String>>(7)?,
      "payload": review_proposal_payload_summary(row)?,
      "accepted_entity_type": row.get::<_, Option<String>>(42)?,
      "accepted_entity_id": row.get::<_, Option<String>>(43)?,
      "created_at": row.get::<_, String>(44)?,
      "decided_at": row.get::<_, Option<String>>(45)?,
      "decision_reason": row.get::<_, Option<String>>(46)?
    }))
  })?;
  rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
}

fn review_proposal_payload_summary(row: &Row<'_>) -> rusqlite::Result<Value> {
  let mut payload = Map::new();
  for (index, key) in [
    (8, "temp_id"),
    (9, "id"),
    (10, "title"),
    (11, "type"),
    (12, "edge_type"),
    (13, "reason"),
    (15, "update_kind"),
    (16, "target_node_id"),
    (17, "node_id"),
    (18, "target_edge_id"),
    (19, "edge_id"),
    (20, "target_entity_type"),
    (21, "target_entity_id"),
    (22, "target"),
    (23, "message"),
    (24, "prompt"),
    (25, "kind"),
    (26, "source_node_id"),
    (27, "source_temp_id"),
    (28, "source_node_ref"),
    (29, "source"),
    (30, "target_temp_id"),
    (31, "target_node_ref"),
    (39, "message_id"),
  ] {
    insert_optional_string(&mut payload, key, row.get(index)?);
  }
  for (index, key) in [(14, "bridge_text"), (40, "compiled_body"), (41, "section_text")] {
    insert_optional_exact_string(&mut payload, key, row.get(index)?);
  }
  for (index, key) in [
    (32, "node_ids"),
    (33, "candidate_node_ids"),
    (34, "candidate_edge_ids"),
    (35, "candidate_node_refs"),
    (36, "node_refs"),
    (37, "source_chunk_ids"),
    (38, "source_message_ids"),
  ] {
    insert_optional_json_array(&mut payload, key, row.get(index)?);
  }
  Ok(Value::Object(payload))
}

fn insert_optional_string(payload: &mut Map<String, Value>, key: &str, value: Option<String>) {
  let Some(value) = value.map(|value| value.trim().to_string()).filter(|value| !value.is_empty()) else {
    return;
  };
  payload.insert(key.to_string(), json!(value));
}

fn insert_optional_exact_string(payload: &mut Map<String, Value>, key: &str, value: Option<String>) {
  let Some(value) = value.filter(|value| !value.is_empty()) else {
    return;
  };
  payload.insert(key.to_string(), json!(value));
}

fn insert_optional_json_array(payload: &mut Map<String, Value>, key: &str, value: Option<String>) {
  let Some(value) = value else {
    return;
  };
  let Ok(value) = serde_json::from_str::<Value>(&value) else {
    return;
  };
  if value.as_array().is_some_and(|items| !items.is_empty()) {
    payload.insert(key.to_string(), value);
  }
}

fn proposal_shape(proposal_type: &str, payload: &Value) -> Value {
  match proposal_type {
    "node" => json!({
      "id": payload.get("temp_id").or_else(|| payload.get("id")).and_then(Value::as_str),
      "title": payload.get("title").and_then(Value::as_str).unwrap_or("New node"),
      "target": payload.get("temp_id").or_else(|| payload.get("title")).and_then(Value::as_str).unwrap_or("new node"),
      "reason": payload.get("reason").and_then(Value::as_str).unwrap_or("Create node")
    }),
    "edge" => {
      let source = edge_source_ref(payload).unwrap_or_else(|| "source".to_string());
      let target = edge_target_ref(payload).unwrap_or_else(|| "target".to_string());
      let edge_type =
        payload.get("type").or_else(|| payload.get("edge_type")).and_then(Value::as_str).unwrap_or("edge");
      json!({
        "id": payload.get("temp_id").or_else(|| payload.get("id")).and_then(Value::as_str),
        "title": format!("{edge_type} edge"),
        "target": format!("{source} -> {target}"),
        "reason": payload
          .get("reason")
          .or_else(|| payload.get("bridge_text"))
          .and_then(Value::as_str)
          .unwrap_or("Create edge")
      })
    }
    "node_body_update" => json!({
      "id": payload.get("id").and_then(Value::as_str),
      "title": node_body_update_title(payload.get("update_kind").and_then(Value::as_str)),
      "target": payload
        .get("target_node_id")
        .or_else(|| payload.get("node_id"))
        .and_then(Value::as_str)
        .unwrap_or("node"),
      "reason": payload.get("reason").and_then(Value::as_str).unwrap_or("Update node body")
    }),
    "edge_bridge_update" => json!({
      "id": payload.get("id").and_then(Value::as_str),
      "title": "Update edge bridge",
      "target": payload
        .get("target_edge_id")
        .or_else(|| payload.get("edge_id"))
        .and_then(Value::as_str)
        .unwrap_or("edge"),
      "reason": payload
        .get("reason")
        .or_else(|| payload.get("bridge_text"))
        .and_then(Value::as_str)
        .unwrap_or("Update bridge text")
    }),
    "message_evidence_attachment" => json!({
      "id": payload.get("id").and_then(Value::as_str),
      "title": "Attach message evidence",
      "target": format!(
        "{} {}",
        payload.get("target_entity_type").and_then(Value::as_str).unwrap_or("entity"),
        payload.get("target_entity_id").and_then(Value::as_str).unwrap_or("")
      ).trim().to_string(),
      "reason": payload.get("reason").and_then(Value::as_str).unwrap_or("Attach graph message as evidence")
    }),
    "path" => json!({
      "id": payload.get("id").and_then(Value::as_str),
      "title": payload
        .get("title")
        .and_then(Value::as_str)
        .map(|title| format!("Path: {title}"))
        .unwrap_or_else(|| "Create path".to_string()),
      "target": format!("{} nodes", payload.get("node_ids").and_then(Value::as_array).map(Vec::len).unwrap_or(0)),
      "reason": payload.get("reason").and_then(Value::as_str).unwrap_or("Create saved path")
    }),
    "ambiguity" => json!({
      "id": payload.get("id").and_then(Value::as_str),
      "title": format!("Ambiguity: {}", format_label(payload.get("kind").and_then(Value::as_str).unwrap_or("review"))),
      "target": ambiguity_target(payload),
      "reason": payload.get("prompt").and_then(Value::as_str).unwrap_or("Needs review")
    }),
    "merge_candidate" => json!({
      "id": payload.get("id").and_then(Value::as_str),
      "title": "Merge candidate",
      "target": string_array(
        payload
          .get("candidate_node_ids")
          .or_else(|| payload.get("candidate_node_refs"))
          .or_else(|| payload.get("node_refs"))
      )
      .join(" + "),
      "reason": payload.get("reason").and_then(Value::as_str).unwrap_or("Review possible node merge")
    }),
    "warning" => json!({
      "id": payload.get("id").and_then(Value::as_str),
      "title": payload.get("title").and_then(Value::as_str).unwrap_or("Patch warning"),
      "target": payload.get("target").and_then(Value::as_str).unwrap_or("patch"),
      "reason": payload
        .get("message")
        .or_else(|| payload.get("reason"))
        .and_then(Value::as_str)
        .unwrap_or("Review warning")
    }),
    _ => json!({
      "id": payload.get("id").and_then(Value::as_str),
      "title": format_label(proposal_type),
      "target": payload.get("target").and_then(Value::as_str).unwrap_or("proposal"),
      "reason": payload.get("reason").and_then(Value::as_str).unwrap_or("Review proposal")
    }),
  }
}

fn review_mutation_payload(proposal_type: &str, payload: &Value) -> Value {
  match proposal_type {
    "node" => mutation_field_payload(payload, "compiled_body"),
    "node_body_update" if payload.get("update_kind").and_then(Value::as_str) == Some("append_section") => {
      mutation_field_payload(payload, "section_text")
    }
    "node_body_update" => mutation_field_payload(payload, "compiled_body"),
    "edge" | "edge_bridge_update" => mutation_field_payload(payload, "bridge_text"),
    _ => Value::Null,
  }
}

fn mutation_field_payload(payload: &Value, field: &str) -> Value {
  let Some(value) = payload.get(field).and_then(Value::as_str).filter(|value| !value.is_empty()) else {
    return Value::Null;
  };
  let mut mutation = Map::new();
  mutation.insert(field.to_string(), json!(value));
  Value::Object(mutation)
}

fn evidence_refs(proposal: &Value, payload: &Value) -> Vec<Value> {
  let mut refs = Vec::new();
  for id in source_chunk_ids(payload) {
    refs.push(json!({ "type": "chunk", "id": id }));
  }
  for id in string_array(payload.get("source_message_ids")) {
    refs.push(json!({ "type": "message", "id": id }));
  }
  if let Some(id) = payload.get("message_id").and_then(Value::as_str) {
    refs.push(json!({ "type": "message", "id": id }));
  }
  if let Some(id) = proposal.get("source_message_id").and_then(Value::as_str) {
    refs.push(json!({ "type": "message", "id": id }));
  }
  unique_refs(refs)
}

fn risk_markers(proposal_type: &str, status: &str, payload: &Value, evidence: &[Value]) -> Vec<String> {
  let mut markers = HashSet::new();
  if status == "draft" {
    markers.insert("draft".to_string());
  }
  if status == "proposed" {
    markers.insert("needs_review".to_string());
  }
  if ["deferred", "superseded", "rejected"].contains(&status) {
    markers.insert(status.to_string());
  }
  if evidence.iter().any(|item| item.get("type").and_then(Value::as_str) == Some("chunk")) {
    markers.insert("source_backed".to_string());
  }
  if evidence.iter().any(|item| item.get("type").and_then(Value::as_str) == Some("message")) {
    markers.insert("message_backed".to_string());
  }
  if proposal_type == "ambiguity" {
    markers.insert("ambiguity".to_string());
    if let Some(kind) = payload.get("kind").and_then(Value::as_str) {
      markers.insert(kind.to_string());
    }
  }
  if proposal_type == "merge_candidate" {
    markers.insert("merge_risk".to_string());
  }
  if proposal_type == "warning" {
    markers.insert("warning".to_string());
  }
  if proposal_type == "message_evidence_attachment" {
    markers.insert("message_evidence".to_string());
  }
  if proposal_type == "node_body_update" {
    markers.insert(if payload.get("update_kind").and_then(Value::as_str) == Some("replace_body") {
      "body_rewrite".to_string()
    } else {
      "body_update".to_string()
    });
  }
  if ["node", "edge", "path"].contains(&proposal_type) {
    markers.insert("new_graph_object".to_string());
  }
  if evidence.is_empty() && !["ambiguity", "path", "warning"].contains(&proposal_type) {
    markers.insert("no_evidence".to_string());
  }
  let mut markers: Vec<String> = markers.into_iter().collect();
  markers.sort();
  markers
}

fn related_node_ids(proposal_type: &str, payload: &Value) -> Vec<String> {
  let mut ids = Vec::new();
  match proposal_type {
    "node_body_update" => {
      push_string(&mut ids, payload.get("target_node_id"));
      push_string(&mut ids, payload.get("node_id"));
    }
    "edge" => {
      push_string(&mut ids, payload.get("source_node_id"));
      push_string(&mut ids, payload.get("target_node_id"));
      push_string(&mut ids, payload.get("source_temp_id"));
      push_string(&mut ids, payload.get("target_temp_id"));
    }
    "message_evidence_attachment" if payload.get("target_entity_type").and_then(Value::as_str) == Some("node") => {
      push_string(&mut ids, payload.get("target_entity_id"));
    }
    "path" => ids.extend(string_array(payload.get("node_ids"))),
    "ambiguity" => ids.extend(string_array(payload.get("candidate_node_ids"))),
    "merge_candidate" => {
      ids.extend(string_array(
        payload
          .get("candidate_node_ids")
          .or_else(|| payload.get("candidate_node_refs"))
          .or_else(|| payload.get("node_refs")),
      ));
    }
    _ => {}
  }
  unique_strings(ids)
}

fn proposal_source(proposal: &Value) -> Value {
  if let Some(message_id) = proposal.get("source_message_id").and_then(Value::as_str) {
    let is_node_message =
      matches!(proposal.get("source").and_then(Value::as_str), Some("node_chat_update_job" | "node_thread_message"));
    return json!({
      "kind": if is_node_message { "node_message" } else { "graph_message" },
      "id": message_id,
      "source_message_id": message_id,
      "job_id": null,
      "label": format!(
        "{} {}",
        if is_node_message { "Node chat message" } else { "Graph message" },
        short_id(message_id)
      )
    });
  }
  if let Some(job_id) = proposal.get("job_id").and_then(Value::as_str) {
    return json!({
      "kind": "job",
      "id": job_id,
      "source_message_id": null,
      "job_id": job_id,
      "label": format!("Job {job_id}")
    });
  }
  let patch_id = proposal.get("patch_id").and_then(Value::as_str);
  json!({
    "kind": "patch",
    "id": patch_id,
    "source_message_id": null,
    "job_id": null,
    "label": patch_id.map(|id| format!("Patch {}", short_id(id))).unwrap_or_else(|| "Patch".to_string())
  })
}

fn node_body_update_title(kind: Option<&str>) -> &'static str {
  match kind {
    Some("append_section") => "Append node section",
    Some("replace_body") => "Replace node body",
    _ => "Update node body",
  }
}

fn group_title(status: &str) -> &'static str {
  match status {
    "draft" => "Draft",
    "proposed" => "Needs review",
    "deferred" => "Deferred",
    "superseded" => "Superseded",
    "rejected" => "Rejected",
    _ => "Review",
  }
}

fn ambiguity_target(payload: &Value) -> String {
  let nodes = string_array(payload.get("candidate_node_ids"));
  if !nodes.is_empty() {
    return nodes.join(", ");
  }
  let edges = string_array(payload.get("candidate_edge_ids"));
  if !edges.is_empty() {
    return edges.join(", ");
  }
  "graph".to_string()
}

fn string_array(value: Option<&Value>) -> Vec<String> {
  value
    .and_then(Value::as_array)
    .into_iter()
    .flatten()
    .filter_map(Value::as_str)
    .filter(|value| !value.trim().is_empty())
    .map(String::from)
    .collect()
}

fn push_string(items: &mut Vec<String>, value: Option<&Value>) {
  if let Some(value) = value.and_then(Value::as_str).filter(|value| !value.trim().is_empty()) {
    items.push(value.to_string());
  }
}

fn unique_strings(items: Vec<String>) -> Vec<String> {
  let mut seen = HashSet::new();
  items.into_iter().filter(|item| seen.insert(item.clone())).collect()
}

fn unique_refs(refs: Vec<Value>) -> Vec<Value> {
  let mut seen = HashSet::new();
  refs
    .into_iter()
    .filter(|item| {
      let key = format!(
        "{}:{}",
        item.get("type").and_then(Value::as_str).unwrap_or(""),
        item.get("id").and_then(Value::as_str).unwrap_or("")
      );
      seen.insert(key)
    })
    .collect()
}

fn format_label(value: &str) -> String {
  value.replace('_', " ")
}

fn short_id(value: &str) -> String {
  value.chars().take(8).collect()
}

#[cfg(test)]
mod tests {
  use std::path::{Path, PathBuf};

  use rusqlite::params;

  use super::*;
  use crate::database::open_database;

  const BASE_TIME: &str = "2026-01-01T00:00:00Z";
  const EXTRA_HISTORY_ROWS: usize = 5;

  #[test]
  fn active_review_items_are_not_history_limited() {
    let (mut conn, path) = test_database();
    let transaction = conn.transaction().unwrap();
    for status in ["draft", "proposed", "deferred"] {
      let patch_id = format!("patch_{status}");
      insert_patch(&transaction, &patch_id, None, BASE_TIME);
      for index in 0..TERMINAL_REVIEW_HISTORY_LIMIT + EXTRA_HISTORY_ROWS {
        let id = format!("{status}_{index:03}");
        insert_proposal(&transaction, &id, &patch_id, status, BASE_TIME, None);
      }
    }
    transaction.commit().unwrap();

    let read_model = load_review_queue_read_model(&conn, None, BASE_TIME).unwrap();
    for status in ["draft", "proposed", "deferred"] {
      assert_eq!(items_with_status(&read_model, status).len(), TERMINAL_REVIEW_HISTORY_LIMIT + EXTRA_HISTORY_ROWS);
    }
    assert_eq!(
      read_model["total_count"].as_u64().unwrap() as usize,
      3 * (TERMINAL_REVIEW_HISTORY_LIMIT + EXTRA_HISTORY_ROWS)
    );
    close_test_database(conn, &path);
  }

  #[test]
  fn terminal_history_is_capped_per_status_with_deterministic_membership_and_order() {
    let (mut conn, path) = test_database();
    let transaction = conn.transaction().unwrap();
    for status in ["accepted", "rejected", "superseded"] {
      let patch_id = format!("patch_{status}");
      insert_patch(&transaction, &patch_id, None, BASE_TIME);
      for index in 0..TERMINAL_REVIEW_HISTORY_LIMIT + EXTRA_HISTORY_ROWS {
        let id = format!("{status}_{index:03}");
        let timestamp = if status == "rejected" { BASE_TIME.to_string() } else { timestamp(index) };
        insert_proposal(&transaction, &id, &patch_id, status, &timestamp, Some(&timestamp));
      }
    }
    transaction.commit().unwrap();

    let read_model = load_review_queue_read_model(&conn, None, BASE_TIME).unwrap();
    for status in ["accepted", "rejected", "superseded"] {
      let items = items_with_status(&read_model, status);
      let ids = items.iter().map(|item| item["id"].as_str().unwrap()).collect::<Vec<_>>();
      assert_eq!(ids.len(), TERMINAL_REVIEW_HISTORY_LIMIT);
      assert_eq!(ids.first().copied(), Some(format!("{status}_005").as_str()));
      assert_eq!(ids.last().copied(), Some(format!("{status}_104").as_str()));
      assert!(ids.windows(2).all(|pair| pair[0] < pair[1]));
      assert_eq!(read_model["counts_by_status"][status].as_u64().unwrap() as usize, TERMINAL_REVIEW_HISTORY_LIMIT);
    }
    close_test_database(conn, &path);
  }

  #[test]
  fn accepted_chat_patch_remains_visible_inside_bounded_history() {
    let (mut conn, path) = test_database();
    let transaction = conn.transaction().unwrap();
    insert_patch(&transaction, "patch_history", None, BASE_TIME);
    for index in 0..TERMINAL_REVIEW_HISTORY_LIMIT {
      let id = format!("accepted_history_{index:03}");
      let timestamp = timestamp(index);
      insert_proposal(&transaction, &id, "patch_history", "accepted", &timestamp, Some(&timestamp));
    }

    let chat_time = timestamp(TERMINAL_REVIEW_HISTORY_LIMIT + 1);
    insert_patch(&transaction, "patch_chat", Some("graph_message_chat"), &chat_time);
    for id in ["accepted_chat_node", "accepted_chat_edge"] {
      insert_proposal(&transaction, id, "patch_chat", "accepted", &chat_time, Some(&chat_time));
    }
    transaction.commit().unwrap();

    let read_model = load_review_queue_read_model(&conn, None, BASE_TIME).unwrap();
    let accepted = items_with_status(&read_model, "accepted");
    let chat_items =
      accepted.iter().filter(|item| item["source_message_id"] == "graph_message_chat").collect::<Vec<_>>();
    assert_eq!(accepted.len(), TERMINAL_REVIEW_HISTORY_LIMIT);
    assert_eq!(chat_items.len(), 2);
    assert!(chat_items.iter().all(|item| item["patch_id"] == "patch_chat"));
    close_test_database(conn, &path);
  }

  #[test]
  fn review_items_expose_only_the_exact_type_specific_mutation_payload() {
    let (mut conn, path) = test_database();
    let transaction = conn.transaction().unwrap();
    insert_patch(&transaction, "patch_mutations", None, BASE_TIME);
    for (id, proposal_type, payload) in [
      (
        "new_node",
        "node",
        json!({
          "temp_id": "node_exact",
          "title": "Exact node",
          "compiled_body": "\nExact compiled body.\n",
          "reason": "Create the node."
        }),
      ),
      (
        "append_body",
        "node_body_update",
        json!({
          "target_node_id": "node_exact",
          "update_kind": "append_section",
          "section_text": "Exact appended section.",
          "reason": "Append the section."
        }),
      ),
      (
        "replace_body",
        "node_body_update",
        json!({
          "target_node_id": "node_exact",
          "update_kind": "replace_body",
          "compiled_body": "Exact replacement body.",
          "reason": "Replace the body."
        }),
      ),
      (
        "new_edge",
        "edge",
        json!({
          "source_node_id": "node_exact",
          "target_node_id": "node_other",
          "type": "supports",
          "bridge_text": "Exact new bridge.",
          "reason": "Create the edge."
        }),
      ),
      (
        "update_bridge",
        "edge_bridge_update",
        json!({
          "target_edge_id": "edge_exact",
          "bridge_text": "Exact replacement bridge.",
          "reason": "Update the bridge."
        }),
      ),
      (
        "warning",
        "warning",
        json!({
          "title": "Warning",
          "message": "No mutation.",
          "compiled_body": "This unrelated field must not leak."
        }),
      ),
    ] {
      insert_typed_proposal(&transaction, id, "patch_mutations", proposal_type, "proposed", &payload, BASE_TIME, None);
    }
    transaction.commit().unwrap();

    let read_model = load_review_queue_read_model(&conn, None, BASE_TIME).unwrap();
    let item = |id: &str| read_model["items"].as_array().unwrap().iter().find(|item| item["id"] == id).unwrap();
    assert_eq!(item("new_node")["mutation_payload"], json!({ "compiled_body": "\nExact compiled body.\n" }));
    assert_eq!(item("append_body")["mutation_payload"], json!({ "section_text": "Exact appended section." }));
    assert_eq!(item("replace_body")["mutation_payload"], json!({ "compiled_body": "Exact replacement body." }));
    assert_eq!(item("new_edge")["mutation_payload"], json!({ "bridge_text": "Exact new bridge." }));
    assert_eq!(item("update_bridge")["mutation_payload"], json!({ "bridge_text": "Exact replacement bridge." }));
    assert!(item("warning")["mutation_payload"].is_null());
    close_test_database(conn, &path);
  }

  fn test_database() -> (Connection, PathBuf) {
    let path = std::env::temp_dir().join(format!("soma-review-read-model-{}.sqlite", uuid::Uuid::new_v4()));
    (open_database(&path).unwrap(), path)
  }

  fn close_test_database(conn: Connection, path: &Path) {
    drop(conn);
    let _ = std::fs::remove_file(path);
    let _ = std::fs::remove_file(path.with_extension("sqlite-wal"));
    let _ = std::fs::remove_file(path.with_extension("sqlite-shm"));
  }

  fn insert_patch(conn: &Connection, id: &str, source_message_id: Option<&str>, created_at: &str) {
    conn
      .execute(
        "INSERT INTO graph_patches (
              id, source_message_id, source, status, created_at, errors_json
            ) VALUES (?1, ?2, 'review_history_test', 'imported', ?3, '[]')",
        params![id, source_message_id, created_at],
      )
      .unwrap();
  }

  fn insert_proposal(
    conn: &Connection,
    id: &str,
    patch_id: &str,
    status: &str,
    created_at: &str,
    decided_at: Option<&str>,
  ) {
    let payload = json!({ "temp_id": id, "title": id, "reason": "fixture" });
    insert_typed_proposal(conn, id, patch_id, "node", status, &payload, created_at, decided_at);
  }

  #[allow(clippy::too_many_arguments)]
  fn insert_typed_proposal(
    conn: &Connection,
    id: &str,
    patch_id: &str,
    proposal_type: &str,
    status: &str,
    payload: &Value,
    created_at: &str,
    decided_at: Option<&str>,
  ) {
    conn
      .execute(
        "INSERT INTO graph_proposals (
              id, patch_id, proposal_type, status, temp_id, payload_json, created_at, decided_at
            ) VALUES (?1, ?2, ?3, ?4, ?1, ?5, ?6, ?7)",
        params![id, patch_id, proposal_type, status, payload.to_string(), created_at, decided_at],
      )
      .unwrap();
  }

  fn items_with_status<'a>(read_model: &'a Value, status: &str) -> Vec<&'a Value> {
    read_model["items"].as_array().unwrap().iter().filter(|item| item["status"] == status).collect()
  }

  fn timestamp(index: usize) -> String {
    format!("2026-01-{:02}T{:02}:00:00Z", index / 24 + 1, index % 24)
  }
}
