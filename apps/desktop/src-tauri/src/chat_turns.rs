use std::collections::{HashMap, HashSet};

use serde_json::{json, Value};
use soma_ai_runtime::CredentialResolver;

use crate::chat_runtime::RuntimeChatTurnResult;
use crate::contracts::{complete_graph_patch, graph_patch_is_empty};
use crate::error::{is_storage_busy_message, CommandError, CommandResult, STORAGE_BUSY_MESSAGE};
use crate::repository::WorkspaceStore;
use crate::runtime_adapters::run_chat_turn_with_credentials;
use crate::workspace::WorkspacePaths;

#[cfg(test)]
pub fn send_graph_chat_turn_with_credentials(
  paths: &WorkspacePaths,
  runtime: &Value,
  content: &str,
  focus_node_ids: Vec<String>,
  credentials: &dyn CredentialResolver,
) -> CommandResult<Value> {
  send_graph_chat_turn_with_reading_context_and_credentials(
    paths,
    runtime,
    content,
    focus_node_ids,
    None,
    true,
    credentials,
  )
}

pub fn send_graph_chat_turn_with_reading_context_and_credentials(
  paths: &WorkspacePaths,
  runtime: &Value,
  content: &str,
  focus_node_ids: Vec<String>,
  reading_context: Option<Value>,
  capture_graph_changes: bool,
  credentials: &dyn CredentialResolver,
) -> CommandResult<Value> {
  let user = {
    let mut store = WorkspaceStore::open(&paths.database_path)?;
    store.append_graph_message_with_reading_context(content, focus_node_ids, reading_context, capture_graph_changes)?
  };
  let context_packet = user["context_packet"].clone();
  let runtime_result = runtime_result_or_failure(run_chat_turn_with_credentials(
    runtime,
    &chat_turn_request("graph_chat", &context_packet, capture_graph_changes),
    credentials,
  ));
  let Some(answer) = runtime_result.assistant_message.as_deref() else {
    return Ok(chat_turn_failure_result(user, context_packet, runtime_result, no_patch_result()));
  };

  let context_packet = context_with_used_areas(&context_packet, &runtime_result.used_graph_areas);
  let mut store = match WorkspaceStore::open(&paths.database_path) {
    Ok(store) => store,
    Err(error) => {
      return Ok(chat_turn_failure_result(
        user,
        context_packet,
        storage_failure_runtime_result(&runtime_result.adapter_kind, error),
        no_patch_result(),
      ));
    }
  };
  let assistant = match store.append_graph_assistant_message(answer, &context_packet) {
    Ok(assistant) => assistant,
    Err(error) => {
      return Ok(chat_turn_failure_result(
        user,
        context_packet,
        storage_failure_runtime_result(&runtime_result.adapter_kind, error),
        no_patch_result(),
      ));
    }
  };
  let (mut patch_result, mutation_preconditions_present) = if capture_graph_changes {
    match import_chat_patch_if_present(runtime_result.proposed_graph_patch.as_ref(), &context_packet, |patch| {
      store.propose_graph_updates_with_evidence_message(
        assistant["id"].as_str().unwrap_or(""),
        user["message"]["id"].as_str().unwrap_or(""),
        patch,
      )
    }) {
      Ok(result) => result,
      Err(error) => (failed_patch_import_result(&error.message), false),
    }
  } else {
    (no_patch_result(), true)
  };
  if capture_graph_changes {
    auto_accept_imported_patch(&mut store, &mut patch_result, mutation_preconditions_present)?;
  }

  Ok(chat_turn_success_result(user, assistant, context_packet, runtime_result, patch_result))
}

pub fn send_node_chat_turn_with_credentials(
  paths: &WorkspacePaths,
  runtime: &Value,
  node_id: &str,
  content: &str,
  capture_graph_changes: bool,
  credentials: &dyn CredentialResolver,
) -> CommandResult<Value> {
  let user = {
    let mut store = WorkspaceStore::open(&paths.database_path)?;
    store.append_node_message_with_capture(node_id, content, capture_graph_changes)?
  };
  let context_packet = user["context_packet"].clone();
  let runtime_result = runtime_result_or_failure(run_chat_turn_with_credentials(
    runtime,
    &chat_turn_request("node_chat", &context_packet, capture_graph_changes),
    credentials,
  ));
  let Some(answer) = runtime_result.assistant_message.as_deref() else {
    return Ok(chat_turn_failure_result(user, context_packet, runtime_result, no_patch_result()));
  };

  let mut store = match WorkspaceStore::open(&paths.database_path) {
    Ok(store) => store,
    Err(error) => {
      return Ok(chat_turn_failure_result(
        user,
        context_packet,
        storage_failure_runtime_result(&runtime_result.adapter_kind, error),
        no_patch_result(),
      ));
    }
  };
  let assistant = match store.append_node_assistant_message(node_id, answer, &context_packet) {
    Ok(assistant) => assistant,
    Err(error) => {
      return Ok(chat_turn_failure_result(
        user,
        context_packet,
        storage_failure_runtime_result(&runtime_result.adapter_kind, error),
        no_patch_result(),
      ));
    }
  };
  let (mut patch_result, mutation_preconditions_present) = if capture_graph_changes {
    match import_chat_patch_if_present(runtime_result.proposed_graph_patch.as_ref(), &context_packet, |patch| {
      store.propose_node_updates(
        assistant["id"].as_str().unwrap_or(""),
        user["message"]["id"].as_str().unwrap_or(""),
        patch,
      )
    }) {
      Ok(result) => result,
      Err(error) => (failed_patch_import_result(&error.message), false),
    }
  } else {
    (no_patch_result(), true)
  };
  if capture_graph_changes {
    auto_accept_imported_patch(&mut store, &mut patch_result, mutation_preconditions_present)?;
  }

  Ok(chat_turn_success_result(user, assistant, context_packet, runtime_result, patch_result))
}

fn chat_turn_request(mode: &str, context_packet: &Value, capture_graph_changes: bool) -> Value {
  json!({
    "schema_version": 1,
    "mode": mode,
    "context_packet": context_packet,
    "capture_graph_changes": capture_graph_changes
  })
}

fn runtime_result_or_failure(result: CommandResult<RuntimeChatTurnResult>) -> RuntimeChatTurnResult {
  result.unwrap_or_else(|error| {
    let failure_kind = error.runtime_failure_kind();
    RuntimeChatTurnResult {
      adapter_kind: "unknown".to_string(),
      status: "failed",
      failure_kind: Some(failure_kind),
      message: error.message,
      assistant_message: None,
      used_graph_areas: Vec::new(),
      proposed_graph_patch: None,
    }
  })
}

fn storage_failure_runtime_result(adapter_kind: &str, error: CommandError) -> RuntimeChatTurnResult {
  RuntimeChatTurnResult {
    adapter_kind: adapter_kind.to_string(),
    status: "failed",
    failure_kind: Some(error.runtime_failure_kind()),
    message: error.message,
    assistant_message: None,
    used_graph_areas: Vec::new(),
    proposed_graph_patch: None,
  }
}

fn import_chat_patch_if_present(
  patch: Option<&Value>,
  context_packet: &Value,
  import: impl FnOnce(Value) -> CommandResult<Value>,
) -> CommandResult<(Value, bool)> {
  let Some(patch) = patch else {
    return Ok((no_patch_result(), true));
  };
  let patch = complete_graph_patch(patch);
  let (patch, mutation_preconditions_present) = attach_chat_mutation_preconditions(patch, context_packet);
  if graph_patch_is_empty(&patch) {
    return Ok((no_patch_result(), true));
  }
  import(patch).map(|result| (normalize_chat_patch_import_result(result), mutation_preconditions_present))
}

fn attach_chat_mutation_preconditions(mut patch: Value, context_packet: &Value) -> (Value, bool) {
  let body_versions = context_body_versions(context_packet);
  let edge_revisions = context_edge_revisions(context_packet);
  let mut all_preconditioned = true;
  if let Some(updates) = patch.get_mut("proposed_node_body_updates").and_then(Value::as_array_mut) {
    for update in updates {
      let Some(update) = update.as_object_mut() else {
        all_preconditioned = false;
        continue;
      };
      let target_id =
        update.get("target_node_id").or_else(|| update.get("node_id")).and_then(Value::as_str).map(str::to_string);
      if let Some(version_id) = target_id.as_ref().and_then(|node_id| body_versions.get(node_id)) {
        update.insert("base_body_version_id".to_string(), json!(version_id));
      } else {
        update.remove("base_body_version_id");
        all_preconditioned = false;
      }
    }
  }
  if let Some(updates) = patch.get_mut("proposed_edge_bridge_updates").and_then(Value::as_array_mut) {
    for update in updates {
      let Some(update) = update.as_object_mut() else {
        all_preconditioned = false;
        continue;
      };
      let target_id =
        update.get("target_edge_id").or_else(|| update.get("edge_id")).and_then(Value::as_str).map(str::to_string);
      if let Some(updated_at) = target_id.as_ref().and_then(|edge_id| edge_revisions.get(edge_id)) {
        update.insert("base_edge_updated_at".to_string(), json!(updated_at));
      } else {
        update.remove("base_edge_updated_at");
        all_preconditioned = false;
      }
    }
  }
  (patch, all_preconditioned)
}

fn context_body_versions(context_packet: &Value) -> HashMap<String, String> {
  let mut versions = HashMap::new();
  if let Some(body) = context_packet.get("focused_node_body") {
    collect_body_version(&mut versions, body);
  }
  for field in ["focus_set_node_bodies", "top_matching_node_bodies", "neighbor_bodies"] {
    for body in context_packet.get(field).and_then(Value::as_array).into_iter().flatten() {
      collect_body_version(&mut versions, body);
    }
  }
  versions
}

fn collect_body_version(versions: &mut HashMap<String, String>, body: &Value) {
  let Some(node_id) = body.get("id").and_then(Value::as_str) else {
    return;
  };
  let Some(version_id) = body.get("body_version_id").and_then(Value::as_str) else {
    return;
  };
  versions.insert(node_id.to_string(), version_id.to_string());
}

fn context_edge_revisions(context_packet: &Value) -> HashMap<String, String> {
  let mut revisions = HashMap::new();
  for field in ["relevant_path_fragments", "bridge_texts"] {
    for edge in context_packet.get(field).and_then(Value::as_array).into_iter().flatten() {
      let Some(edge_id) = edge.get("edge_id").and_then(Value::as_str) else {
        continue;
      };
      let Some(updated_at) = edge.get("updated_at").and_then(Value::as_str) else {
        continue;
      };
      revisions.insert(edge_id.to_string(), updated_at.to_string());
    }
  }
  revisions
}

fn no_patch_result() -> Value {
  json!({
    "valid": true,
    "imported": false,
    "trusted": false,
    "proposalCount": 0,
    "proposals": [],
    "errors": [],
    "warnings": []
  })
}

fn failed_patch_import_result(message: &str) -> Value {
  json!({
    "valid": false,
    "imported": false,
    "trusted": false,
    "proposalCount": 0,
    "proposals": [],
    "errors": [{ "message": normalize_chat_failure_message(message) }],
    "warnings": []
  })
}

fn normalize_chat_patch_import_result(mut result: Value) -> Value {
  if !result.is_object() {
    return json!({
      "valid": false,
      "imported": false,
      "trusted": false,
      "proposalCount": 0,
      "proposals": [],
      "errors": [{ "message": "Graph update import returned an unsupported result." }],
      "warnings": []
    });
  }
  if result.get("valid").and_then(Value::as_bool).is_none() {
    result["valid"] = json!(false);
  }
  if result.get("imported").and_then(Value::as_bool).is_none() {
    result["imported"] = json!(false);
  }
  if result.get("trusted").and_then(Value::as_bool).is_none() {
    result["trusted"] = json!(false);
  }
  if result.get("proposalCount").and_then(Value::as_i64).is_none() {
    result["proposalCount"] = json!(0);
  }
  if result.get("proposals").and_then(Value::as_array).is_none() {
    result["proposals"] = json!([]);
  }
  if result.get("errors").and_then(Value::as_array).is_none() {
    result["errors"] = json!([]);
  }
  if result.get("warnings").and_then(Value::as_array).is_none() {
    result["warnings"] = json!([]);
  }
  result
}

fn auto_accept_imported_patch(
  store: &mut WorkspaceStore,
  patch_result: &mut Value,
  mutation_preconditions_present: bool,
) -> CommandResult<()> {
  if !patch_result.get("imported").and_then(Value::as_bool).unwrap_or(false) {
    return Ok(());
  }
  if !mutation_preconditions_present || !auto_acceptable_patch_result(patch_result) {
    patch_result["trusted"] = json!(false);
    return Ok(());
  }
  let Some(patch_id) = patch_result.get("patchId").and_then(Value::as_str).map(str::to_string) else {
    return Ok(());
  };
  let accepted = match store.accept_graph_patch_proposals(&patch_id, Some("auto-applied from chat")) {
    Ok(accepted) => accepted,
    Err(error) => {
      patch_result["trusted"] = json!(false);
      patch_result["autoAcceptResult"] = json!({
        "patchId": patch_id,
        "acceptedCount": 0,
        "accepted": [],
        "errors": [{ "message": normalize_chat_failure_message(&error.message) }]
      });
      return Ok(());
    }
  };
  patch_result["autoAcceptResult"] = accepted;
  let proposal_count = patch_result.get("proposalCount").and_then(Value::as_i64).unwrap_or(0);
  let accepted_count = patch_result["autoAcceptResult"].get("acceptedCount").and_then(Value::as_i64).unwrap_or(0);
  let error_count =
    patch_result["autoAcceptResult"].get("errors").and_then(Value::as_array).map(|errors| errors.len()).unwrap_or(0);
  patch_result["trusted"] = json!(proposal_count > 0 && accepted_count == proposal_count && error_count == 0);
  Ok(())
}

fn auto_acceptable_patch_result(patch_result: &Value) -> bool {
  let Some(proposals) = patch_result.get("proposals").and_then(Value::as_array) else {
    return false;
  };
  !proposals.is_empty()
    && proposals.iter().all(|proposal| {
      matches!(
        proposal.get("type").and_then(Value::as_str),
        Some("node" | "node_body_update" | "edge" | "edge_bridge_update") | Some("message_evidence_attachment")
      )
    })
}

fn chat_turn_failure_result(
  user: Value,
  context_packet: Value,
  runtime_result: RuntimeChatTurnResult,
  patch_result: Value,
) -> Value {
  let runtime_message = normalize_chat_failure_message(&runtime_result.message);
  let proposal_count = patch_result.get("proposalCount").and_then(Value::as_i64).unwrap_or(0);
  let patch_import_status = patch_import_status_for_result(&patch_result);
  json!({
    "user_message_id": user["message"]["id"],
    "user_message": user["message"],
    "assistant_message": null,
    "context_packet": context_packet,
    "used_graph_areas": context_packet.get("used_graph_areas").cloned().unwrap_or_else(|| json!([])),
    "proposal_count": proposal_count,
    "patch_import_status": patch_import_status,
    "patch_import_result": patch_result,
    "runtime_status": runtime_result.status,
    "runtime_adapter_kind": runtime_result.adapter_kind,
    "runtime_failure_kind": runtime_result.failure_kind,
    "runtime_message": runtime_message.clone(),
    "error": runtime_message
  })
}

fn normalize_chat_failure_message(message: &str) -> String {
  if is_storage_busy_message(message) {
    return STORAGE_BUSY_MESSAGE.to_string();
  }
  message.to_string()
}

fn chat_turn_success_result(
  user: Value,
  assistant: Value,
  context_packet: Value,
  runtime_result: RuntimeChatTurnResult,
  patch_result: Value,
) -> Value {
  let imported = patch_result.get("imported").and_then(Value::as_bool).unwrap_or(false);
  let valid = patch_result.get("valid").and_then(Value::as_bool).unwrap_or(true);
  let proposal_count = patch_result.get("proposalCount").and_then(Value::as_i64).unwrap_or(0);
  let trusted = patch_result.get("trusted").and_then(Value::as_bool).unwrap_or(false);
  let patch_import_status = patch_import_status_for_values(trusted, imported, valid);
  let error =
    if !valid { Some("Graph updates need regeneration; the assistant answer was kept.".to_string()) } else { None };

  json!({
    "user_message_id": user["message"]["id"],
    "user_message": user["message"],
    "assistant_message": assistant,
    "context_packet": context_packet,
    "used_graph_areas": context_packet.get("used_graph_areas").cloned().unwrap_or_else(|| json!([])),
    "proposal_count": proposal_count,
    "patch_import_status": patch_import_status,
    "patch_import_result": patch_result,
    "runtime_status": runtime_result.status,
    "runtime_adapter_kind": runtime_result.adapter_kind,
    "runtime_failure_kind": runtime_result.failure_kind,
    "runtime_message": runtime_result.message,
    "error": error
  })
}

fn patch_import_status_for_result(patch_result: &Value) -> &'static str {
  let imported = patch_result.get("imported").and_then(Value::as_bool).unwrap_or(false);
  let valid = patch_result.get("valid").and_then(Value::as_bool).unwrap_or(true);
  let trusted = patch_result.get("trusted").and_then(Value::as_bool).unwrap_or(false);
  patch_import_status_for_values(trusted, imported, valid)
}

fn patch_import_status_for_values(trusted: bool, imported: bool, valid: bool) -> &'static str {
  if trusted {
    "accepted_to_graph"
  } else if imported {
    "imported_to_review"
  } else if !valid {
    "invalid"
  } else {
    "none"
  }
}

fn context_with_used_areas(context_packet: &Value, used_graph_areas: &[Value]) -> Value {
  if used_graph_areas.is_empty() {
    return context_packet.clone();
  }
  let known_areas = context_graph_area_refs(context_packet);
  let mut seen = HashSet::new();
  let normalized = used_graph_areas
    .iter()
    .filter_map(|area| area.get("id").and_then(Value::as_str).map(str::trim))
    .filter(|id| !id.is_empty() && seen.insert((*id).to_string()))
    .filter_map(|id| known_areas.get(id).cloned())
    .collect::<Vec<_>>();
  if normalized.is_empty() {
    return context_packet.clone();
  }
  let mut next = context_packet.clone();
  next["used_graph_areas"] = json!(normalized);
  next
}

fn context_graph_area_refs(context_packet: &Value) -> HashMap<String, Value> {
  let mut areas = HashMap::new();
  for field in [
    "used_graph_areas",
    "focus_set_node_bodies",
    "top_matching_nodes",
    "top_matching_node_bodies",
    "unresolved_questions",
    "open_tasks",
    "neighbor_bodies",
  ] {
    for area in context_packet.get(field).and_then(Value::as_array).into_iter().flatten() {
      insert_context_graph_area_ref(&mut areas, area);
    }
  }
  if let Some(area) = context_packet.get("focused_node_body") {
    insert_context_graph_area_ref(&mut areas, area);
  }
  areas
}

fn insert_context_graph_area_ref(areas: &mut HashMap<String, Value>, area: &Value) {
  let Some(id) = area.get("id").and_then(Value::as_str).map(str::trim).filter(|value| !value.is_empty()) else {
    return;
  };
  let Some(title) = area.get("title").and_then(Value::as_str).map(str::trim).filter(|value| !value.is_empty()) else {
    return;
  };
  let mut normalized = json!({ "id": id, "title": title });
  if let Some(area_type) = area.get("type").and_then(Value::as_str).map(str::trim).filter(|value| !value.is_empty()) {
    normalized["type"] = json!(area_type);
  }
  areas.entry(id.to_string()).or_insert(normalized);
}

#[cfg(test)]
#[path = "chat_turns_tests.rs"]
mod tests;
