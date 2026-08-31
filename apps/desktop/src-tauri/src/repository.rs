use std::path::Path;

use rusqlite::Connection;
use serde_json::{json, Value};
use time::format_description::well_known::Rfc3339;
use time::OffsetDateTime;
#[cfg(test)]
use uuid::Uuid;

use crate::chat_thread_store;
#[cfg(test)]
use crate::database::open_database;
use crate::database::{open_existing_database, open_existing_database_readonly, with_write_transaction};
use crate::error::{CommandError, CommandResult};
use crate::graph_read_model::{
  active_graph_context_snapshot, active_graph_node_cards_for_ids, active_graph_node_detail,
  active_graph_startup_canvas_snapshot, node_ids_from_values,
};
use crate::graph_write_model;
use crate::layout_state::{list_node_layout_for_nodes, persist_node_position};
use crate::retrieval::{
  build_graph_context_packet_with_reading_context, build_node_context_packet, graph_search_terms,
  GRAPH_CONTEXT_NODE_LIMIT, NODE_CONTEXT_NODE_LIMIT,
};
use crate::retrieval_read_model::{active_graph_context_node_ids, active_node_context_node_ids};
use crate::review_read_model::load_review_queue_read_model;

const GRAPH_NODE_SEARCH_LIMIT_MAX: usize = 20;
const GRAPH_NODE_SEARCH_QUERY_MAX_CHARS: usize = 256;
pub(crate) const CHAT_MESSAGE_MAX_CHARACTERS: usize = 4_000;

pub struct WorkspaceStore {
  conn: Connection,
}

impl WorkspaceStore {
  pub fn open(database_path: impl AsRef<Path>) -> CommandResult<Self> {
    Ok(Self { conn: open_existing_database(database_path)? })
  }

  pub fn open_readonly(database_path: impl AsRef<Path>) -> CommandResult<Self> {
    Ok(Self { conn: open_existing_database_readonly(database_path)? })
  }

  #[cfg(test)]
  pub(crate) fn load_graph_snapshot(&self) -> CommandResult<Value> {
    crate::graph_read_model::active_graph_snapshot(&self.conn)
  }

  pub fn load_graph_canvas_snapshot(&self) -> CommandResult<Value> {
    active_graph_startup_canvas_snapshot(&self.conn)
  }

  pub fn load_workspace_bootstrap(&self) -> CommandResult<Value> {
    let canvas = active_graph_startup_canvas_snapshot(&self.conn)?;
    let node_ids =
      canvas.get("nodes").and_then(Value::as_array).map(|nodes| node_ids_from_values(nodes)).unwrap_or_default();
    Ok(json!({
      "canvas": canvas,
      "layout": list_node_layout_for_nodes(&self.conn, &node_ids)?
    }))
  }

  pub fn load_graph_node_detail(&self, node_id: &str) -> CommandResult<Value> {
    active_graph_node_detail(&self.conn, node_id)
  }

  pub fn search_graph_node_cards(&self, query: &str, limit: usize) -> CommandResult<Value> {
    let query = query.trim();
    if query.is_empty() {
      return Err(CommandError::validation("Search query is required."));
    }
    if query.chars().count() > GRAPH_NODE_SEARCH_QUERY_MAX_CHARS {
      return Err(CommandError::validation(format!(
        "Search query must be at most {GRAPH_NODE_SEARCH_QUERY_MAX_CHARS} characters."
      )));
    }
    if !(1..=GRAPH_NODE_SEARCH_LIMIT_MAX).contains(&limit) {
      return Err(CommandError::validation(format!(
        "Search limit must be between 1 and {GRAPH_NODE_SEARCH_LIMIT_MAX}."
      )));
    }

    let terms = graph_search_terms(query, None);
    if terms.is_empty() {
      return Ok(json!([]));
    }
    let node_ids = active_graph_context_node_ids(&self.conn, &terms, &[], limit)?;
    Ok(json!(active_graph_node_cards_for_ids(&self.conn, &node_ids)?))
  }

  pub fn load_review_queue(&self) -> CommandResult<Value> {
    let latest_undoable_patch = graph_write_model::latest_undoable_graph_patch(&self.conn)?;
    load_review_queue_read_model(&self.conn, latest_undoable_patch, &now_string()?)
  }

  #[cfg(test)]
  pub fn append_graph_message(&mut self, content: &str, focus_node_ids: Vec<String>) -> CommandResult<Value> {
    self.append_graph_message_with_reading_context(content, focus_node_ids, None, true)
  }

  pub fn append_graph_message_with_reading_context(
    &mut self,
    content: &str,
    focus_node_ids: Vec<String>,
    reading_context: Option<Value>,
    graph_capture_enabled: bool,
  ) -> CommandResult<Value> {
    let message_content = validate_chat_message(content, "Graph thread message content is required.")?;
    let message = self.in_transaction(|conn| chat_thread_store::append_graph_user_message(conn, message_content))?;

    let message_id = message.get("id").and_then(Value::as_str).unwrap_or("").to_string();
    let search_terms = graph_search_terms(message_content, reading_context.as_ref());
    let context_node_ids =
      active_graph_context_node_ids(&self.conn, &search_terms, &focus_node_ids, GRAPH_CONTEXT_NODE_LIMIT)?;
    let snapshot = active_graph_context_snapshot(&self.conn, &context_node_ids)?;
    let recent_messages = chat_thread_store::recent_graph_thread_messages(&self.conn, 6)?;
    let context_packet = build_graph_context_packet_with_reading_context(
      &snapshot,
      message_content,
      recent_messages,
      &focus_node_ids,
      reading_context.as_ref(),
      graph_capture_enabled,
    );
    let used_graph_areas = context_packet.get("used_graph_areas").cloned().unwrap_or_else(|| json!([]));

    self.in_transaction(|conn| {
      chat_thread_store::attach_graph_message_context(conn, &message_id, &context_packet)?;

      Ok(json!({
        "message": message,
        "context_packet": context_packet,
        "used_graph_areas": used_graph_areas
      }))
    })
  }

  pub fn list_graph_messages(&self) -> CommandResult<Value> {
    Ok(json!(chat_thread_store::recent_graph_thread_messages(&self.conn, 30)?))
  }

  pub fn append_graph_assistant_message(&mut self, content: &str, context_packet: &Value) -> CommandResult<Value> {
    let message_content = content.trim();
    if message_content.is_empty() {
      return Err(CommandError::validation("Assistant graph message content is required."));
    }
    self.in_transaction(|conn| chat_thread_store::append_graph_assistant_message(conn, message_content, context_packet))
  }

  #[cfg(test)]
  pub fn append_node_message(&mut self, node_id: &str, content: &str) -> CommandResult<Value> {
    self.append_node_message_with_capture(node_id, content, true)
  }

  pub fn append_node_message_with_capture(
    &mut self,
    node_id: &str,
    content: &str,
    graph_capture_enabled: bool,
  ) -> CommandResult<Value> {
    let node_id = node_id.trim();
    if node_id.is_empty() {
      return Err(CommandError::validation("Node id is required."));
    }
    let message_content = validate_chat_message(content, "Node thread message content is required.")?;

    let message = self.in_transaction(|conn| {
      graph_write_model::require_active_node(conn, node_id)?;
      chat_thread_store::append_node_user_message(conn, node_id, message_content)
    })?;

    let message_id = message.get("id").and_then(Value::as_str).unwrap_or("").to_string();
    let context_node_ids = active_node_context_node_ids(&self.conn, node_id, NODE_CONTEXT_NODE_LIMIT)?;
    let snapshot = active_graph_context_snapshot(&self.conn, &context_node_ids)?;
    let recent_messages = chat_thread_store::recent_node_thread_messages(&self.conn, node_id, 6)?;
    let context_packet =
      build_node_context_packet(&snapshot, node_id, message_content, recent_messages, graph_capture_enabled)?;
    let mut message_with_context = message.clone();
    message_with_context["context_packet"] = context_packet.clone();

    self.in_transaction(|conn| {
      chat_thread_store::attach_node_message_context(conn, &message_id, &context_packet)?;

      Ok(json!({
        "message": message_with_context,
        "context_packet": context_packet
      }))
    })
  }

  pub fn list_node_messages(&self, node_id: &str) -> CommandResult<Value> {
    let node_id = node_id.trim();
    if node_id.is_empty() {
      return Err(CommandError::validation("Node id is required."));
    }
    graph_write_model::require_active_node(&self.conn, node_id)?;
    Ok(json!(chat_thread_store::recent_node_thread_messages(&self.conn, node_id, 30)?))
  }

  pub fn append_node_assistant_message(
    &mut self,
    node_id: &str,
    content: &str,
    context_packet: &Value,
  ) -> CommandResult<Value> {
    let node_id = node_id.trim();
    if node_id.is_empty() {
      return Err(CommandError::validation("Node id is required."));
    }
    let message_content = content.trim();
    if message_content.is_empty() {
      return Err(CommandError::validation("Assistant node message content is required."));
    }
    self.in_transaction(|conn| {
      graph_write_model::require_active_node(conn, node_id)?;
      chat_thread_store::append_node_assistant_message(conn, node_id, message_content, context_packet)
    })
  }

  pub fn update_node_body(&mut self, node_id: &str, compiled_body: &str) -> CommandResult<Value> {
    self.in_transaction(|conn| graph_write_model::update_node_body(conn, node_id, compiled_body))
  }

  pub fn rollback_node_body(&mut self, node_id: &str, version_number: i64) -> CommandResult<Value> {
    self.in_transaction(|conn| graph_write_model::rollback_node_body(conn, node_id, version_number))
  }

  pub fn persist_node_position(&mut self, node_id: &str, x: f64, y: f64, pinned: bool) -> CommandResult<Value> {
    let node_id = node_id.trim();
    if node_id.is_empty() {
      return Err(CommandError::validation("Node id is required."));
    }

    self.in_transaction(|conn| {
      graph_write_model::require_active_node(conn, node_id)?;
      let updated_at = now_string()?;
      persist_node_position(conn, node_id, x, y, pinned, &updated_at)
    })
  }

  #[cfg(test)]
  pub fn propose_graph_updates(&mut self, message_id: &str, patch: Value) -> CommandResult<Value> {
    self.in_transaction(|conn| graph_write_model::propose_graph_updates(conn, message_id, patch))
  }

  pub fn propose_graph_updates_with_evidence_message(
    &mut self,
    source_message_id: &str,
    evidence_message_id: &str,
    patch: Value,
  ) -> CommandResult<Value> {
    self.in_transaction(|conn| {
      graph_write_model::propose_graph_updates_with_evidence_message(
        conn,
        source_message_id,
        evidence_message_id,
        patch,
      )
    })
  }

  pub fn propose_node_updates(
    &mut self,
    source_message_id: &str,
    evidence_message_id: &str,
    patch: Value,
  ) -> CommandResult<Value> {
    self.in_transaction(|conn| {
      graph_write_model::propose_node_updates(conn, source_message_id, evidence_message_id, patch)
    })
  }

  pub fn accept_graph_proposal(&mut self, proposal_id: &str, reason: Option<&str>) -> CommandResult<Value> {
    self.in_transaction(|conn| graph_write_model::accept_graph_proposal(conn, proposal_id, reason))
  }

  pub fn accept_graph_patch_proposals(&mut self, patch_id: &str, reason: Option<&str>) -> CommandResult<Value> {
    self.in_transaction(|conn| graph_write_model::accept_graph_patch_proposals(conn, patch_id, reason))
  }

  pub fn undo_graph_patch(&mut self, patch_id: &str) -> CommandResult<Value> {
    self.in_transaction(|conn| graph_write_model::undo_graph_patch(conn, patch_id))
  }

  pub fn reject_graph_proposal(&mut self, proposal_id: &str, reason: Option<&str>) -> CommandResult<Value> {
    self.set_graph_proposal_lifecycle_status(proposal_id, "rejected", reason)
  }

  pub fn defer_graph_proposal(&mut self, proposal_id: &str, reason: Option<&str>) -> CommandResult<Value> {
    self.set_graph_proposal_lifecycle_status(proposal_id, "deferred", reason)
  }

  fn set_graph_proposal_lifecycle_status(
    &mut self,
    proposal_id: &str,
    status: &str,
    reason: Option<&str>,
  ) -> CommandResult<Value> {
    self
      .in_transaction(|conn| graph_write_model::set_graph_proposal_lifecycle_status(conn, proposal_id, status, reason))
  }

  fn in_transaction(&mut self, action: impl FnOnce(&Connection) -> CommandResult<Value>) -> CommandResult<Value> {
    with_write_transaction(&self.conn, action)
  }
}

fn validate_chat_message<'a>(content: &'a str, required_message: &str) -> CommandResult<&'a str> {
  let content = content.trim();
  if content.is_empty() {
    return Err(CommandError::validation(required_message));
  }
  if content.chars().count() > CHAT_MESSAGE_MAX_CHARACTERS {
    return Err(CommandError::validation(format!(
      "Chat messages are limited to {CHAT_MESSAGE_MAX_CHARACTERS} characters."
    )));
  }
  Ok(content)
}

fn now_string() -> CommandResult<String> {
  Ok(OffsetDateTime::now_utc().format(&Rfc3339)?)
}

#[cfg(test)]
fn new_id() -> String {
  Uuid::new_v4().to_string()
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::contracts::{empty_graph_patch, NODE_BODY_MAX_CHARS};
  use rusqlite::params;

  #[test]
  fn rejects_overlong_graph_and_node_messages_at_the_store_boundary() {
    let mut store = test_store();
    let maximum = "🧠".repeat(CHAT_MESSAGE_MAX_CHARACTERS);
    let over_limit = format!("{maximum}x");

    assert_eq!(
      validate_chat_message(&format!("  {maximum}  "), "required").unwrap().chars().count(),
      CHAT_MESSAGE_MAX_CHARACTERS
    );
    for error in [
      store.append_graph_message(&over_limit, Vec::new()).unwrap_err(),
      store.append_node_message("missing-node", &over_limit).unwrap_err(),
    ] {
      assert_eq!(error.code, "Soma_VALIDATION_ERROR");
      assert_eq!(error.message, format!("Chat messages are limited to {CHAT_MESSAGE_MAX_CHARACTERS} characters."));
    }
    assert!(store.list_graph_messages().unwrap().as_array().unwrap().is_empty());
  }

  #[test]
  fn write_transaction_waits_for_active_sqlite_writer() {
    let path = std::env::temp_dir().join(format!("soma-lock-wait-test-{}.sqlite", new_id()));
    let blocker = open_database(&path).unwrap();
    blocker.execute_batch("BEGIN IMMEDIATE").unwrap();

    let write_path = path.clone();
    let (started_tx, started_rx) = std::sync::mpsc::channel();
    let writer = std::thread::spawn(move || {
      started_tx.send(()).unwrap();
      let mut store = WorkspaceStore::open(&write_path).unwrap();
      store.append_graph_message("This write should wait for the active writer.", Vec::new()).unwrap()
    });

    started_rx.recv().unwrap();
    std::thread::sleep(std::time::Duration::from_millis(100));
    blocker.execute_batch("COMMIT").unwrap();

    let message = writer.join().unwrap();
    assert_eq!(message["message"]["content"], "This write should wait for the active writer.");
    remove_sqlite_file_set(&path);
  }

  #[test]
  fn appends_message_persists_patch_and_accepts_node_into_snapshot() {
    let mut store = test_store();
    seed_chunk(&store.conn, "chunk_1");

    let message = store.append_graph_message("connectedness slider", Vec::new()).unwrap();
    let message_id = message["message"]["id"].as_str().unwrap();
    let patch = node_patch("node_slider", "Connectedness Slider", "A compiled section about graph density.", "chunk_1");

    let proposed = store.propose_graph_updates(message_id, patch).unwrap();
    assert_eq!(proposed["valid"], true);
    assert_eq!(proposed["proposal_status"], "proposed");

    let queue = store.load_review_queue().unwrap();
    assert_eq!(queue["total_count"], 1);
    let proposal_id = queue["groups"]["proposed"]["items"][0]["id"].as_str().unwrap().to_string();

    let accepted = store.accept_graph_proposal(&proposal_id, None).unwrap();
    assert_eq!(accepted["status"], "accepted");

    let snapshot = store.load_graph_snapshot().unwrap();
    assert_eq!(snapshot["nodes"].as_array().unwrap().len(), 1);
    assert_eq!(snapshot["nodes"][0]["title"], "Connectedness Slider");
    assert_eq!(snapshot["nodes"][0]["markers"][0], "source_backed");
  }

  #[test]
  fn accepting_imported_message_refs_resolves_them_to_canonical_chunks() {
    let mut store = test_store();
    seed_chunk(&store.conn, "chunk_1");

    let message = store.append_graph_message("Compile the imported source message.", Vec::new()).unwrap();
    let message_id = message["message"]["id"].as_str().unwrap();
    let mut patch = empty_graph_patch();
    patch["proposed_nodes"] = json!([{
      "temp_id": "node_imported_message",
      "type": "concept",
      "title": "Imported Message Evidence",
      "compiled_body": "Imported source message references resolve to their stored chunks.",
      "source_message_ids": ["message_1"],
      "reason": "The compile patch cited an imported source message."
    }]);

    store.propose_graph_updates(message_id, patch).unwrap();
    let proposal_id =
      store.load_review_queue().unwrap()["groups"]["proposed"]["items"][0]["id"].as_str().unwrap().to_string();
    store.accept_graph_proposal(&proposal_id, None).unwrap();

    let snapshot = store.load_graph_snapshot().unwrap();
    assert_eq!(snapshot["nodes"][0]["source_chunk_ids"], json!(["chunk_1"]));
    assert_eq!(snapshot["nodes"][0]["evidence"][0]["message_id"], "message_1");
  }

  #[test]
  fn search_graph_node_cards_reaches_nodes_outside_startup_canvas() {
    let store = test_store();
    let now = now_string().unwrap();
    for index in 0..165 {
      let node_id = format!("search_node_{index:03}");
      let body_id = format!("{node_id}_body");
      let preview = if index == 164 { "global tailneedle" } else { "ordinary node" };
      store
        .conn
        .execute(
          concat!(
            "INSERT INTO graph_nodes ",
            "(id, node_type, title, preview, current_body_version_id, status, created_at, updated_at) ",
            "VALUES (?1, 'concept', ?2, ?3, ?4, 'active', ?5, ?5)"
          ),
          params![node_id, format!("Search Node {index:03}"), preview, body_id, now],
        )
        .unwrap();
      store
        .conn
        .execute(
          concat!(
            "INSERT INTO node_body_versions ",
            "(id, node_id, version_number, compiled_body, created_at) VALUES (?1, ?2, 1, ?3, ?4)"
          ),
          params![body_id, node_id, format!("Body for node {index:03}"), now],
        )
        .unwrap();
    }

    let canvas = store.load_graph_canvas_snapshot().unwrap();
    assert_eq!(canvas["nodes"].as_array().unwrap().len(), 160);
    assert!(canvas["nodes"].as_array().unwrap().iter().all(|node| node["id"] != "search_node_164"));

    let results = store.search_graph_node_cards("tailneedle", 5).unwrap();
    assert_eq!(results.as_array().unwrap().len(), 1);
    assert_eq!(results[0]["id"], "search_node_164");
    assert!(results[0].get("compiled_body").is_none());
    assert!(results[0].get("evidence").is_none());

    let error = store.search_graph_node_cards("tailneedle", GRAPH_NODE_SEARCH_LIMIT_MAX + 1).unwrap_err();
    assert_eq!(error.code, "Soma_VALIDATION_ERROR");
  }

  #[test]
  fn accepting_edge_without_bridge_text_preserves_typed_graph_connection() {
    let mut store = test_store();
    seed_chunk(&store.conn, "chunk_1");

    let message = store.append_graph_message("connectedness slider bridge", Vec::new()).unwrap();
    let message_id = message["message"]["id"].as_str().unwrap();
    let mut patch = empty_graph_patch();
    patch["proposed_nodes"] = json!([
      node_payload("node_source", "Source Node", "Source body.", "chunk_1"),
      node_payload("node_target", "Target Node", "Target body.", "chunk_1")
    ]);
    patch["proposed_edges"] = json!([{
      "source_temp_id": "node_source",
      "target_temp_id": "node_target",
      "type": "supports",
      "source_chunk_ids": ["chunk_1"],
      "reason": "Typed edge evidence is enough when bridge text is absent."
    }]);
    store.propose_graph_updates(message_id, patch).unwrap();
    let queue = store.load_review_queue().unwrap();
    let items = queue["groups"]["proposed"]["items"].as_array().unwrap();
    let patch_id = items[0]["patch_id"].as_str().unwrap();
    let accepted = store.accept_graph_patch_proposals(patch_id, None).unwrap();

    assert_eq!(accepted["acceptedCount"], 3);
    assert!(accepted["errors"].as_array().unwrap().is_empty());
    let snapshot = store.load_graph_snapshot().unwrap();
    assert_eq!(snapshot["nodes"].as_array().unwrap().len(), 2);
    assert_eq!(snapshot["edges"].as_array().unwrap().len(), 1);
    assert_eq!(snapshot["edges"][0]["type"], "supports");
    assert_eq!(snapshot["edges"][0]["bridge_text"], Value::Null);
    let edge = &snapshot["edges"][0];
    let source_detail = store.load_graph_node_detail(edge["source_node_id"].as_str().unwrap()).unwrap();
    let source_relation = &source_detail["relations"]["items"][0];
    assert_eq!(source_relation["type"], "supports");
    assert_eq!(source_relation["direction"], "outgoing");
    assert_eq!(source_relation["bridge_text"], "");
    assert_eq!(source_relation["neighbor"]["title"], "Target Node");

    let target_detail = store.load_graph_node_detail(edge["target_node_id"].as_str().unwrap()).unwrap();
    let target_relation = &target_detail["relations"]["items"][0];
    assert_eq!(target_relation["direction"], "incoming");
    assert_eq!(target_relation["neighbor"]["title"], "Source Node");
    assert_eq!(snapshot["edges"][0]["source_chunk_ids"][0], "chunk_1");
  }

  #[test]
  fn canvas_snapshot_omits_full_node_detail_until_node_is_selected() {
    let mut store = test_store();
    seed_chunk(&store.conn, "chunk_1");

    let long_body = "This full compiled body should stay out of the startup canvas payload. ".repeat(40);
    let message = store.append_graph_message("startup canvas should stay small", Vec::new()).unwrap();
    let message_id = message["message"]["id"].as_str().unwrap();
    let patch = node_patch("node_canvas_payload", "Canvas Payload", &long_body, "chunk_1");
    store.propose_graph_updates(message_id, patch).unwrap();
    let proposal_id =
      store.load_review_queue().unwrap()["groups"]["proposed"]["items"][0]["id"].as_str().unwrap().to_string();
    store.accept_graph_proposal(&proposal_id, None).unwrap();

    let canvas = store.load_graph_canvas_snapshot().unwrap();
    let canvas_node = &canvas["nodes"][0];
    assert_eq!(canvas_node["title"], "Canvas Payload");
    assert!(canvas_node.get("compiled_body").is_none());
    assert!(canvas_node.get("body_sections").is_none());
    assert!(canvas_node.get("update_history").is_none());
    assert!(canvas_node.get("evidence").is_none());

    let detail = store.load_graph_node_detail(canvas_node["id"].as_str().unwrap()).unwrap();
    assert_eq!(detail["compiled_body"], long_body);
    assert!(detail.get("body_sections").is_none());
    assert!(detail.get("body_max_words").is_none());
    assert!(detail["relations"]["items"].as_array().unwrap().is_empty());
    assert!(!detail["evidence"].as_array().unwrap().is_empty());
  }

  #[test]
  fn review_queue_exposes_exact_mutation_text_without_the_raw_proposal_payload() {
    let mut store = test_store();
    seed_chunk(&store.conn, "chunk_1");

    let long_body = "This full proposal body should stay out of the review queue list payload. ".repeat(40);
    let message = store.append_graph_message("review queue should stay small", Vec::new()).unwrap();
    let message_id = message["message"]["id"].as_str().unwrap();
    store
      .propose_graph_updates(message_id, node_patch("node_review_payload", "Review Payload", &long_body, "chunk_1"))
      .unwrap();

    let queue = store.load_review_queue().unwrap();
    let item = &queue["groups"]["proposed"]["items"][0];

    assert_eq!(item["title"], "Review Payload");
    assert_eq!(item["reason"], "fixture");
    assert_eq!(item["evidence_count"], 2);
    assert!(item.get("payload").is_none());
    assert_eq!(item["mutation_payload"]["compiled_body"], long_body);
  }

  #[test]
  fn lifecycle_commands_keep_proposals_out_of_graph_truth() {
    let mut store = test_store();
    seed_chunk(&store.conn, "chunk_1");
    let message = store.append_graph_message("review several proposals", Vec::new()).unwrap();
    let message_id = message["message"]["id"].as_str().unwrap();
    let mut patch = empty_graph_patch();
    patch["proposed_nodes"] =
      json!([node_payload("node_a", "A", "Body A", "chunk_1"), node_payload("node_b", "B", "Body B", "chunk_1")]);
    store.propose_graph_updates(message_id, patch).unwrap();
    let queue = store.load_review_queue().unwrap();
    let items = queue["groups"]["proposed"]["items"].as_array().unwrap();
    let reject_id = items[0]["id"].as_str().unwrap().to_string();
    let defer_id = items[1]["id"].as_str().unwrap().to_string();

    store.reject_graph_proposal(&reject_id, Some("not useful")).unwrap();
    store.defer_graph_proposal(&defer_id, Some("later")).unwrap();
    let queue = store.load_review_queue().unwrap();
    assert_eq!(queue["groups"]["rejected"]["count"], 1);
    assert_eq!(queue["groups"]["deferred"]["count"], 1);
    assert_eq!(store.load_graph_snapshot().unwrap()["nodes"].as_array().unwrap().len(), 0);
  }

  #[test]
  fn persists_layout_and_lists_graph_messages_without_mutating_graph_truth() {
    let mut store = test_store();
    seed_chunk(&store.conn, "chunk_1");

    let message = store.append_graph_message("connectedness slider layout", Vec::new()).unwrap();
    let message_id = message["message"]["id"].as_str().unwrap().to_string();
    let patch = node_patch("node_slider", "Connectedness Slider", "A compiled section about graph density.", "chunk_1");
    store.propose_graph_updates(&message_id, patch).unwrap();
    let queue = store.load_review_queue().unwrap();
    let proposal_id = queue["groups"]["proposed"]["items"][0]["id"].as_str().unwrap().to_string();
    store.accept_graph_proposal(&proposal_id, None).unwrap();

    let before_layout = store.load_graph_snapshot().unwrap();
    let node_id = before_layout["nodes"][0]["id"].as_str().unwrap().to_string();
    let saved = store.persist_node_position(&node_id, 220.0, 300.0, true).unwrap();
    let bootstrap = store.load_workspace_bootstrap().unwrap();
    let listed = &bootstrap["layout"];
    let stored_layout = listed["layoutOverrides"].as_object().unwrap().get(&node_id).unwrap();
    let messages = store.list_graph_messages().unwrap();

    assert_eq!(saved["node_id"], node_id);
    assert_eq!(saved["pinned"], true);
    assert_eq!(listed["pinnedNodeIds"].as_array().unwrap()[0], node_id);
    assert_eq!(stored_layout["x"], saved["x"]);
    assert_eq!(stored_layout["y"], saved["y"]);
    assert_eq!(store.load_graph_snapshot().unwrap(), before_layout);
    assert_eq!(messages.as_array().unwrap()[0]["id"], message_id);
    assert!(messages.as_array().unwrap()[0].get("context_packet").is_none());
  }

  #[test]
  fn stores_node_messages_and_focus_set_graph_context() {
    let mut store = test_store();
    let node_id = seed_accepted_node(&mut store);
    let node_message = store.append_node_message(&node_id, "Keep this inside the selected node.").unwrap();
    let node_messages = store.list_node_messages(&node_id).unwrap();
    let focused_graph_message =
      store.append_graph_message("unrelated workspace message", vec![node_id.clone()]).unwrap();

    assert_eq!(node_message["message"]["node_id"], node_id);
    assert_eq!(node_message["context_packet"]["mode"], "node_chat");
    assert_eq!(node_message["context_packet"]["focused_node_id"], node_id);
    assert_eq!(node_messages.as_array().unwrap().len(), 1);
    assert_eq!(focused_graph_message["context_packet"]["focus_node_ids"][0], node_id);
    assert_eq!(focused_graph_message["used_graph_areas"][0]["id"], node_id);
  }

  #[test]
  fn node_context_history_stays_bounded_across_turns() {
    let mut store = test_store();
    let node_id = seed_accepted_node(&mut store);
    let mut steady_packet_sizes = Vec::new();

    for turn in 0..10 {
      let user_turn = store.append_node_message(&node_id, &format!("Question {turn:02}")).unwrap();
      let context_packet = user_turn["context_packet"].clone();
      let recent_messages = context_packet["node_thread_recent_messages"].as_array().unwrap();

      assert!(recent_messages.len() <= 6);
      assert!(recent_messages.iter().all(|message| message.get("context_packet").is_none()));

      if turn >= 3 {
        steady_packet_sizes.push(context_packet.to_string().len());
      }

      store.append_node_assistant_message(&node_id, &format!("Answer {turn:02}"), &context_packet).unwrap();
    }

    let smallest_packet = steady_packet_sizes.iter().min().unwrap();
    let largest_packet = steady_packet_sizes.iter().max().unwrap();
    assert!(
      largest_packet - smallest_packet <= 128,
      "node context grew after the six-message history window filled: {steady_packet_sizes:?}"
    );

    let messages = store.list_node_messages(&node_id).unwrap();
    assert_eq!(messages.as_array().unwrap().len(), 20);
    assert!(messages.as_array().unwrap().iter().all(|message| {
      message.get("id").and_then(Value::as_str).is_some()
        && message.get("node_id").and_then(Value::as_str) == Some(node_id.as_str())
        && message.get("role").and_then(Value::as_str).is_some()
        && message.get("content").and_then(Value::as_str).is_some()
        && message.get("created_at").and_then(Value::as_str).is_some()
        && message.get("context_packet").is_none()
    }));
  }

  #[test]
  fn merge_candidate_rejects_accept_but_preserves_review_lifecycle() {
    let mut store = test_store();
    seed_chunk(&store.conn, "chunk_1");

    let message = store.append_graph_message("Review overlapping concepts before merging.", Vec::new()).unwrap();
    let message_id = message["message"]["id"].as_str().unwrap();
    let mut patch = empty_graph_patch();
    patch["proposed_nodes"] = json!([
      node_payload("node_a", "Concept A", "First formulation.", "chunk_1"),
      node_payload("node_b", "Concept B", "Second formulation.", "chunk_1")
    ]);
    patch["merge_candidates"] = json!([{
      "candidate_node_refs": ["node_a", "node_b"],
      "source_chunk_ids": ["chunk_1"],
      "reason": "The two proposals may describe the same concept."
    }]);

    let proposed = store.propose_graph_updates(message_id, patch).unwrap();
    assert_eq!(proposed["valid"], true);
    assert_eq!(proposed["proposal_status"], "proposed");

    let queue = store.load_review_queue().unwrap();
    let merge_id = queue["items"].as_array().unwrap().iter().find(|item| item["type"] == "merge_candidate").unwrap()
      ["id"]
      .as_str()
      .unwrap()
      .to_string();
    let error = store.accept_graph_proposal(&merge_id, None).unwrap_err();
    assert_eq!(error.code, "Soma_VALIDATION_ERROR");
    assert_eq!(
      error.message,
      "Merge candidates support Reject or Later, but not Accept until transactional merging is implemented."
    );

    let queue = store.load_review_queue().unwrap();
    let merge = queue["items"].as_array().unwrap().iter().find(|item| item["id"] == merge_id).unwrap();
    assert_eq!(merge["status"], "proposed");

    let deferred = store.defer_graph_proposal(&merge_id, Some("Review the overlap later.")).unwrap();
    assert_eq!(deferred["status"], "deferred");
    let queue = store.load_review_queue().unwrap();
    let merge = queue["items"].as_array().unwrap().iter().find(|item| item["id"] == merge_id).unwrap();
    assert_eq!(merge["status"], "deferred");
    assert_eq!(merge["decision_reason"], "Review the overlap later.");

    let rejected = store.reject_graph_proposal(&merge_id, Some("Keep both concepts.")).unwrap();
    assert_eq!(rejected["status"], "rejected");

    let queue = store.load_review_queue().unwrap();
    let merge = queue["items"].as_array().unwrap().iter().find(|item| item["id"] == merge_id).unwrap();
    assert_eq!(merge["status"], "rejected");
    assert_eq!(merge["decision_reason"], "Keep both concepts.");
    let snapshot = store.load_graph_snapshot().unwrap();
    assert!(snapshot["nodes"].as_array().unwrap().is_empty());
    assert!(snapshot["edges"].as_array().unwrap().is_empty());
  }

  #[test]
  fn rolls_back_node_body_to_exact_requested_version() {
    let mut store = test_store();
    seed_chunk(&store.conn, "chunk_1");
    let original = "The exact first body remains recoverable.";
    let node_id = accept_test_node(&mut store, "node_rollback", "Rollback", original);

    let second = store.update_node_body(&node_id, "A second user-authored version.").unwrap();
    let third = store.update_node_body(&node_id, "A third user-authored version.").unwrap();
    assert_eq!(second["bodyVersion"], 2);
    assert_eq!(third["bodyVersion"], 3);

    let rolled_back = store.rollback_node_body(&node_id, 1).unwrap();
    assert_eq!(rolled_back["bodyVersion"], 1);
    let detail = store.load_graph_node_detail(&node_id).unwrap();
    assert_eq!(detail["compiled_body"], original);
    assert_eq!(detail["body_version"], 1);
    assert_eq!(detail["body_version_id"], rolled_back["bodyVersionId"]);
    assert_eq!(detail["update_history"].as_array().unwrap().len(), 3);
    let first =
      detail["update_history"].as_array().unwrap().iter().find(|version| version["version_number"] == 1).unwrap();
    assert_eq!(first["is_current"], true);
  }

  #[test]
  fn direct_user_body_enforces_the_unicode_character_limit() {
    let mut store = test_store();
    seed_chunk(&store.conn, "chunk_1");
    let node_id = accept_test_node(&mut store, "node_body_limit", "Body limit", "Original body.");
    let maximum = "\u{1F9E0}".repeat(NODE_BODY_MAX_CHARS);

    let updated = store.update_node_body(&node_id, &maximum).unwrap();
    assert_eq!(updated["bodyVersion"], 2);
    assert_eq!(store.load_graph_node_detail(&node_id).unwrap()["compiled_body"], maximum);

    let error = store.update_node_body(&node_id, &format!("{maximum}\u{1F9E0}")).unwrap_err();
    assert_eq!(error.code, "Soma_VALIDATION_ERROR");
    assert_eq!(error.message, format!("compiled_body must not exceed {NODE_BODY_MAX_CHARS} characters."));
    let detail = store.load_graph_node_detail(&node_id).unwrap();
    assert_eq!(detail["compiled_body"], maximum);
    assert_eq!(detail["update_history"].as_array().unwrap().len(), 2);
  }

  #[test]
  fn accepts_append_section_as_a_new_node_body_version() {
    let mut store = test_store();
    seed_chunk(&store.conn, "chunk_1");
    let node_id = accept_test_node(&mut store, "node_section", "Section update", "First section.\n\nSecond section.");
    let base_body_version_id =
      store.load_graph_node_detail(&node_id).unwrap()["body_version_id"].as_str().unwrap().to_string();

    let message = store.append_graph_message("Append a third section.", Vec::new()).unwrap();
    let message_id = message["message"]["id"].as_str().unwrap();
    let mut patch = empty_graph_patch();
    patch["proposed_node_body_updates"] = json!([{
      "target_node_id": node_id,
      "base_body_version_id": base_body_version_id,
      "update_kind": "append_section",
      "section_text": "Third section.",
      "source_chunk_ids": ["chunk_1"],
      "reason": "The evidence supports an additional section."
    }]);

    let proposed = store.propose_graph_updates(message_id, patch).unwrap();
    assert_eq!(proposed["valid"], true);
    let queue = store.load_review_queue().unwrap();
    let update_id = queue["items"].as_array().unwrap().iter().find(|item| item["type"] == "node_body_update").unwrap()
      ["id"]
      .as_str()
      .unwrap()
      .to_string();
    let accepted = store.accept_graph_proposal(&update_id, None).unwrap();
    assert_eq!(accepted["status"], "accepted");
    assert_eq!(accepted["entityType"], "node_body_version");

    let detail = store.load_graph_node_detail(&node_id).unwrap();
    assert_eq!(detail["compiled_body"], "First section.\n\nSecond section.\n\nThird section.");
    assert_eq!(detail["body_version"], 2);
    assert_eq!(detail["body_version_id"], accepted["entityId"]);
  }

  fn test_store() -> WorkspaceStore {
    let path = std::env::temp_dir().join(format!("soma-tauri-test-{}.sqlite", new_id()));
    drop(open_database(&path).unwrap());
    WorkspaceStore::open(path).unwrap()
  }

  fn seed_accepted_node(store: &mut WorkspaceStore) -> String {
    seed_chunk(&store.conn, "chunk_1");
    let graph_message = store.append_graph_message("connectedness slider", Vec::new()).unwrap();
    let message_id = graph_message["message"]["id"].as_str().unwrap();
    store
      .propose_graph_updates(
        message_id,
        node_patch("node_slider", "Connectedness Slider", "A compiled section about graph density.", "chunk_1"),
      )
      .unwrap();
    let queue = store.load_review_queue().unwrap();
    let proposal_id = queue["groups"]["proposed"]["items"][0]["id"].as_str().unwrap().to_string();
    store.accept_graph_proposal(&proposal_id, None).unwrap();

    store.load_graph_snapshot().unwrap()["nodes"][0]["id"].as_str().unwrap().to_string()
  }

  fn remove_sqlite_file_set(path: &Path) {
    let _ = std::fs::remove_file(path);
    let _ = std::fs::remove_file(path.with_extension("sqlite-wal"));
    let _ = std::fs::remove_file(path.with_extension("sqlite-shm"));
  }

  fn seed_chunk(conn: &Connection, chunk_id: &str) {
    let now = now_string().unwrap();
    conn
      .execute(
        concat!(
          "INSERT INTO sources (id, source_type, title, original_path, raw_path, imported_at) ",
          "VALUES ('source_1', 'text', 'Source', 'source.txt', 'raw/source.txt', ?1)"
        ),
        params![now],
      )
      .unwrap();
    conn
      .execute(
        concat!(
          "INSERT INTO conversations (id, source_id, provider, title, created_at) ",
          "VALUES ('conversation_1', 'source_1', 'manual', 'Conversation', ?1)"
        ),
        params![now],
      )
      .unwrap();
    conn
      .execute(
        concat!(
          "INSERT INTO messages (id, conversation_id, role, content, order_index, created_at) ",
          "VALUES ('message_1', 'conversation_1', 'user', 'Graph density needs a slider.', 0, ?1)"
        ),
        params![now],
      )
      .unwrap();
    conn
      .execute(
        concat!(
          "INSERT INTO chunks (id, message_id, content, chunk_index, token_count) ",
          "VALUES (?1, 'message_1', 'Graph density needs a slider.', 0, 6)"
        ),
        params![chunk_id],
      )
      .unwrap();
  }

  fn node_patch(temp_id: &str, title: &str, body: &str, chunk_id: &str) -> Value {
    let mut patch = empty_graph_patch();
    patch["proposed_nodes"] = json!([node_payload(temp_id, title, body, chunk_id)]);
    patch
  }

  fn node_payload(temp_id: &str, title: &str, body: &str, chunk_id: &str) -> Value {
    json!({
      "temp_id": temp_id,
      "type": "concept",
      "title": title,
      "preview": title,
      "compiled_body": body,
      "source_chunk_ids": [chunk_id],
      "reason": "fixture"
    })
  }

  fn accept_test_node(store: &mut WorkspaceStore, temp_id: &str, title: &str, body: &str) -> String {
    let message = store.append_graph_message("Create a node fixture.", Vec::new()).unwrap();
    let message_id = message["message"]["id"].as_str().unwrap();
    store.propose_graph_updates(message_id, node_patch(temp_id, title, body, "chunk_1")).unwrap();
    let queue = store.load_review_queue().unwrap();
    let proposal_id = queue["items"]
      .as_array()
      .unwrap()
      .iter()
      .find(|item| item["type"] == "node" && item["temp_id"] == temp_id)
      .unwrap()["id"]
      .as_str()
      .unwrap()
      .to_string();
    store.accept_graph_proposal(&proposal_id, None).unwrap()["entityId"].as_str().unwrap().to_string()
  }
}
