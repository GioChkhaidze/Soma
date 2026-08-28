use super::*;
use crate::contracts::empty_graph_patch;
use crate::database::open_database;
use crate::error::{CommandResult, RuntimeFailureKind};
use crate::repository::WorkspaceStore;
use crate::source_import::import_source_file;
use crate::workspace::{create_workspace_dir, WorkspacePaths};
use rusqlite::{params, Connection};
use soma_ai_runtime::{AgentTaskCancellation, NoopCredentialResolver};
use std::collections::HashSet;
use std::fs;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::Path;
use std::sync::mpsc;
use std::thread;
use uuid::Uuid;

fn send_graph_chat_turn_with_runtime(
  paths: &WorkspacePaths,
  runtime: &Value,
  content: &str,
  focus_node_ids: Vec<String>,
) -> CommandResult<Value> {
  send_graph_chat_turn_with_credentials(paths, runtime, content, focus_node_ids, &NoopCredentialResolver)
}

fn send_node_chat_turn_with_runtime(
  paths: &WorkspacePaths,
  runtime: &Value,
  node_id: &str,
  content: &str,
) -> CommandResult<Value> {
  send_node_chat_turn_with_runtime_and_capture(paths, runtime, node_id, content, true)
}

fn send_node_chat_turn_with_runtime_and_capture(
  paths: &WorkspacePaths,
  runtime: &Value,
  node_id: &str,
  content: &str,
  capture_graph_changes: bool,
) -> CommandResult<Value> {
  send_node_chat_turn_with_credentials(
    paths,
    runtime,
    node_id,
    content,
    capture_graph_changes,
    ChatRuntimeExecution::new(&NoopCredentialResolver, AgentTaskCancellation::new()),
  )
}

#[test]
fn chat_mutation_preconditions_come_from_the_retrieval_snapshot() {
  let context_packet = json!({
    "focus_set_node_bodies": [{
      "id": "node_1",
      "body_version_id": "body_snapshot"
    }],
    "relevant_path_fragments": [{
      "edge_id": "edge_1",
      "updated_at": "2026-07-25T00:00:00Z"
    }]
  });
  let mut patch = empty_graph_patch();
  patch["proposed_node_body_updates"] = json!([{
    "target_node_id": "node_1",
    "base_body_version_id": "model_forgery",
    "update_kind": "replace_body",
    "compiled_body": "Replacement.",
    "reason": "Fixture."
  }]);
  patch["proposed_edge_bridge_updates"] = json!([{
    "target_edge_id": "edge_1",
    "base_edge_updated_at": "model_forgery",
    "bridge_text": "Replacement.",
    "reason": "Fixture."
  }]);

  let (stamped, all_preconditioned) = attach_chat_mutation_preconditions(patch.clone(), &context_packet);

  assert!(all_preconditioned);
  assert_eq!(stamped["proposed_node_body_updates"][0]["base_body_version_id"], "body_snapshot");
  assert_eq!(stamped["proposed_edge_bridge_updates"][0]["base_edge_updated_at"], "2026-07-25T00:00:00Z");

  let (unstamped, all_preconditioned) = attach_chat_mutation_preconditions(patch, &json!({}));
  assert!(!all_preconditioned);
  assert!(unstamped["proposed_node_body_updates"][0].get("base_body_version_id").is_none());
  assert!(unstamped["proposed_edge_bridge_updates"][0].get("base_edge_updated_at").is_none());
}

#[test]
fn graph_chat_turn_answers_and_applies_graph_updates() {
  let root = temp_root("soma-graph-chat-turn-test");
  let paths = workspace_with_source(
    &root,
    concat!(
      "User: Direct graph chat should answer immediately.\n\n",
      "Assistant: Graph updates should enter Review Updates before graph truth changes.",
    ),
  );
  let chunk_id = first_chunk_id(&paths);
  let chat_json = json!({
    "assistant_message": "Graph chat should answer first, then offer reviewable graph updates.",
    "used_graph_areas": [],
    "proposed_graph_patch": {
      "proposed_nodes": [{
        "temp_id": "node_direct_graph_chat",
        "title": "Direct Graph Chat",
        "compiled_body": concat!(
          "Direct graph chat retrieves workspace context, returns an assistant answer immediately, ",
          "and imports any graph changes as proposals for Review Updates.",
        ),
        "source_chunk_ids": [chunk_id],
        "reason": "The imported source describes direct graph chat and reviewable updates."
      }]
    }
  });
  let (runtime, server) = local_runtime_with_chat_response(chat_json);

  let result =
    send_graph_chat_turn_with_runtime(&paths, &runtime, "How should graph chat behave?", Vec::new()).unwrap();
  server.join().unwrap();

  assert_eq!(result["runtime_status"], "completed", "{result:#}");
  assert_eq!(result["patch_import_status"], "accepted_to_graph");
  assert_eq!(result["proposal_count"], 1);
  assert!(result["assistant_message"]["content"].as_str().unwrap().contains("answer first"));

  let store = WorkspaceStore::open(&paths.database_path).unwrap();
  let queue = store.load_review_queue().unwrap();
  assert_eq!(queue["items"].as_array().unwrap().len(), 1);
  assert_eq!(queue["items"][0]["status"], "accepted");
  assert_eq!(queue["items"][0]["source"]["kind"], "graph_message");
  assert_eq!(queue["items"][0]["source_message_id"], result["assistant_message"]["id"]);
  let graph = store.load_graph_snapshot().unwrap();
  assert_eq!(graph["nodes"].as_array().unwrap().len(), 1);
  assert_eq!(graph["nodes"][0]["title"], "Direct Graph Chat");
  assert_eq!(graph["nodes"][0]["type"], "concept");
  let _ = fs::remove_dir_all(root);
}

#[test]
fn model_graph_patch_warnings_survive_import_results_without_becoming_proposals() {
  let root = temp_root("soma-graph-patch-warning-import-test");
  let paths = create_workspace_dir(&root).unwrap();
  let (runtime, server) = local_runtime_with_chat_response(json!({
    "assistant_message": "The answer is usable, with one bounded warning.",
    "used_graph_areas": [],
    "proposed_graph_patch": {
      "warnings": [{
        "path": " $.context ",
        "message": " Verify the limited context before relying on this answer. ",
        "provider_detail": { "must_not_cross": true }
      }]
    }
  }));

  let result = send_graph_chat_turn_with_runtime(&paths, &runtime, "Answer with a warning only.", Vec::new()).unwrap();
  server.join().unwrap();

  assert_eq!(result["runtime_status"], "completed", "{result:#}");
  assert_eq!(result["patch_import_status"], "none");
  assert_eq!(result["proposal_count"], 0);
  assert_eq!(
    result["patch_import_result"]["warnings"],
    json!([{
      "path": "$.context",
      "message": "Verify the limited context before relying on this answer."
    }])
  );
  let store = WorkspaceStore::open(&paths.database_path).unwrap();
  assert!(store.load_review_queue().unwrap()["items"].as_array().unwrap().is_empty());
  assert!(store.load_graph_snapshot().unwrap()["nodes"].as_array().unwrap().is_empty());
  let _ = fs::remove_dir_all(root);
}

#[test]
fn every_valid_edge_bridge_update_is_acceptance_ready() {
  let root = temp_root("soma-edge-bridge-validation-acceptance-parity-test");
  let paths = workspace_with_source(&root, "User: Edge bridge updates must be valid before they reach review.");
  let chunk_id = first_chunk_id(&paths);
  let (_, _, edge_id) = seed_connected_active_nodes(
    &paths,
    "edge_source",
    "Edge source",
    "The source concept anchors the bridge.",
    &chunk_id,
    "edge_target",
    "Edge target",
    "The target concept receives the bridge.",
    &chunk_id,
    "The original bridge.",
  );
  let mut store = WorkspaceStore::open(&paths.database_path).unwrap();
  let graph = store.load_graph_snapshot().unwrap();
  let base_edge_updated_at = graph["edges"].as_array().unwrap().iter().find(|edge| edge["id"] == edge_id).unwrap()
    ["updated_at"]
    .as_str()
    .unwrap()
    .to_string();
  let message = store.append_graph_message("Replace the edge bridge with a clearer explanation.", Vec::new()).unwrap();
  let message_id = message["message"]["id"].as_str().unwrap();
  let mut patch = empty_graph_patch();
  patch["proposed_edge_bridge_updates"] = json!([{
    "target_edge_id": edge_id,
    "base_edge_updated_at": base_edge_updated_at,
    "reason": "Exercise validation and acceptance parity."
  }]);

  let invalid = store.propose_graph_updates(message_id, patch.clone()).unwrap();
  assert_eq!(invalid["valid"], false);
  assert!(invalid["errors"]
    .as_array()
    .unwrap()
    .iter()
    .any(|error| error["path"] == "$.proposed_edge_bridge_updates[0].bridge_text"));

  patch["proposed_edge_bridge_updates"][0]["bridge_text"] = json!("The acceptance-ready replacement bridge.");
  let proposed = store.propose_graph_updates(message_id, patch).unwrap();
  assert_eq!(proposed["valid"], true, "{proposed:#}");
  let proposal_id = proposed["proposals"][0]["id"].as_str().unwrap();
  let accepted = store.accept_graph_proposal(proposal_id, None).unwrap();
  assert_eq!(accepted["status"], "accepted");
  let graph = store.load_graph_snapshot().unwrap();
  let edge = graph["edges"].as_array().unwrap().iter().find(|edge| edge["id"] == edge_id).unwrap();
  assert_eq!(edge["bridge_text"], "The acceptance-ready replacement bridge.");
  let _ = fs::remove_dir_all(root);
}

#[test]
fn graph_chat_turn_uses_paper_context_without_mutating_graph_when_capture_is_off() {
  let root = temp_root("soma-paper-chat-no-capture-test");
  let paths = create_workspace_dir(&root).unwrap();
  let chat_json = json!({
    "assistant_message": "The highlighted passage says recursive depth is the central tradeoff.",
    "used_graph_areas": [],
    "proposed_graph_patch": {
      "proposed_nodes": [{
        "temp_id": "paper_tradeoff",
        "title": "Recursive Depth Tradeoff",
        "compiled_body": "Recursive depth is the central tradeoff."
      }]
    }
  });
  let (runtime, server) = local_runtime_with_chat_response(chat_json);

  let result = send_graph_chat_turn_with_reading_context_and_credentials(
    &paths,
    &runtime,
    "What does this mean?",
    Vec::new(),
    Some(json!({
      "kind": "pdf",
      "document_name": "tiny-networks.pdf",
      "page_number": 4,
      "page_count": 12,
      "page_text": "This page discusses recursive reasoning depth.",
      "selected_text": "recursive depth is the central tradeoff",
      "selection_page_number": 4
    })),
    false,
    ChatRuntimeExecution::new(&NoopCredentialResolver, AgentTaskCancellation::new()),
  )
  .unwrap();
  server.join().unwrap();

  assert_eq!(result["patch_import_status"], "none", "{result:#}");
  assert_eq!(result["proposal_count"], 0);
  assert_eq!(result["context_packet"]["graph_capture_enabled"], false);
  assert_eq!(result["context_packet"]["reading_context"]["selected_text"], "recursive depth is the central tradeoff");
  let store = WorkspaceStore::open(&paths.database_path).unwrap();
  assert!(store.load_graph_snapshot().unwrap()["nodes"].as_array().unwrap().is_empty());
  assert!(store.load_review_queue().unwrap()["items"].as_array().unwrap().is_empty());
  let _ = fs::remove_dir_all(root);
}

#[test]
fn auto_applied_chat_patch_can_restore_the_previous_graph_state() {
  let root = temp_root("soma-chat-undo-test");
  let paths = create_workspace_dir(&root).unwrap();
  let chat_json = json!({
    "assistant_message": "I added the durable concept.",
    "used_graph_areas": [],
    "proposed_graph_patch": {
      "proposed_nodes": [{
        "temp_id": "undoable_concept",
        "type": "concept",
        "title": "Undoable Concept",
        "compiled_body": "This concept exists to verify chat patch undo."
      }]
    }
  });
  let (runtime, server) = local_runtime_with_chat_response(chat_json);
  let result = send_graph_chat_turn_with_runtime(&paths, &runtime, "Create an undoable concept.", Vec::new()).unwrap();
  server.join().unwrap();
  let patch_id = result["patch_import_result"]["patchId"].as_str().unwrap().to_string();

  let mut store = WorkspaceStore::open(&paths.database_path).unwrap();
  assert_eq!(store.load_graph_snapshot().unwrap()["nodes"].as_array().unwrap().len(), 1);
  let review = store.load_review_queue().unwrap();
  assert_eq!(review["latest_undoable_patch"]["patch_id"], patch_id);
  assert_eq!(review["latest_undoable_patch"]["source_message_id"], result["assistant_message"]["id"]);
  assert_eq!(review["latest_undoable_patch"]["change_count"], 1);
  let undone = store.undo_graph_patch(&patch_id).unwrap();

  assert_eq!(undone["status"], "undone");
  assert_eq!(undone["undoneCount"], 1);
  assert!(store.load_graph_snapshot().unwrap()["nodes"].as_array().unwrap().is_empty());
  let review = store.load_review_queue().unwrap();
  assert_eq!(review["items"][0]["status"], "superseded");
  assert!(review["latest_undoable_patch"].is_null());
  assert!(store.undo_graph_patch(&patch_id).is_err());
  let _ = fs::remove_dir_all(root);
}

#[test]
fn chat_patch_undo_refuses_to_overwrite_a_later_accepted_update() {
  let root = temp_root("soma-chat-undo-later-update-test");
  let paths = create_workspace_dir(&root).unwrap();
  let (runtime, server) = local_runtime_with_chat_response(json!({
    "assistant_message": "I added the first concept.",
    "used_graph_areas": [],
    "proposed_graph_patch": {
      "proposed_nodes": [{
        "temp_id": "first_concept",
        "type": "concept",
        "title": "First Concept",
        "compiled_body": "The first accepted chat update."
      }]
    }
  }));
  let result = send_graph_chat_turn_with_runtime(&paths, &runtime, "Add the first concept.", Vec::new()).unwrap();
  server.join().unwrap();
  let patch_id = result["patch_import_result"]["patchId"].as_str().unwrap().to_string();

  let mut store = WorkspaceStore::open(&paths.database_path).unwrap();
  let message = store.append_graph_message("Add a later concept.", Vec::new()).unwrap();
  let message_id = message["message"]["id"].as_str().unwrap();
  let mut later_patch = empty_graph_patch();
  later_patch["proposed_nodes"] = json!([{
    "temp_id": "later_concept",
    "type": "concept",
    "title": "Later Concept",
    "compiled_body": "A later accepted update."
  }]);
  let proposed = store.propose_graph_updates(message_id, later_patch).unwrap();
  let proposal_id = proposed["proposals"]
    .get(0)
    .and_then(|proposal| proposal.get("id"))
    .and_then(Value::as_str)
    .unwrap_or_else(|| panic!("later proposal was not imported: {proposed:#}"))
    .to_string();
  store.accept_graph_proposal(&proposal_id, None).unwrap();

  assert!(store.load_review_queue().unwrap()["latest_undoable_patch"].is_null());
  let error = store.undo_graph_patch(&patch_id).unwrap_err();
  assert!(error.message.contains("cannot be undone safely"));
  assert_eq!(store.load_graph_snapshot().unwrap()["nodes"].as_array().unwrap().len(), 2);
  let _ = fs::remove_dir_all(root);
}

#[test]
fn manually_and_partially_accepted_patch_never_exposes_undo() {
  let root = temp_root("soma-chat-partial-undo-test");
  let paths = create_workspace_dir(&root).unwrap();
  let mut store = WorkspaceStore::open(&paths.database_path).unwrap();
  let message = store.append_graph_message("Add two concepts.", Vec::new()).unwrap();
  let message_id = message["message"]["id"].as_str().unwrap();
  let mut patch = empty_graph_patch();
  patch["proposed_nodes"] = json!([
    {
      "temp_id": "manual_concept",
      "type": "concept",
      "title": "Manual Concept",
      "compiled_body": "This proposal is accepted manually."
    },
    {
      "temp_id": "bulk_concept",
      "type": "concept",
      "title": "Bulk Concept",
      "compiled_body": "This proposal is accepted by the remaining patch action."
    }
  ]);
  let proposed = store.propose_graph_updates(message_id, patch).unwrap();
  let patch_id = proposed["patchId"].as_str().unwrap();
  let manual_proposal_id = proposed["proposals"][0]["id"].as_str().unwrap();

  store.accept_graph_proposal(manual_proposal_id, None).unwrap();
  assert!(store.load_review_queue().unwrap()["latest_undoable_patch"].is_null());
  assert_eq!(store.accept_graph_patch_proposals(patch_id, None).unwrap()["acceptedCount"], 1);
  assert!(store.load_review_queue().unwrap()["latest_undoable_patch"].is_null());
  assert!(store.undo_graph_patch(patch_id).is_err());
  assert_eq!(store.load_graph_snapshot().unwrap()["nodes"].as_array().unwrap().len(), 2);
  let _ = fs::remove_dir_all(root);
}

#[test]
fn direct_edit_to_new_chat_node_invalidates_patch_undo() {
  let root = temp_root("soma-chat-undo-direct-edit-test");
  let paths = create_workspace_dir(&root).unwrap();
  let (runtime, server) = local_runtime_with_chat_response(json!({
    "assistant_message": "I added the editable concept.",
    "used_graph_areas": [],
    "proposed_graph_patch": {
      "proposed_nodes": [{
        "temp_id": "editable_concept",
        "type": "concept",
        "title": "Editable Concept",
        "compiled_body": "The initially accepted body."
      }]
    }
  }));
  let result = send_graph_chat_turn_with_runtime(&paths, &runtime, "Add an editable concept.", Vec::new()).unwrap();
  server.join().unwrap();
  let patch_id = result["patch_import_result"]["patchId"].as_str().unwrap();

  let mut store = WorkspaceStore::open(&paths.database_path).unwrap();
  let node_id = store.load_graph_snapshot().unwrap()["nodes"][0]["id"].as_str().unwrap().to_string();
  store.update_node_body(&node_id, "The user edited this body after the chat update.").unwrap();

  assert!(store.load_review_queue().unwrap()["latest_undoable_patch"].is_null());
  let error = store.undo_graph_patch(patch_id).unwrap_err();
  assert!(error.message.contains("cannot be undone safely"));
  assert_eq!(
    store.load_graph_node_detail(&node_id).unwrap()["compiled_body"],
    "The user edited this body after the chat update."
  );
  let _ = fs::remove_dir_all(root);
}

#[test]
fn direct_edit_to_new_chat_edge_invalidates_patch_undo() {
  let root = temp_root("soma-chat-undo-direct-edge-edit-test");
  let paths = create_workspace_dir(&root).unwrap();
  let (runtime, server) = local_runtime_with_chat_response(json!({
    "assistant_message": "I connected the two concepts.",
    "used_graph_areas": [],
    "proposed_graph_patch": {
      "proposed_nodes": [{
        "temp_id": "source_concept",
        "type": "concept",
        "title": "Source Concept",
        "compiled_body": "The source concept."
      }, {
        "temp_id": "target_concept",
        "type": "concept",
        "title": "Target Concept",
        "compiled_body": "The target concept."
      }],
      "proposed_edges": [{
        "source_temp_id": "source_concept",
        "target_temp_id": "target_concept",
        "type": "supports",
        "bridge_text": "The original bridge."
      }]
    }
  }));
  let result = send_graph_chat_turn_with_runtime(&paths, &runtime, "Connect two concepts.", Vec::new()).unwrap();
  server.join().unwrap();
  let patch_id = result["patch_import_result"]["patchId"].as_str().unwrap();
  let store = WorkspaceStore::open(&paths.database_path).unwrap();
  let edge_id = store.load_graph_snapshot().unwrap()["edges"][0]["id"].as_str().unwrap().to_string();
  drop(store);

  let conn = open_database(&paths.database_path).unwrap();
  conn
    .execute(
      "UPDATE graph_edges SET bridge_text = ?1, updated_at = ?2 WHERE id = ?3",
      params!["The user-edited bridge.", "2099-01-01T00:00:00Z", edge_id],
    )
    .unwrap();
  drop(conn);

  let mut store = WorkspaceStore::open(&paths.database_path).unwrap();
  assert!(store.load_review_queue().unwrap()["latest_undoable_patch"].is_null());
  let error = store.undo_graph_patch(patch_id).unwrap_err();
  assert!(error.message.contains("cannot be undone safely"));
  assert_eq!(store.load_graph_snapshot().unwrap()["edges"][0]["bridge_text"], "The user-edited bridge.");
  let _ = fs::remove_dir_all(root);
}

#[test]
fn chat_patch_undo_restores_the_previous_node_body_version() {
  let root = temp_root("soma-chat-body-undo-test");
  let paths = workspace_with_source(&root, "User: Keep the original paper explanation.");
  let chunk_id = first_chunk_id(&paths);
  let node_id = seed_active_node_with_body(
    &paths,
    "paper_explanation",
    "Paper Explanation",
    "The original paper explanation.",
    &chunk_id,
  );
  let chat_json = json!({
    "assistant_message": "I expanded the explanation.",
    "used_graph_areas": [],
    "proposed_graph_patch": {
      "proposed_node_body_updates": [{
        "target_node_id": node_id,
        "update_kind": "replace_body",
        "compiled_body": "The expanded paper explanation.",
        "reason": "The user asked for a clearer explanation."
      }]
    }
  });
  let (runtime, server) = local_runtime_with_chat_response(chat_json);
  let result =
    send_graph_chat_turn_with_runtime(&paths, &runtime, "Expand the paper explanation.", vec![node_id.clone()])
      .unwrap();
  server.join().unwrap();
  let patch_id = result["patch_import_result"]["patchId"].as_str().unwrap();

  let mut store = WorkspaceStore::open(&paths.database_path).unwrap();
  assert_eq!(store.load_graph_node_detail(&node_id).unwrap()["compiled_body"], "The expanded paper explanation.");
  store.undo_graph_patch(patch_id).unwrap();
  assert_eq!(store.load_graph_node_detail(&node_id).unwrap()["compiled_body"], "The original paper explanation.");
  let _ = fs::remove_dir_all(root);
}

#[test]
fn node_edit_then_rollback_to_the_chat_version_does_not_reenable_patch_undo() {
  let root = temp_root("soma-chat-body-edit-rollback-undo-test");
  let paths = workspace_with_source(&root, "User: Keep the original paper explanation.");
  let chunk_id = first_chunk_id(&paths);
  let node_id = seed_active_node_with_body(
    &paths,
    "paper_explanation",
    "Paper Explanation",
    "The original paper explanation.",
    &chunk_id,
  );
  let (runtime, server) = local_runtime_with_chat_response(json!({
    "assistant_message": "I expanded the explanation.",
    "used_graph_areas": [],
    "proposed_graph_patch": {
      "proposed_node_body_updates": [{
        "target_node_id": node_id,
        "update_kind": "replace_body",
        "compiled_body": "The expanded paper explanation.",
        "reason": "The user asked for a clearer explanation."
      }]
    }
  }));
  let result =
    send_graph_chat_turn_with_runtime(&paths, &runtime, "Expand the paper explanation.", vec![node_id.clone()])
      .unwrap();
  server.join().unwrap();
  let patch_id = result["patch_import_result"]["patchId"].as_str().unwrap();

  let mut store = WorkspaceStore::open(&paths.database_path).unwrap();
  let accepted_version = store.load_graph_node_detail(&node_id).unwrap()["body_version"].as_i64().unwrap();
  store.update_node_body(&node_id, "The user made a later direct edit.").unwrap();
  store.rollback_node_body(&node_id, accepted_version).unwrap();

  assert_eq!(store.load_graph_node_detail(&node_id).unwrap()["compiled_body"], "The expanded paper explanation.");
  assert!(store.load_review_queue().unwrap()["latest_undoable_patch"].is_null());
  let error = store.undo_graph_patch(patch_id).unwrap_err();
  assert!(error.message.contains("cannot be undone safely"));
  assert_eq!(store.load_graph_node_detail(&node_id).unwrap()["compiled_body"], "The expanded paper explanation.");
  let _ = fs::remove_dir_all(root);
}

#[test]
fn chat_patch_undo_removes_only_evidence_inserted_by_an_edge_bridge_update() {
  let root = temp_root("soma-chat-edge-bridge-evidence-undo-test");
  let paths =
    workspace_with_source(&root, "User: The original and revised bridge explanations both have source evidence.");
  let chunk_id = first_chunk_id(&paths);
  let (_, _, edge_id) = seed_connected_active_nodes(
    &paths,
    "edge_source",
    "Edge Source",
    "The source side of the evidence-preservation test.",
    &chunk_id,
    "edge_target",
    "Edge Target",
    "The target side of the evidence-preservation test.",
    &chunk_id,
    "The original bridge remains recoverable.",
  );
  let before = WorkspaceStore::open(&paths.database_path).unwrap().load_graph_snapshot().unwrap();
  let edge_before = before["edges"].as_array().unwrap().iter().find(|edge| edge["id"] == edge_id).unwrap();
  let base_updated_at = edge_before["updated_at"].as_str().unwrap().to_string();
  let original_bridge = edge_before["bridge_text"].as_str().unwrap().to_string();
  let original_chunk_evidence = edge_evidence_ids(&paths, "graph_evidence", &edge_id);
  let original_message_evidence = edge_evidence_ids(&paths, "graph_message_evidence", &edge_id);

  let mut store = WorkspaceStore::open(&paths.database_path).unwrap();
  let message =
    store.append_graph_message("Use this message and source chunk for the revised bridge.", Vec::new()).unwrap();
  let message_id = message["message"]["id"].as_str().unwrap();
  let mut patch = empty_graph_patch();
  patch["proposed_edge_bridge_updates"] = json!([{
    "target_edge_id": edge_id,
    "base_edge_updated_at": base_updated_at,
    "bridge_text": "The revised bridge is backed by both exact evidence paths.",
    "source_chunk_ids": [chunk_id],
    "source_message_ids": [message_id],
    "reason": "Exercise complete edge-bridge patch undo."
  }]);
  let proposed = store.propose_graph_updates(message_id, patch).unwrap();
  let patch_id = proposed["patchId"].as_str().unwrap().to_string();
  let accepted = store.accept_graph_patch_proposals(&patch_id, Some("edge evidence undo regression")).unwrap();
  assert_eq!(accepted["acceptedCount"], 1);
  assert!(accepted["errors"].as_array().unwrap().is_empty());

  let inserted_chunk_evidence =
    inserted_ids(&original_chunk_evidence, &edge_evidence_ids(&paths, "graph_evidence", &edge_id));
  let inserted_message_evidence =
    inserted_ids(&original_message_evidence, &edge_evidence_ids(&paths, "graph_message_evidence", &edge_id));
  assert_eq!(inserted_chunk_evidence.len(), 1);
  assert_eq!(inserted_message_evidence.len(), 1);

  store.undo_graph_patch(&patch_id).unwrap();

  let after = store.load_graph_snapshot().unwrap();
  let edge_after = after["edges"].as_array().unwrap().iter().find(|edge| edge["id"] == edge_id).unwrap();
  assert_eq!(edge_after["bridge_text"], original_bridge);
  assert_eq!(edge_evidence_ids(&paths, "graph_evidence", &edge_id), original_chunk_evidence);
  assert_eq!(edge_evidence_ids(&paths, "graph_message_evidence", &edge_id), original_message_evidence);
  let _ = fs::remove_dir_all(root);
}

#[test]
fn graph_chat_turn_sends_only_retrieved_node_bodies_and_evidence_to_runtime() {
  let root = temp_root("soma-graph-chat-context-test");
  let paths = create_workspace_dir(&root).unwrap();
  let alpha_source = root.join("alpha.md");
  let off_topic_source = root.join("offtopic.md");
  fs::write(&alpha_source, "User: ALPHA_EVIDENCE_INCLUDED. Alpha retrieval belongs in the graph.").unwrap();
  fs::write(&off_topic_source, "User: OFFTOPIC_EVIDENCE_SENTINEL. This belongs to another topic.").unwrap();
  import_source_file(&paths, &alpha_source).unwrap();
  import_source_file(&paths, &off_topic_source).unwrap();
  let alpha_chunk_id = chunk_id_containing(&paths, "ALPHA_EVIDENCE_INCLUDED");
  let off_topic_chunk_id = chunk_id_containing(&paths, "OFFTOPIC_EVIDENCE_SENTINEL");
  let alpha_node_id = seed_active_node_with_body(
    &paths,
    "node_alpha_retrieval",
    "Alpha Retrieval",
    "ALPHA_BODY_INCLUDED. Alpha retrieval should be available to the brain.",
    &alpha_chunk_id,
  );
  seed_active_node_with_body(
    &paths,
    "node_offtopic",
    "Unrelated Archive",
    "OFFTOPIC_FULL_BODY_SENTINEL. This body should not be sent.",
    &off_topic_chunk_id,
  );
  let chat_json = json!({
    "assistant_message": "Alpha retrieval is the relevant graph area.",
    "used_graph_areas": [],
    "proposed_graph_patch": null
  });
  let (runtime, server, request_received) = local_runtime_with_captured_chat_request(chat_json);

  let result = send_graph_chat_turn_with_runtime(&paths, &runtime, "alpha retrieval question", Vec::new()).unwrap();
  let request_body = request_received.recv().unwrap();
  server.join().unwrap();

  assert!(request_body.contains("ALPHA_BODY_INCLUDED"));
  assert!(request_body.contains("ALPHA_EVIDENCE_INCLUDED"));
  assert!(!request_body.contains("OFFTOPIC_FULL_BODY_SENTINEL"));
  assert!(!request_body.contains("OFFTOPIC_EVIDENCE_SENTINEL"));
  let bodies = result["context_packet"]["top_matching_node_bodies"].as_array().unwrap();
  assert_eq!(bodies.len(), 1, "{result:#}");
  assert_eq!(bodies[0]["id"], alpha_node_id);
  let evidence = result["context_packet"]["source_evidence_excerpts"].as_array().unwrap();
  assert!(!evidence.is_empty(), "{result:#}");
  assert!(evidence.iter().all(|item| item["entity_id"] == alpha_node_id));
  let _ = fs::remove_dir_all(root);
}

#[test]
fn graph_chat_filters_runtime_used_areas_to_server_known_refs_before_persisting() {
  let root = temp_root("soma-graph-chat-used-area-validation-test");
  let paths = workspace_with_source(&root, "User: Alpha retrieval is the relevant graph context.");
  let chunk_id = first_chunk_id(&paths);
  let node_id = seed_active_node_with_body(
    &paths,
    "node_alpha_used_area",
    "Alpha Retrieval",
    "Alpha retrieval is the server-known context for this turn.",
    &chunk_id,
  );
  let chat_json = json!({
    "assistant_message": "Alpha retrieval is the only graph area used in this answer.",
    "used_graph_areas": [
      { "id": node_id, "title": 42, "type": ["malformed"] },
      { "id": node_id, "title": "Forged duplicate title", "type": "task" },
      { "id": "node_not_in_context", "title": "Unknown Area", "type": "concept" },
      { "title": "Missing id" },
      "not an area"
    ],
    "proposed_graph_patch": null
  });
  let (runtime, server) = local_runtime_with_chat_response(chat_json);

  let result = send_graph_chat_turn_with_runtime(&paths, &runtime, "Explain alpha retrieval.", Vec::new()).unwrap();
  server.join().unwrap();

  let used_areas = result["used_graph_areas"].as_array().unwrap();
  assert_eq!(used_areas.len(), 1, "{result:#}");
  assert_eq!(used_areas[0]["id"], node_id);
  assert_eq!(used_areas[0]["title"], "Alpha Retrieval");
  assert_eq!(used_areas[0]["type"], "concept");
  assert_eq!(result["context_packet"]["used_graph_areas"], result["used_graph_areas"]);

  let assistant_id = result["assistant_message"]["id"].as_str().unwrap();
  let conn = open_database(&paths.database_path).unwrap();
  let stored_context: String = conn
    .query_row("SELECT context_json FROM graph_thread_messages WHERE id = ?1", params![assistant_id], |row| row.get(0))
    .unwrap();
  let stored_context: Value = serde_json::from_str(&stored_context).unwrap();
  assert_eq!(stored_context["used_graph_areas"], result["used_graph_areas"]);
  let _ = fs::remove_dir_all(root);
}

#[test]
fn graph_chat_turn_releases_database_while_runtime_is_waiting() {
  let root = temp_root("soma-graph-chat-lock-test");
  let paths = workspace_with_source(
    &root,
    concat!(
      "User: Graph chat should not hold the database while the brain thinks.\n\n",
      "Assistant: Other workspace writes should still work.",
    ),
  );
  let chat_json = json!({
    "assistant_message": "The runtime answer can be saved after another workspace write.",
    "used_graph_areas": [],
    "proposed_graph_patch": null
  });
  let (runtime, server, request_received, release_response) = pausing_runtime_with_chat_response(chat_json);
  let chat_paths = paths.clone();
  let chat_thread = thread::spawn(move || {
    send_graph_chat_turn_with_runtime(&chat_paths, &runtime, "Explain the lock behavior.", Vec::new()).unwrap()
  });

  request_received.recv().unwrap();
  let mut store = WorkspaceStore::open(&paths.database_path).unwrap();
  store.append_graph_message("This write should not see database is locked.", Vec::new()).unwrap();
  drop(store);

  release_response.send(()).unwrap();
  let result = chat_thread.join().unwrap();
  server.join().unwrap();

  assert_eq!(result["runtime_status"], "completed", "{result:#}");
  assert!(result["assistant_message"]["content"].as_str().unwrap().contains("runtime answer"));
  let _ = fs::remove_dir_all(root);
}

#[test]
fn graph_chat_failure_result_hides_raw_storage_lock_message() {
  let result = chat_turn_failure_result(
    json!({
      "message": {
        "id": "graph_msg_locked",
        "role": "user",
        "content": "Why did this lock?",
        "created_at": "2026-07-06T00:00:00.000Z"
      }
    }),
    json!({ "used_graph_areas": [] }),
    RuntimeChatTurnResult {
      adapter_kind: "codex_sdk_profile".to_string(),
      status: "failed",
      failure_kind: Some(RuntimeFailureKind::Busy),
      message: "Runtime command exited with status 1. Error: database is locked".to_string(),
      assistant_message: None,
      used_graph_areas: Vec::new(),
      proposed_graph_patch: None,
    },
    no_patch_result(),
  );

  assert_eq!(result["runtime_message"], "Soma is busy finishing another local write. Try again in a moment.");
  assert_eq!(result["runtime_failure_kind"], "busy");
  assert_eq!(result["error"], result["runtime_message"]);
  assert!(!result.to_string().contains("database is locked"));
}

#[test]
fn graph_chat_turn_does_not_mutate_graph_when_runtime_storage_is_busy() {
  let root = temp_root("soma-runtime-storage-busy-test");
  let paths = create_workspace_dir(&root).unwrap();
  let (runtime, server) = local_runtime_with_chat_response(json!("Error: database is locked"));

  let result = send_graph_chat_turn_with_runtime(
    &paths,
    &runtime,
    "Compare deep learning, machine learning, and a jigsaw puzzle as connected ideas.",
    Vec::new(),
  )
  .unwrap();
  server.join().unwrap();

  assert_eq!(result["assistant_message"], Value::Null);
  assert_eq!(result["runtime_message"], "Soma is busy finishing another local write. Try again in a moment.");
  assert_eq!(result["runtime_failure_kind"], "busy");
  assert_eq!(result["patch_import_status"], "none");
  assert_eq!(result["proposal_count"], 0);
  assert!(!result.to_string().contains("database is locked"));

  let store = WorkspaceStore::open(&paths.database_path).unwrap();
  let graph = store.load_graph_snapshot().unwrap();
  assert!(graph["nodes"].as_array().unwrap().is_empty());
  assert!(graph["edges"].as_array().unwrap().is_empty());
  let queue = store.load_review_queue().unwrap();
  assert!(queue["items"].as_array().unwrap().is_empty());
  let _ = fs::remove_dir_all(root);
}

#[test]
fn graph_chat_turn_creates_message_backed_graph_in_empty_workspace() {
  let root = temp_root("soma-empty-chat-graph-test");
  let paths = create_workspace_dir(&root).unwrap();
  let chat_json = json!({
    "assistant_message": "I mapped this as a new investigation thread.",
    "used_graph_areas": [],
    "proposed_graph_patch": {
      "proposed_nodes": [{
        "temp_id": "node_alley_timeline",
        "type": "question",
        "title": "Alley Timeline",
        "preview": "Open question about what happened in the alley.",
        "compiled_body": concat!(
          "The conversation introduces an investigation thread about reconstructing the alley ",
          "timeline from available observations.",
        ),
        "reason": "The current chat message starts a durable investigation thread."
      }]
    }
  });
  let (runtime, server) = local_runtime_with_chat_response(chat_json);

  let result =
    send_graph_chat_turn_with_runtime(&paths, &runtime, "Investigate what happened in the alley timeline.", Vec::new())
      .unwrap();
  server.join().unwrap();

  assert_eq!(result["patch_import_status"], "accepted_to_graph");
  let store = WorkspaceStore::open(&paths.database_path).unwrap();
  let graph = store.load_graph_snapshot().unwrap();
  assert_eq!(graph["nodes"].as_array().unwrap().len(), 1);
  assert_eq!(graph["nodes"][0]["title"], "Alley Timeline");
  assert!(graph["nodes"][0]["source_chunk_ids"].as_array().unwrap().is_empty());
  assert_eq!(graph["nodes"][0]["evidence"][0]["message_id"], result["user_message"]["id"]);
  let queue = store.load_review_queue().unwrap();
  assert_eq!(queue["items"][0]["source_message_id"], result["assistant_message"]["id"]);
  let _ = fs::remove_dir_all(root);
}

#[test]
fn graph_chat_turn_does_not_mutate_graph_when_runtime_returns_no_patch() {
  let root = temp_root("soma-empty-chat-no-patch-test");
  let paths = create_workspace_dir(&root).unwrap();
  let chat_json = json!({
    "assistant_message": concat!(
      "The alley timeline should start with entry points, witness statements, and camera ",
      "blind spots.",
    ),
    "used_graph_areas": [],
    "proposed_graph_patch": null
  });
  let (runtime, server) = local_runtime_with_chat_response(chat_json);

  let result = send_graph_chat_turn_with_runtime(
    &paths,
    &runtime,
    "Investigate the alley timeline for missing evidence.",
    Vec::new(),
  )
  .unwrap();
  server.join().unwrap();

  assert_eq!(result["patch_import_status"], "none");
  assert_eq!(result["proposal_count"], 0);
  assert_eq!(
    result["assistant_message"]["content"],
    "The alley timeline should start with entry points, witness statements, and camera blind spots."
  );
  let store = WorkspaceStore::open(&paths.database_path).unwrap();
  let graph = store.load_graph_snapshot().unwrap();
  assert!(graph["nodes"].as_array().unwrap().is_empty());
  assert!(graph["edges"].as_array().unwrap().is_empty());
  let _ = fs::remove_dir_all(root);
}

#[test]
fn graph_chat_turn_keeps_the_runtime_patch_without_heuristic_replacement() {
  let root = temp_root("soma-sparse-runtime-patch-test");
  let paths = create_workspace_dir(&root).unwrap();
  let chat_json = json!({
    "assistant_message": concat!(
      "Deep learning belongs under machine learning, and the jigsaw puzzle analogy explains ",
      "how the ideas connect.",
    ),
    "used_graph_areas": [],
    "proposed_graph_patch": {
      "proposed_nodes": [{
        "temp_id": "node_learning_comparison",
        "type": "concept",
        "title": "Learning Comparison",
        "preview": "A broad comparison of learning-related ideas.",
        "compiled_body": concat!(
          "The runtime collapsed deep learning, machine learning, and the jigsaw puzzle analogy ",
          "into one broad node.",
        ),
        "reason": "Sparse runtime patch regression fixture."
      }],
      "proposed_edges": []
    }
  });
  let (runtime, server) = local_runtime_with_chat_response(chat_json);

  let result = send_graph_chat_turn_with_runtime(
    &paths,
    &runtime,
    "Compare deep learning, machine learning, and a jigsaw puzzle as connected ideas for learning.",
    Vec::new(),
  )
  .unwrap();
  server.join().unwrap();

  assert_eq!(result["patch_import_status"], "accepted_to_graph");
  assert_eq!(result["proposal_count"], 1);
  let store = WorkspaceStore::open(&paths.database_path).unwrap();
  let graph = store.load_graph_snapshot().unwrap();
  let nodes = graph["nodes"].as_array().unwrap();
  assert_eq!(nodes.len(), 1);
  assert_eq!(nodes[0]["title"], "Learning Comparison");
  assert!(graph["edges"].as_array().unwrap().is_empty());
  let _ = fs::remove_dir_all(root);
}

#[test]
fn graph_chat_turn_preserves_existing_graph_when_focused_turn_returns_no_patch() {
  let root = temp_root("soma-focused-chat-no-patch-test");
  let paths = workspace_with_source(&root, "User: The active case node stores the original investigation scope.");
  let chunk_id = first_chunk_id(&paths);
  let (existing_node_id, _) = seed_active_node(&paths, &chunk_id);
  let chat_json = json!({
    "assistant_message": concat!(
      "The license plate lead should become a separate working thread tied to the current ",
      "investigation.",
    ),
    "used_graph_areas": [{
      "id": existing_node_id,
      "title": "Node Chat",
      "type": "concept"
    }],
    "proposed_graph_patch": null
  });
  let (runtime, server) = local_runtime_with_chat_response(chat_json);

  let result = send_graph_chat_turn_with_runtime(
    &paths,
    &runtime,
    "Investigate the license plate lead for the missing witness.",
    vec![existing_node_id.clone()],
  )
  .unwrap();
  server.join().unwrap();

  assert_eq!(result["patch_import_status"], "none");
  assert_eq!(result["proposal_count"], 0);
  let store = WorkspaceStore::open(&paths.database_path).unwrap();
  let graph = store.load_graph_snapshot().unwrap();
  assert_eq!(graph["nodes"].as_array().unwrap().len(), 1);
  assert_eq!(graph["nodes"][0]["id"], existing_node_id);
  assert!(graph["edges"].as_array().unwrap().is_empty());
  let _ = fs::remove_dir_all(root);
}

#[test]
fn graph_chat_turn_accepts_camel_case_runtime_patch() {
  let root = temp_root("soma-camel-chat-patch-test");
  let paths = create_workspace_dir(&root).unwrap();
  let chat_json = json!({
    "assistant_message": "I mapped the alias-shaped runtime patch.",
    "usedGraphAreas": [],
    "graphPatch": {
      "schemaVersion": 1,
      "proposedNodes": [{
        "temp_id": "node_alias_patch",
        "type": "question",
        "title": "Alias Patch",
        "preview": "Runtime output used camelCase graph patch fields.",
        "compiled_body": concat!(
          "The chat runtime returned graphPatch and proposedNodes. Soma should normalize those ",
          "aliases before review and auto-acceptance.",
        ),
        "reason": "The current chat message asks Soma to map parser aliases."
      }],
      "proposedEdges": [],
      "proposedNodeBodyUpdates": [],
      "proposedEdgeBridgeUpdates": [],
      "proposedMessageEvidenceAttachments": [],
      "proposedPaths": [],
      "ambiguities": [],
      "mergeCandidates": [],
      "warnings": []
    }
  });
  let (runtime, server) = local_runtime_with_chat_response(chat_json);

  let result =
    send_graph_chat_turn_with_runtime(&paths, &runtime, "Investigate parser aliases in graph chat.", Vec::new())
      .unwrap();
  server.join().unwrap();

  assert_eq!(result["patch_import_status"], "accepted_to_graph");
  let store = WorkspaceStore::open(&paths.database_path).unwrap();
  let graph = store.load_graph_snapshot().unwrap();
  assert_eq!(graph["nodes"].as_array().unwrap().len(), 1);
  assert_eq!(graph["nodes"][0]["title"], "Alias Patch");
  let _ = fs::remove_dir_all(root);
}

#[test]
fn graph_chat_turn_reports_invalid_runtime_patch_without_mutating_graph() {
  let root = temp_root("soma-invalid-runtime-patch-test");
  let paths = create_workspace_dir(&root).unwrap();
  let chat_json = json!({
    "assistant_message": "I can still answer even if the proposed patch is malformed.",
    "used_graph_areas": [],
    "proposed_graph_patch": {
      "proposed_nodes": [{
        "temp_id": "node_invalid_runtime_patch",
        "type": "concept",
        "title": "Invalid Runtime Patch",
        "reason": "This deliberately omits compiled_body."
      }]
    }
  });
  let (runtime, server) = local_runtime_with_chat_response(chat_json);

  let result = send_graph_chat_turn_with_runtime(
    &paths,
    &runtime,
    "Compare deep learning and machine learning as connected ideas.",
    Vec::new(),
  )
  .unwrap();
  server.join().unwrap();

  assert_eq!(result["patch_import_status"], "invalid");
  assert_eq!(result["patch_import_result"]["valid"], false);
  assert_eq!(result["proposal_count"], 0);
  assert!(!result["patch_import_result"]["errors"].as_array().unwrap().is_empty());
  let store = WorkspaceStore::open(&paths.database_path).unwrap();
  let graph = store.load_graph_snapshot().unwrap();
  assert!(graph["nodes"].as_array().unwrap().is_empty());
  assert!(graph["edges"].as_array().unwrap().is_empty());
  let _ = fs::remove_dir_all(root);
}

#[test]
fn graph_chat_turn_rejects_scalar_patch_items_without_poisoning_writes() {
  let root = temp_root("soma-scalar-runtime-patch-test");
  let paths = create_workspace_dir(&root).unwrap();
  let (runtime, server) = local_runtime_with_chat_response(json!({
    "assistant_message": "The answer remains available even though the update shape is invalid.",
    "used_graph_areas": [],
    "proposed_graph_patch": {
      "proposed_nodes": [1]
    }
  }));

  let result = send_graph_chat_turn_with_runtime(&paths, &runtime, "Return a malformed update.", Vec::new()).unwrap();
  server.join().unwrap();

  assert_eq!(result["patch_import_status"], "invalid", "{result:#}");
  let mut store = WorkspaceStore::open(&paths.database_path).unwrap();
  store.append_graph_message("A later write still succeeds.", Vec::new()).unwrap();
  let _ = fs::remove_dir_all(root);
}

#[test]
fn graph_chat_turn_reports_a_non_object_patch_as_invalid() {
  let root = temp_root("soma-non-object-runtime-patch-test");
  let paths = create_workspace_dir(&root).unwrap();
  let (runtime, server) = local_runtime_with_chat_response(json!({
    "assistant_message": "The answer remains available even though the patch is not an object.",
    "used_graph_areas": [],
    "proposed_graph_patch": "not a graph patch"
  }));

  let result = send_graph_chat_turn_with_runtime(&paths, &runtime, "Return a malformed patch.", Vec::new()).unwrap();
  server.join().unwrap();

  assert_eq!(result["patch_import_status"], "invalid", "{result:#}");
  assert_eq!(result["patch_import_result"]["valid"], false);
  assert!(result["assistant_message"]["content"].as_str().unwrap().contains("answer remains available"));
  let _ = fs::remove_dir_all(root);
}

#[test]
fn graph_chat_attaches_message_evidence_to_merge_candidates() {
  let root = temp_root("soma-chat-merge-evidence-test");
  let paths = workspace_with_source(&root, "Alpha and beta describe the same durable idea.");
  let chunk_id = first_chunk_id(&paths);
  let alpha_id = seed_active_node_with_body(&paths, "alpha", "Alpha", "Alpha is one description.", &chunk_id);
  let beta_id = seed_active_node_with_body(&paths, "beta", "Beta", "Beta is an overlapping description.", &chunk_id);
  let (runtime, server) = local_runtime_with_chat_response(json!({
    "assistant_message": "These concepts overlap and should be reviewed as a merge.",
    "used_graph_areas": [],
    "proposed_graph_patch": {
      "merge_candidates": [{
        "candidate_node_ids": [alpha_id, beta_id],
        "reason": "The current message identifies the overlap."
      }]
    }
  }));

  let result = send_graph_chat_turn_with_runtime(&paths, &runtime, "These concepts overlap.", Vec::new()).unwrap();
  server.join().unwrap();

  assert_eq!(result["patch_import_status"], "imported_to_review", "{result:#}");
  assert_eq!(result["patch_import_result"]["valid"], true);
  assert_eq!(result["proposal_count"], 1);
  let queue = WorkspaceStore::open(&paths.database_path).unwrap().load_review_queue().unwrap();
  let merge = queue["items"].as_array().unwrap().iter().find(|item| item["type"] == "merge_candidate").unwrap();
  assert_eq!(merge["status"], "proposed");
  assert!(merge["evidence_count"].as_i64().unwrap() >= 1);
  assert!(merge["evidence_refs"].as_array().unwrap().iter().any(|evidence| evidence["type"] == "message"));
  let _ = fs::remove_dir_all(root);
}

#[test]
fn graph_chat_turn_auto_applies_valid_graph_objects_without_manual_review() {
  let root = temp_root("soma-direct-chat-auto-apply-test");
  let paths = create_workspace_dir(&root).unwrap();
  let chat_json = json!({
    "assistant_message": "I created a graph node for this investigation thread.",
    "used_graph_areas": [],
    "proposed_graph_patch": {
      "proposed_nodes": [{
        "temp_id": "node_rooftop_sighting",
        "type": "claim",
        "title": "Rooftop Sighting",
        "preview": "A witness reported movement on the rooftop.",
        "compiled_body": concat!(
          "The conversation records a claim that a witness saw movement on the rooftop before ",
          "the incident.",
        ),
        "reason": "The current message introduces a durable claim for the investigation graph."
      }]
    }
  });
  let (runtime, server) = local_runtime_with_chat_response(chat_json);

  let result = send_graph_chat_turn_with_runtime(
    &paths,
    &runtime,
    "Witness says there was movement on the rooftop before the incident.",
    Vec::new(),
  )
  .unwrap();
  server.join().unwrap();

  assert_eq!(result["patch_import_status"], "accepted_to_graph");
  assert_eq!(result["patch_import_result"]["proposal_status"], "proposed");
  let store = WorkspaceStore::open(&paths.database_path).unwrap();
  let graph = store.load_graph_snapshot().unwrap();
  assert_eq!(graph["nodes"].as_array().unwrap().len(), 1);
  assert_eq!(graph["nodes"][0]["title"], "Rooftop Sighting");
  let queue = store.load_review_queue().unwrap();
  assert_eq!(queue["items"][0]["status"], "accepted");
  let _ = fs::remove_dir_all(root);
}

#[test]
fn graph_chat_turn_auto_applies_runtime_multi_concept_edges() {
  let root = temp_root("soma-direct-chat-connected-runtime-test");
  let paths = create_workspace_dir(&root).unwrap();
  let chat_json = json!({
    "assistant_message": "I mapped the learning concepts and connected them.",
    "used_graph_areas": [],
    "proposed_graph_patch": {
      "proposed_nodes": [{
        "temp_id": "node_deep_learning",
        "type": "concept",
        "title": "Deep Learning",
        "preview": "A machine learning approach based on layered representation learning.",
        "compiled_body": concat!(
          "Deep learning was introduced as a durable concept in the current chat turn. It should ",
          "be represented separately from the broader machine learning category.",
        ),
        "reason": "The current message names deep learning as a distinct idea."
      }, {
        "temp_id": "node_machine_learning",
        "type": "concept",
        "title": "Machine Learning",
        "preview": "The broader field that includes deep learning.",
        "compiled_body": concat!(
          "Machine learning was introduced as a durable concept in the current chat turn. It gives ",
          "the graph a broader category for the deep learning concept.",
        ),
        "reason": "The current message names machine learning as a distinct idea."
      }, {
        "temp_id": "node_jigsaw_puzzle",
        "type": "concept",
        "title": "Jigsaw Puzzle",
        "preview": "An analogy for assembling connected ideas.",
        "compiled_body": concat!(
          "A jigsaw puzzle was introduced as an analogy for connecting learning concepts into a ",
          "coherent structure.",
        ),
        "reason": "The current message uses a jigsaw puzzle as a distinct analogy."
      }],
      "proposed_edges": [{
        "source_temp_id": "node_deep_learning",
        "target_temp_id": "node_machine_learning",
        "type": "part_of",
        "bridge_text": "Deep learning is part of machine learning.",
        "reason": "The current message relates deep learning to the broader machine learning category."
      }, {
        "source_temp_id": "node_jigsaw_puzzle",
        "target_temp_id": "node_deep_learning",
        "type": "mentions",
        "bridge_text": "The puzzle analogy helps explain connected learning concepts.",
        "reason": "The current message uses the jigsaw puzzle analogy alongside deep learning."
      }]
    }
  });
  let (runtime, server) = local_runtime_with_chat_response(chat_json);

  let result = send_graph_chat_turn_with_runtime(
    &paths,
    &runtime,
    "Connect deep learning, machine learning, and a jigsaw puzzle as ideas.",
    Vec::new(),
  )
  .unwrap();
  server.join().unwrap();

  assert_eq!(result["patch_import_status"], "accepted_to_graph");
  assert_eq!(result["proposal_count"], 5);
  let store = WorkspaceStore::open(&paths.database_path).unwrap();
  let graph = store.load_graph_snapshot().unwrap();
  assert_eq!(graph["nodes"].as_array().unwrap().len(), 3);
  assert_eq!(graph["edges"].as_array().unwrap().len(), 2);
  assert!(graph["edges"].as_array().unwrap().iter().any(|edge| edge["type"] == "part_of"));
  for node in graph["nodes"].as_array().unwrap() {
    assert_eq!(node["evidence"][0]["message_id"], result["user_message"]["id"]);
  }
  for edge in graph["edges"].as_array().unwrap() {
    assert_eq!(edge["evidence"][0]["message_id"], result["user_message"]["id"]);
  }
  let _ = fs::remove_dir_all(root);
}

#[test]
fn graph_chat_turn_rolls_back_the_entire_patch_when_auto_apply_fails() {
  let root = temp_root("soma-direct-chat-partial-auto-apply-test");
  let paths = create_workspace_dir(&root).unwrap();
  let chat_json = json!({
    "assistant_message": "I created one graph object and one update that needs attention.",
    "used_graph_areas": [],
    "proposed_graph_patch": {
      "proposed_nodes": [{
        "temp_id": "node_valid_thread",
        "type": "task",
        "title": "Valid Thread",
        "preview": "A valid thread from the current chat.",
        "compiled_body": "The conversation introduces a valid investigation thread that can be saved to the graph.",
        "reason": "The current chat message supports this graph object."
      }, {
        "temp_id": "node_missing_evidence",
        "type": "claim",
        "title": "Missing Evidence",
        "preview": "A proposal with a missing chunk id.",
        "compiled_body": concat!(
          "This proposal is syntactically valid, but its declared source chunk is not present ",
          "in storage.",
        ),
        "source_chunk_ids": ["chunk_missing"],
        "reason": "The missing chunk should keep the patch from reporting full success."
      }]
    }
  });
  let (runtime, server) = local_runtime_with_chat_response(chat_json);

  let result = send_graph_chat_turn_with_runtime(
    &paths,
    &runtime,
    "Create a graph thread, but also cite a missing source chunk.",
    Vec::new(),
  )
  .unwrap();
  server.join().unwrap();

  assert_eq!(result["patch_import_status"], "imported_to_review");
  assert_eq!(result["patch_import_result"]["trusted"], false);
  assert_eq!(result["patch_import_result"]["autoAcceptResult"]["acceptedCount"], 0);
  assert_eq!(result["patch_import_result"]["autoAcceptResult"]["errors"].as_array().unwrap().len(), 1);

  let mut store = WorkspaceStore::open(&paths.database_path).unwrap();
  let graph = store.load_graph_snapshot().unwrap();
  assert!(graph["nodes"].as_array().unwrap().is_empty());
  assert!(graph["edges"].as_array().unwrap().is_empty());
  let queue = store.load_review_queue().unwrap();
  let items = queue["items"].as_array().unwrap();
  assert_eq!(items.len(), 2);
  assert!(items.iter().all(|item| item["status"] == "proposed"));
  assert!(queue["latest_undoable_patch"].is_null());
  let patch_id = result["patch_import_result"]["patchId"].as_str().unwrap();
  assert!(store.undo_graph_patch(patch_id).is_err());
  let _ = fs::remove_dir_all(root);
}

#[test]
fn graph_chat_turn_auto_applies_message_evidence_attachments() {
  let root = temp_root("soma-direct-chat-evidence-attachment-test");
  let paths = workspace_with_source(
    &root,
    "User: Alpha evidence should support the compiled graph.\n\nAssistant: Alpha evidence belongs on the active node.",
  );
  let chunk_id = first_chunk_id(&paths);
  let (node_id, _) = seed_active_node(&paths, &chunk_id);
  let evidence_message_id = {
    let mut store = WorkspaceStore::open(&paths.database_path).unwrap();
    let message =
      store.append_graph_message("Alpha evidence should become direct graph evidence.", Vec::new()).unwrap();
    message["message"]["id"].as_str().unwrap().to_string()
  };
  let chat_json = json!({
    "assistant_message": "I attached that graph message as direct evidence.",
    "used_graph_areas": [{
      "id": node_id,
      "title": "Node Chat",
      "type": "concept"
    }],
    "proposed_graph_patch": {
      "proposed_message_evidence_attachments": [{
        "message_id": evidence_message_id,
        "target_entity_type": "node",
        "target_entity_id": node_id,
        "quote_excerpt": "Alpha evidence should become direct graph evidence.",
        "reason": "The graph message directly supports the active node."
      }]
    }
  });
  let (runtime, server) = local_runtime_with_chat_response(chat_json);

  let result =
    send_graph_chat_turn_with_runtime(&paths, &runtime, "Attach the alpha evidence to the graph.", Vec::new()).unwrap();
  server.join().unwrap();

  assert_eq!(result["patch_import_status"], "accepted_to_graph");
  assert_eq!(result["proposal_count"], 1);
  let store = WorkspaceStore::open(&paths.database_path).unwrap();
  let queue = store.load_review_queue().unwrap();
  let evidence_proposal =
    queue["items"].as_array().unwrap().iter().find(|item| item["type"] == "message_evidence_attachment").unwrap();
  assert_eq!(evidence_proposal["status"], "accepted");
  let graph = store.load_graph_snapshot().unwrap();
  let evidence = graph["nodes"][0]["evidence"].as_array().unwrap();
  assert!(evidence.iter().any(|item| item["message_id"].as_str() == Some(evidence_message_id.as_str())));
  let _ = fs::remove_dir_all(root);
}

#[test]
fn graph_chat_turn_rejects_stale_body_version_evidence_attachment() {
  let root = temp_root("soma-stale-body-evidence-attachment-test");
  let paths = workspace_with_source(
    &root,
    concat!(
      "User: Alpha evidence should support only the current body.\n\n",
      "Assistant: Body evidence must not attach to stale versions.",
    ),
  );
  let chunk_id = first_chunk_id(&paths);
  let (node_id, _) = seed_active_node(&paths, &chunk_id);
  let (stale_body_version_id, evidence_message_id) = {
    let mut store = WorkspaceStore::open(&paths.database_path).unwrap();
    let graph = store.load_graph_snapshot().unwrap();
    let stale_body_version_id = graph["nodes"][0]["body_version_id"].as_str().unwrap().to_string();
    store.update_node_body(&node_id, "The current body has replaced the original version.").unwrap();
    let message = store.append_graph_message("This message should not attach to the stale body.", Vec::new()).unwrap();
    (stale_body_version_id, message["message"]["id"].as_str().unwrap().to_string())
  };
  let chat_json = json!({
    "assistant_message": "I tried to attach evidence to an old body version.",
    "used_graph_areas": [{
      "id": node_id,
      "title": "Node Chat",
      "type": "concept"
    }],
    "proposed_graph_patch": {
      "proposed_message_evidence_attachments": [{
        "message_id": evidence_message_id,
        "target_entity_type": "node_body_version",
        "target_entity_id": stale_body_version_id,
        "quote_excerpt": "stale body",
        "reason": "Regression fixture for stale body evidence."
      }]
    }
  });
  let (runtime, server) = local_runtime_with_chat_response(chat_json);

  let result =
    send_graph_chat_turn_with_runtime(&paths, &runtime, "Attach this to the old body version.", Vec::new()).unwrap();
  server.join().unwrap();

  assert_eq!(result["patch_import_status"], "imported_to_review");
  assert_eq!(result["proposal_count"], 1);
  assert_eq!(result["patch_import_result"]["autoAcceptResult"]["acceptedCount"], 0);
  assert_eq!(result["patch_import_result"]["autoAcceptResult"]["errors"].as_array().unwrap().len(), 1);
  let store = WorkspaceStore::open(&paths.database_path).unwrap();
  let queue = store.load_review_queue().unwrap();
  let proposal =
    queue["items"].as_array().unwrap().iter().find(|item| item["type"] == "message_evidence_attachment").unwrap();
  assert_ne!(proposal["status"], "accepted");
  let _ = fs::remove_dir_all(root);
}

#[test]
fn graph_chat_turn_keeps_ambiguous_patch_in_review() {
  let root = temp_root("soma-ambiguous-chat-graph-test");
  let paths = create_workspace_dir(&root).unwrap();
  let chat_json = json!({
    "assistant_message": "This could become a graph node, but the target is ambiguous.",
    "used_graph_areas": [],
    "proposed_graph_patch": {
      "proposed_nodes": [{
        "temp_id": "node_unclear_case",
        "type": "question",
        "title": "Unclear Case",
        "preview": "Unclear investigation direction.",
        "compiled_body": concat!(
          "The conversation introduces a possible investigation direction, but the graph target ",
          "is still ambiguous.",
        ),
        "reason": "The current chat message introduces a possible graph object."
      }],
      "ambiguities": [{
        "kind": "unclear_node_target",
        "prompt": "Should this become a new node or attach to an existing investigation thread?"
      }]
    }
  });
  let (runtime, server) = local_runtime_with_chat_response(chat_json);

  let result = send_graph_chat_turn_with_runtime(
    &paths,
    &runtime,
    "This might be a new case thread, or maybe it belongs under the old one.",
    Vec::new(),
  )
  .unwrap();
  server.join().unwrap();

  assert_eq!(result["patch_import_status"], "imported_to_review");
  assert_eq!(result["patch_import_result"]["trusted"], false);
  assert_eq!(result["proposal_count"], 2);
  let store = WorkspaceStore::open(&paths.database_path).unwrap();
  let graph = store.load_graph_snapshot().unwrap();
  assert!(graph["nodes"].as_array().unwrap().is_empty());
  let queue = store.load_review_queue().unwrap();
  let items = queue["items"].as_array().unwrap();
  assert_eq!(items.len(), 2);
  assert!(items.iter().all(|item| item["status"] == "proposed"));
  assert!(items.iter().any(|item| item["type"] == "node"));
  assert!(items.iter().any(|item| item["type"] == "ambiguity"));
  let _ = fs::remove_dir_all(root);
}

#[test]
fn node_chat_turn_answers_and_applies_body_update() {
  let root = temp_root("soma-node-chat-turn-test");
  let paths = workspace_with_source(
    &root,
    concat!(
      "User: Node chat should enrich the focused compiled section.\n\n",
      "Assistant: The node body changes only after Review Updates accepts the proposal.",
    ),
  );
  let chunk_id = first_chunk_id(&paths);
  let (node_id, original_body) = seed_active_node(&paths, &chunk_id);
  let chat_json = json!({
    "assistant_message": "The focused node can be enriched, but I will keep the body change in Review Updates.",
    "used_graph_areas": [{
      "id": node_id,
      "title": "Node Chat",
      "type": "concept"
    }],
    "proposed_graph_patch": {
      "proposed_node_body_updates": [{
        "target_node_id": node_id,
        "update_kind": "append_section",
        "section_text": concat!(
          "Node chat can append a source-backed clarification to the focused compiled section ",
          "after user review.",
        ),
        "reason": "The node-local conversation adds a clarification to the focused node.",
        "source_chunk_ids": [chunk_id]
      }]
    }
  });
  let (runtime, server) = local_runtime_with_chat_response(chat_json);

  let result =
    send_node_chat_turn_with_runtime(&paths, &runtime, &node_id, "Add the node-local clarification.").unwrap();
  server.join().unwrap();

  assert_eq!(result["runtime_status"], "completed", "{result:#}");
  assert_eq!(result["patch_import_status"], "accepted_to_graph");
  assert_eq!(result["proposal_count"], 1);
  assert!(result["assistant_message"]["content"].as_str().unwrap().contains("focused node"));

  let store = WorkspaceStore::open(&paths.database_path).unwrap();
  let graph = store.load_graph_snapshot().unwrap();
  assert_ne!(graph["nodes"][0]["compiled_body"], original_body);
  let queue = store.load_review_queue().unwrap();
  let assistant_proposals: Vec<&Value> = queue["items"]
    .as_array()
    .unwrap()
    .iter()
    .filter(|item| item["source_message_id"] == result["assistant_message"]["id"])
    .collect();
  assert_eq!(assistant_proposals.len(), 1);
  assert_eq!(assistant_proposals[0]["type"], "node_body_update");
  assert_eq!(assistant_proposals[0]["status"], "accepted");
  assert_eq!(assistant_proposals[0]["source"]["kind"], "node_message");
  assert_eq!(assistant_proposals[0]["source_message_id"], result["assistant_message"]["id"]);
  assert!(graph["nodes"][0]["compiled_body"].as_str().unwrap().contains("source-backed clarification"));
  assert_eq!(graph["nodes"][0]["body_version"], 2);
  let _ = fs::remove_dir_all(root);
}

#[test]
fn node_chat_uses_user_message_as_default_evidence_and_assistant_as_proposal_source() {
  let root = temp_root("soma-node-chat-message-authority-test");
  let paths =
    workspace_with_source(&root, "User: Node chat must keep proposal authorship separate from the user's evidence.");
  let chunk_id = first_chunk_id(&paths);
  let (node_id, _) = seed_active_node(&paths, &chunk_id);
  let chat_json = json!({
    "assistant_message": "I captured the user's node-local observation as a separate concept.",
    "used_graph_areas": [],
    "proposed_graph_patch": {
      "proposed_nodes": [{
        "temp_id": "node_user_backed_observation",
        "type": "concept",
        "title": "User-backed Observation",
        "compiled_body": "The current user message supplies the evidence for this node-chat concept."
      }]
    }
  });
  let (runtime, server) = local_runtime_with_chat_response(chat_json);

  let result = send_node_chat_turn_with_runtime(
    &paths,
    &runtime,
    &node_id,
    "Record this node-local observation as a separate concept.",
  )
  .unwrap();
  server.join().unwrap();

  assert_eq!(result["patch_import_status"], "accepted_to_graph", "{result:#}");
  let user_message_id = result["user_message"]["id"].as_str().unwrap();
  let assistant_message_id = result["assistant_message"]["id"].as_str().unwrap();
  let store = WorkspaceStore::open(&paths.database_path).unwrap();
  let graph = store.load_graph_snapshot().unwrap();
  let captured =
    graph["nodes"].as_array().unwrap().iter().find(|node| node["title"] == "User-backed Observation").unwrap();
  assert_eq!(captured["evidence"][0]["message_id"], user_message_id);
  assert_ne!(captured["evidence"][0]["message_id"], assistant_message_id);
  let queue = store.load_review_queue().unwrap();
  let proposal = queue["items"]
    .as_array()
    .unwrap()
    .iter()
    .find(|item| item["source_message_id"] == assistant_message_id && item["type"] == "node")
    .unwrap();
  assert_eq!(proposal["status"], "accepted");
  let _ = fs::remove_dir_all(root);
}

#[test]
fn node_chat_capture_off_keeps_the_answer_without_importing_the_runtime_patch() {
  let root = temp_root("soma-node-chat-capture-off-test");
  let paths = workspace_with_source(&root, "User: Question-only node chat must not change graph truth.");
  let chunk_id = first_chunk_id(&paths);
  let (node_id, _) = seed_active_node(&paths, &chunk_id);
  let chat_json = json!({
    "assistant_message": "This answer is retained without changing the graph.",
    "used_graph_areas": [],
    "proposed_graph_patch": {
      "proposed_nodes": [{
        "temp_id": "node_capture_must_ignore",
        "type": "concept",
        "title": "Ignored Capture",
        "compiled_body": "This runtime patch must be ignored while node-chat capture is off."
      }]
    }
  });
  let (runtime, server, request_received) = local_runtime_with_captured_chat_request(chat_json);

  let result = send_node_chat_turn_with_runtime_and_capture(
    &paths,
    &runtime,
    &node_id,
    "Answer this without changing the graph.",
    false,
  )
  .unwrap();
  let request_body = request_received.recv().unwrap();
  server.join().unwrap();

  assert!(request_body.contains("Graph capture is off for this turn."));
  assert_eq!(result["runtime_status"], "completed", "{result:#}");
  assert_eq!(result["assistant_message"]["content"], "This answer is retained without changing the graph.");
  assert_eq!(result["context_packet"]["graph_capture_enabled"], false);
  assert_eq!(result["patch_import_status"], "none");
  assert_eq!(result["proposal_count"], 0);

  let store = WorkspaceStore::open(&paths.database_path).unwrap();
  assert_eq!(store.load_graph_snapshot().unwrap()["nodes"].as_array().unwrap().len(), 1);
  let assistant_id = result["assistant_message"]["id"].as_str().unwrap();
  assert!(!store.load_review_queue().unwrap()["items"]
    .as_array()
    .unwrap()
    .iter()
    .any(|item| item["source_message_id"].as_str() == Some(assistant_id)));
  let messages = store.list_node_messages(&node_id).unwrap();
  assert_eq!(messages.as_array().unwrap().len(), 2);
  let _ = fs::remove_dir_all(root);
}

#[test]
fn node_chat_turn_keeps_stored_answer_when_patch_persistence_fails() {
  let root = temp_root("soma-node-chat-patch-storage-failure-test");
  let paths = workspace_with_source(&root, "User: Node chat answers must survive a graph patch storage failure.");
  let chunk_id = first_chunk_id(&paths);
  let (node_id, _) = seed_active_node(&paths, &chunk_id);
  let chat_json = json!({
    "assistant_message": "The node-chat answer remains available even though its graph update could not be saved.",
    "used_graph_areas": [],
    "proposed_graph_patch": {
      "proposed_nodes": [{
        "temp_id": "node_patch_storage_failure",
        "type": "concept",
        "title": "Patch Storage Failure",
        "compiled_body": "This graph object must not be accepted when patch persistence fails."
      }]
    }
  });
  let (runtime, server, request_received, release_response) = pausing_runtime_with_chat_response(chat_json);
  let chat_paths = paths.clone();
  let chat_node_id = node_id.clone();
  let chat_thread = thread::spawn(move || {
    send_node_chat_turn_with_runtime(&chat_paths, &runtime, &chat_node_id, "Keep the answer if the patch fails.")
      .unwrap()
  });

  request_received.recv().unwrap();
  let conn = open_database(&paths.database_path).unwrap();
  conn
    .execute_batch(
      r#"
      CREATE TRIGGER fail_node_chat_patch_insert
      BEFORE INSERT ON graph_patches
      WHEN NEW.source = 'node_thread_message'
      BEGIN
        SELECT RAISE(ABORT, 'forced node-chat patch storage failure');
      END;
      "#,
    )
    .unwrap();
  drop(conn);

  release_response.send(()).unwrap();
  let result = chat_thread.join().unwrap();
  server.join().unwrap();

  assert_eq!(result["runtime_status"], "completed", "{result:#}");
  assert!(result["assistant_message"]["content"].as_str().unwrap().contains("answer remains available"));
  assert_eq!(result["patch_import_status"], "invalid");
  assert_eq!(result["patch_import_result"]["valid"], false);
  assert!(result["patch_import_result"]["errors"][0]["message"]
    .as_str()
    .unwrap()
    .contains("forced node-chat patch storage failure"));

  let store = WorkspaceStore::open(&paths.database_path).unwrap();
  let messages = store.list_node_messages(&node_id).unwrap();
  assert_eq!(messages.as_array().unwrap().len(), 2);
  assert_eq!(messages[0]["role"], "user");
  assert_eq!(messages[1]["role"], "assistant");
  assert_eq!(store.load_graph_snapshot().unwrap()["nodes"].as_array().unwrap().len(), 1);
  let assistant_id = result["assistant_message"]["id"].as_str().unwrap();
  assert!(!store.load_review_queue().unwrap()["items"]
    .as_array()
    .unwrap()
    .iter()
    .any(|item| item["source_message_id"].as_str() == Some(assistant_id)));
  let _ = fs::remove_dir_all(root);
}

#[test]
fn node_chat_turn_releases_database_while_runtime_is_waiting() {
  let root = temp_root("soma-node-chat-lock-test");
  let paths = workspace_with_source(
    &root,
    concat!(
      "User: Node chat should not hold the database while the brain thinks.\n\n",
      "Assistant: Other workspace writes should still work.",
    ),
  );
  let chunk_id = first_chunk_id(&paths);
  let (node_id, _) = seed_active_node(&paths, &chunk_id);
  let chat_json = json!({
    "assistant_message": "The node runtime answer can be saved after another workspace write.",
    "used_graph_areas": [{
      "id": node_id,
      "title": "Node Chat",
      "type": "concept"
    }],
    "proposed_graph_patch": null
  });
  let (runtime, server, request_received, release_response) = pausing_runtime_with_chat_response(chat_json);
  let chat_paths = paths.clone();
  let chat_node_id = node_id.clone();
  let chat_thread = thread::spawn(move || {
    send_node_chat_turn_with_runtime(&chat_paths, &runtime, &chat_node_id, "Explain the node lock behavior.").unwrap()
  });

  request_received.recv().unwrap();
  let mut store = WorkspaceStore::open(&paths.database_path).unwrap();
  store.append_graph_message("This graph write should not see database is locked.", Vec::new()).unwrap();
  drop(store);

  release_response.send(()).unwrap();
  let result = chat_thread.join().unwrap();
  server.join().unwrap();

  assert_eq!(result["runtime_status"], "completed", "{result:#}");
  assert!(result["assistant_message"]["content"].as_str().unwrap().contains("node runtime answer"));
  let _ = fs::remove_dir_all(root);
}

#[test]
fn node_chat_turn_preserves_a_newer_user_body_edit() {
  let root = temp_root("soma-node-chat-stale-body-test");
  let paths = workspace_with_source(
    &root,
    "User: A delayed node-chat update must not replace a newer body edit made by the user.",
  );
  let chunk_id = first_chunk_id(&paths);
  let (node_id, _) = seed_active_node(&paths, &chunk_id);
  let chat_json = json!({
    "assistant_message": "I prepared a source-backed body update.",
    "used_graph_areas": [{ "id": node_id, "title": "Node Chat", "type": "concept" }],
    "proposed_graph_patch": {
      "proposed_node_body_updates": [{
        "target_node_id": node_id,
        "update_kind": "replace_body",
        "compiled_body": "This delayed model body must remain reviewable after the user edits the node.",
        "reason": "The source discusses stale-write safety.",
        "source_chunk_ids": [chunk_id]
      }]
    }
  });
  let (runtime, server, request_received, release_response) = pausing_runtime_with_chat_response(chat_json);
  let chat_paths = paths.clone();
  let chat_node_id = node_id.clone();
  let chat_thread = thread::spawn(move || {
    send_node_chat_turn_with_runtime(&chat_paths, &runtime, &chat_node_id, "Update this node.").unwrap()
  });

  request_received.recv().unwrap();
  let user_body = "The user wrote this body while the model was still answering.";
  let mut store = WorkspaceStore::open(&paths.database_path).unwrap();
  store.update_node_body(&node_id, user_body).unwrap();
  drop(store);

  release_response.send(()).unwrap();
  let result = chat_thread.join().unwrap();
  server.join().unwrap();

  assert_eq!(result["patch_import_status"], "imported_to_review", "{result:#}");
  assert_eq!(result["patch_import_result"]["trusted"], false);
  assert!(result["patch_import_result"]["autoAcceptResult"]["errors"][0]["message"]
    .as_str()
    .unwrap()
    .contains("changed after this update was proposed"));
  let store = WorkspaceStore::open(&paths.database_path).unwrap();
  let graph = store.load_graph_snapshot().unwrap();
  assert_eq!(graph["nodes"][0]["compiled_body"], user_body);
  let queue = store.load_review_queue().unwrap();
  let proposal = queue["items"]
    .as_array()
    .unwrap()
    .iter()
    .find(|item| item["source_message_id"] == result["assistant_message"]["id"])
    .unwrap();
  assert_ne!(proposal["status"], "accepted");
  let _ = fs::remove_dir_all(root);
}

#[test]
fn node_chat_turn_sends_only_focused_neighborhood_to_runtime() {
  let root = temp_root("soma-node-chat-context-test");
  let paths = create_workspace_dir(&root).unwrap();
  let focused_source = root.join("focused.md");
  let neighbor_source = root.join("neighbor.md");
  let unrelated_source = root.join("unrelated.md");
  fs::write(&focused_source, "User: FOCUSED_EVIDENCE_INCLUDED. This selected node is the one being discussed.")
    .unwrap();
  fs::write(&neighbor_source, "User: NEIGHBOR_EVIDENCE_INCLUDED. This one-hop neighbor should be available.").unwrap();
  fs::write(&unrelated_source, "User: UNRELATED_NODE_EVIDENCE_SENTINEL. This should not enter node chat context.")
    .unwrap();
  import_source_file(&paths, &focused_source).unwrap();
  import_source_file(&paths, &neighbor_source).unwrap();
  import_source_file(&paths, &unrelated_source).unwrap();
  let focused_chunk_id = chunk_id_containing(&paths, "FOCUSED_EVIDENCE_INCLUDED");
  let neighbor_chunk_id = chunk_id_containing(&paths, "NEIGHBOR_EVIDENCE_INCLUDED");
  let unrelated_chunk_id = chunk_id_containing(&paths, "UNRELATED_NODE_EVIDENCE_SENTINEL");
  let (focused_node_id, neighbor_node_id, local_edge_id) = seed_connected_active_nodes(
    &paths,
    "node_focused_context",
    "Focused Context",
    "FOCUSED_BODY_INCLUDED. The selected node body should be sent.",
    &focused_chunk_id,
    "node_neighbor_context",
    "Neighbor Context",
    "NEIGHBOR_BODY_INCLUDED. This one-hop neighbor body should be sent.",
    &neighbor_chunk_id,
    "LOCAL_BRIDGE_INCLUDED",
  );
  seed_active_node_with_body(
    &paths,
    "node_unrelated_context",
    "Unrelated Context",
    "UNRELATED_NODE_BODY_SENTINEL. This body should not be sent.",
    &unrelated_chunk_id,
  );
  let chat_json = json!({
    "assistant_message": "The selected node is the only relevant context.",
    "used_graph_areas": [{
      "id": focused_node_id,
      "title": "Focused Context",
      "type": "concept"
    }],
    "proposed_graph_patch": null
  });
  let (runtime, server, request_received) = local_runtime_with_captured_chat_request(chat_json);

  let result =
    send_node_chat_turn_with_runtime(&paths, &runtime, &focused_node_id, "Use only this selected node.").unwrap();
  let request_body = request_received.recv().unwrap();
  server.join().unwrap();

  assert!(request_body.contains("FOCUSED_BODY_INCLUDED"));
  assert!(request_body.contains("FOCUSED_EVIDENCE_INCLUDED"));
  assert!(request_body.contains("NEIGHBOR_BODY_INCLUDED"));
  assert!(request_body.contains("NEIGHBOR_EVIDENCE_INCLUDED"));
  assert!(request_body.contains("LOCAL_BRIDGE_INCLUDED"));
  assert!(!request_body.contains("UNRELATED_NODE_BODY_SENTINEL"));
  assert!(!request_body.contains("UNRELATED_NODE_EVIDENCE_SENTINEL"));
  assert_eq!(result["context_packet"]["focused_node_id"], focused_node_id);
  let neighbor_bodies = result["context_packet"]["neighbor_bodies"].as_array().unwrap();
  assert_eq!(neighbor_bodies.len(), 1, "{result:#}");
  assert_eq!(neighbor_bodies[0]["id"], neighbor_node_id);
  assert!(result["context_packet"]["bridge_texts"]
    .as_array()
    .unwrap()
    .iter()
    .any(|item| item["edge_id"] == local_edge_id && item["bridge_text"] == "LOCAL_BRIDGE_INCLUDED"));
  let allowed_evidence_ids = [focused_node_id.as_str(), neighbor_node_id.as_str(), local_edge_id.as_str()];
  assert!(result["context_packet"]["source_evidence_excerpts"]
    .as_array()
    .unwrap()
    .iter()
    .all(|item| item["entity_id"].as_str().is_some_and(|id| allowed_evidence_ids.contains(&id))));
  let _ = fs::remove_dir_all(root);
}

fn workspace_with_source(root: &Path, source_text: &str) -> WorkspacePaths {
  let paths = create_workspace_dir(root).unwrap();
  let source = root.join("source.md");
  fs::write(&source, source_text).unwrap();
  import_source_file(&paths, &source).unwrap();
  paths
}

fn seed_active_node(paths: &WorkspacePaths, chunk_id: &str) -> (String, String) {
  let node_id = seed_active_node_with_body(
    paths,
    "node_chat",
    "Node Chat",
    "Node chat keeps work scoped to the selected compiled section.",
    chunk_id,
  );
  let graph = WorkspaceStore::open(&paths.database_path).unwrap().load_graph_snapshot().unwrap();
  let compiled_body = graph["nodes"].as_array().unwrap().iter().find(|node| node["id"] == node_id).unwrap()
    ["compiled_body"]
    .as_str()
    .unwrap()
    .to_string();
  (node_id, compiled_body)
}

fn seed_active_node_with_body(
  paths: &WorkspacePaths,
  temp_id: &str,
  title: &str,
  compiled_body: &str,
  chunk_id: &str,
) -> String {
  let mut store = WorkspaceStore::open(&paths.database_path).unwrap();
  let message = store.append_graph_message(&format!("Create {title}."), Vec::new()).unwrap();
  let mut patch = empty_graph_patch();
  patch["proposed_nodes"] = json!([{
    "temp_id": temp_id,
    "type": "concept",
    "title": title,
    "compiled_body": compiled_body,
    "source_chunk_ids": [chunk_id],
    "reason": "Fixture node for direct chat."
  }]);
  store.propose_graph_updates(message["message"]["id"].as_str().unwrap(), patch).unwrap();
  let queue = store.load_review_queue().unwrap();
  let proposal_id = queue["items"]
    .as_array()
    .unwrap()
    .iter()
    .find(|item| item["temp_id"] == temp_id && item["status"] == "proposed")
    .unwrap()["id"]
    .as_str()
    .unwrap()
    .to_string();
  store.accept_graph_proposal(&proposal_id, None).unwrap();
  let graph = store.load_graph_snapshot().unwrap();
  graph["nodes"].as_array().unwrap().iter().find(|node| node["title"] == title).unwrap()["id"]
    .as_str()
    .unwrap()
    .to_string()
}

#[allow(clippy::too_many_arguments)]
fn seed_connected_active_nodes(
  paths: &WorkspacePaths,
  source_temp_id: &str,
  source_title: &str,
  source_body: &str,
  source_chunk_id: &str,
  target_temp_id: &str,
  target_title: &str,
  target_body: &str,
  target_chunk_id: &str,
  bridge_text: &str,
) -> (String, String, String) {
  let mut store = WorkspaceStore::open(&paths.database_path).unwrap();
  let message = store.append_graph_message(&format!("Connect {source_title} to {target_title}."), Vec::new()).unwrap();
  let mut patch = empty_graph_patch();
  patch["proposed_nodes"] = json!([
      {
        "temp_id": source_temp_id,
        "type": "concept",
        "title": source_title,
        "compiled_body": source_body,
        "source_chunk_ids": [source_chunk_id],
        "reason": "Fixture source node for direct chat."
      },
      {
        "temp_id": target_temp_id,
        "type": "concept",
        "title": target_title,
        "compiled_body": target_body,
        "source_chunk_ids": [target_chunk_id],
        "reason": "Fixture target node for direct chat."
      }
  ]);
  patch["proposed_edges"] = json!([{
    "source_temp_id": source_temp_id,
    "target_temp_id": target_temp_id,
    "type": "supports",
    "bridge_text": bridge_text,
    "source_chunk_ids": [source_chunk_id],
    "reason": "Fixture one-hop edge for node chat."
  }]);
  let proposed = store.propose_graph_updates(message["message"]["id"].as_str().unwrap(), patch).unwrap();
  let patch_id = proposed["patchId"].as_str().unwrap();
  let accepted = store.accept_graph_patch_proposals(patch_id, None).unwrap();
  assert_eq!(accepted["errors"].as_array().unwrap().len(), 0);
  let graph = store.load_graph_snapshot().unwrap();
  let nodes = graph["nodes"].as_array().unwrap();
  let source_node_id =
    nodes.iter().find(|node| node["title"] == source_title).unwrap()["id"].as_str().unwrap().to_string();
  let target_node_id =
    nodes.iter().find(|node| node["title"] == target_title).unwrap()["id"].as_str().unwrap().to_string();
  let edge_id = graph["edges"].as_array().unwrap().iter().find(|edge| edge["bridge_text"] == bridge_text).unwrap()
    ["id"]
    .as_str()
    .unwrap()
    .to_string();
  (source_node_id, target_node_id, edge_id)
}

fn first_chunk_id(paths: &WorkspacePaths) -> String {
  let conn: Connection = open_database(&paths.database_path).unwrap();
  conn.query_row("SELECT id FROM chunks ORDER BY chunk_index LIMIT 1", [], |row| row.get::<_, String>(0)).unwrap()
}

fn chunk_id_containing(paths: &WorkspacePaths, needle: &str) -> String {
  let conn: Connection = open_database(&paths.database_path).unwrap();
  conn
    .query_row(
      "SELECT id FROM chunks WHERE content LIKE ?1 ORDER BY id LIMIT 1",
      params![format!("%{needle}%")],
      |row| row.get::<_, String>(0),
    )
    .unwrap()
}

fn edge_evidence_ids(paths: &WorkspacePaths, table: &str, edge_id: &str) -> HashSet<String> {
  let sql = match table {
    "graph_evidence" => "SELECT id FROM graph_evidence WHERE entity_type = 'edge' AND entity_id = ?1",
    "graph_message_evidence" => {
      "SELECT id FROM graph_message_evidence WHERE target_entity_type = 'edge' AND target_entity_id = ?1"
    }
    _ => panic!("unsupported edge evidence table: {table}"),
  };
  let conn = open_database(&paths.database_path).unwrap();
  let mut stmt = conn.prepare(sql).unwrap();
  stmt.query_map(params![edge_id], |row| row.get::<_, String>(0)).unwrap().collect::<Result<HashSet<_>, _>>().unwrap()
}

fn inserted_ids(before: &HashSet<String>, after: &HashSet<String>) -> HashSet<String> {
  after.difference(before).cloned().collect()
}

fn local_runtime_with_chat_response(chat_response: Value) -> (Value, thread::JoinHandle<()>) {
  let listener = TcpListener::bind("127.0.0.1:0").unwrap();
  let endpoint = format!("http://{}", listener.local_addr().unwrap());
  let server = thread::spawn(move || {
    let (mut stream, _) = listener.accept().unwrap();
    read_http_request(&mut stream);
    let body = json!({
      "choices": [{
        "message": {
          "content": chat_response.to_string()
        }
      }]
    })
    .to_string();
    let response = format!(
      "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
      body.len(),
      body
    );
    stream.write_all(response.as_bytes()).unwrap();
  });
  let runtime = json!({
    "providerId": "local_llm",
    "model": "fixture-model",
    "endpoint": endpoint,
    "authProfile": "",
    "credentialConfigured": false,
    "adapter": {
      "kind": "local_offline_endpoint",
      "endpoint": endpoint
    }
  });
  (runtime, server)
}

fn local_runtime_with_captured_chat_request(
  chat_response: Value,
) -> (Value, thread::JoinHandle<()>, mpsc::Receiver<String>) {
  let listener = TcpListener::bind("127.0.0.1:0").unwrap();
  let endpoint = format!("http://{}", listener.local_addr().unwrap());
  let (request_sender, request_received) = mpsc::channel();
  let server = thread::spawn(move || {
    let (mut stream, _) = listener.accept().unwrap();
    request_sender.send(read_http_request_body(&mut stream)).unwrap();
    let body = json!({
      "choices": [{
        "message": {
          "content": chat_response.to_string()
        }
      }]
    })
    .to_string();
    let response = format!(
      "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
      body.len(),
      body
    );
    stream.write_all(response.as_bytes()).unwrap();
  });
  let runtime = json!({
    "providerId": "local_llm",
    "model": "fixture-model",
    "endpoint": endpoint,
    "authProfile": "",
    "credentialConfigured": false,
    "adapter": {
      "kind": "local_offline_endpoint",
      "endpoint": endpoint
    }
  });
  (runtime, server, request_received)
}

fn pausing_runtime_with_chat_response(
  chat_response: Value,
) -> (Value, thread::JoinHandle<()>, mpsc::Receiver<()>, mpsc::Sender<()>) {
  let listener = TcpListener::bind("127.0.0.1:0").unwrap();
  let endpoint = format!("http://{}", listener.local_addr().unwrap());
  let (request_sender, request_received) = mpsc::channel();
  let (release_response, release_receiver) = mpsc::channel();
  let server = thread::spawn(move || {
    let (mut stream, _) = listener.accept().unwrap();
    read_http_request(&mut stream);
    request_sender.send(()).unwrap();
    release_receiver.recv().unwrap();
    let body = json!({
      "choices": [{
        "message": {
          "content": chat_response.to_string()
        }
      }]
    })
    .to_string();
    let response = format!(
      "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
      body.len(),
      body
    );
    stream.write_all(response.as_bytes()).unwrap();
  });
  let runtime = json!({
    "providerId": "local_llm",
    "model": "fixture-model",
    "endpoint": endpoint,
    "authProfile": "",
    "credentialConfigured": false,
    "adapter": {
      "kind": "local_offline_endpoint",
      "endpoint": endpoint
    }
  });
  (runtime, server, request_received, release_response)
}

fn read_http_request(stream: &mut TcpStream) {
  let _ = read_http_request_body(stream);
}

fn read_http_request_body(stream: &mut TcpStream) -> String {
  let mut bytes = Vec::new();
  let mut buffer = [0_u8; 4096];
  loop {
    let read = stream.read(&mut buffer).unwrap_or(0);
    if read == 0 {
      return String::new();
    }
    bytes.extend_from_slice(&buffer[..read]);
    let Some(header_end) = find_header_end(&bytes) else {
      continue;
    };
    let headers = String::from_utf8_lossy(&bytes[..header_end]);
    let content_length = headers
      .lines()
      .find_map(|line| {
        let (name, value) = line.split_once(':')?;
        name.eq_ignore_ascii_case("content-length").then(|| value.trim().parse::<usize>().ok()).flatten()
      })
      .unwrap_or(0);
    if bytes.len() >= header_end + 4 + content_length {
      let body_start = header_end + 4;
      let body_end = body_start + content_length;
      return String::from_utf8_lossy(&bytes[body_start..body_end]).to_string();
    }
  }
}

fn find_header_end(bytes: &[u8]) -> Option<usize> {
  bytes.windows(4).position(|window| window == b"\r\n\r\n")
}

fn temp_root(prefix: &str) -> std::path::PathBuf {
  std::env::temp_dir().join(format!("{prefix}-{}", Uuid::new_v4()))
}
