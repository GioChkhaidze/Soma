use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use serde_json::{json, Value};
use soma_ai_runtime::{AgentTaskCancellation, NoopCredentialResolver};
use uuid::Uuid;

use crate::brain_settings::BrainSettings;
use crate::chat_turns::{
  send_graph_chat_turn_with_credentials, send_node_chat_turn_with_credentials, ChatRuntimeExecution,
};
use crate::jobs::compile_graph_workspace_with_runtime_and_credentials;
use crate::repository::WorkspaceStore;
use crate::runtime_adapters::{codex_brain_status, runtime_descriptor};
use crate::source_import::{import_source_file, workspace_stats};
use crate::workspace::{create_workspace_dir, RAW_IMPORT_DIR};

const LIVE_TEST_FLAG: &str = "SOMA_RUN_LIVE_CODEX_E2E";
const LIVE_TEST_ROOT: &str = "SOMA_LIVE_CODEX_E2E_ROOT";

#[test]
#[ignore = "requires an installed, authenticated Codex CLI and makes live model requests"]
fn live_codex_builds_and_mutates_an_evidence_backed_workspace() {
  assert_eq!(env::var(LIVE_TEST_FLAG).as_deref(), Ok("1"), "set {LIVE_TEST_FLAG}=1 to run this live test");

  let (run_root, preserve_artifacts) = live_run_root();
  fs::create_dir_all(&run_root).unwrap();
  println!("live Codex artifacts: {}", run_root.display());

  let source_text = concat!(
    "User: Project Helios uses Adaptive Retrieval to select only source chunks relevant to the current question.\n\n",
    "Assistant: The Evidence Ledger records every selected chunk and its exact quote so claims remain auditable.\n\n",
    "User: Adaptive Retrieval supports the Evidence Ledger by supplying the evidence that the ledger preserves.\n\n",
    "Assistant: Treat Adaptive Retrieval and the Evidence Ledger as separate durable concepts ",
    "connected by that support relation."
  );
  let source_path = run_root.join("project-helios.md");
  fs::write(&source_path, source_text).unwrap();
  let paths = create_workspace_dir(run_root.join("workspace")).unwrap();

  let import_result = import_source_file(&paths, &source_path).unwrap();
  assert_eq!(import_result["messageCount"], 4, "{import_result:#}");
  assert_eq!(import_result["chunkCount"], 4, "{import_result:#}");
  let stats = workspace_stats(&paths).unwrap();
  assert_eq!(stats["sources"], 1);
  assert!(stats["messages"].as_i64().unwrap_or_default() >= 4, "{stats:#}");
  assert!(stats["chunks"].as_i64().unwrap_or_default() >= 4, "{stats:#}");
  let raw_sources = files_in(&paths.workspace_dir.join(RAW_IMPORT_DIR));
  assert_eq!(raw_sources.len(), 1, "raw imports: {raw_sources:#?}");
  assert_eq!(fs::read(&raw_sources[0]).unwrap(), source_text.as_bytes());

  let brain_status = codex_brain_status();
  assert_eq!(brain_status["status"], "ready", "{brain_status:#}");
  let runtime = runtime_descriptor(&BrainSettings::default());
  assert_eq!(runtime["adapter"]["kind"], "codex_sdk_profile");

  let compile_result =
    compile_graph_workspace_with_runtime_and_credentials(&paths, &runtime, &NoopCredentialResolver).unwrap();
  assert_eq!(compile_result["status"], "review_ready", "{compile_result:#}");
  assert_eq!(compile_result["run"]["status"], "completed", "{compile_result:#}");
  assert!(compile_result["proposalCount"].as_i64().unwrap_or_default() >= 3, "{compile_result:#}");

  let mut store = WorkspaceStore::open(&paths.database_path).unwrap();
  let compile_queue = store.load_review_queue().unwrap();
  let accepted_nodes = accept_review_items(&mut store, &compile_queue, "node");
  let accepted_edges = accept_review_items(&mut store, &compile_queue, "edge");
  assert!(accepted_nodes >= 2, "{compile_queue:#}");
  assert!(accepted_edges >= 1, "{compile_queue:#}");
  let compiled_graph = store.load_graph_snapshot().unwrap();
  assert!(compiled_graph["nodes"].as_array().unwrap().len() >= 2, "{compiled_graph:#}");
  assert!(!compiled_graph["edges"].as_array().unwrap().is_empty(), "{compiled_graph:#}");
  for node in compiled_graph["nodes"].as_array().unwrap() {
    let node_id = node["id"].as_str().unwrap();
    let detail = store.load_graph_node_detail(node_id).unwrap();
    assert!(!detail["compiled_body"].as_str().unwrap_or("").trim().is_empty(), "{detail:#}");
    assert!(!detail["evidence"].as_array().unwrap().is_empty(), "{detail:#}");
    assert_eq!(detail["body_version"], 1, "{detail:#}");
  }
  let anchor_title = compiled_graph["nodes"][0]["title"].as_str().unwrap().to_string();
  let compiled_node_count = compiled_graph["nodes"].as_array().unwrap().len();
  drop(store);

  let graph_chat_prompt = format!(
    "Create exactly one new concept node titled 'Live Verification Boundary' and connect it to '{anchor_title}'. \
     The node body must explain that live model output is accepted only after schema, evidence, and \
     persistence checks. \
     Return a valid proposed_graph_patch with no ambiguities or merge candidates."
  );
  let graph_chat =
    send_graph_chat_turn_with_credentials(&paths, &runtime, &graph_chat_prompt, Vec::new(), &NoopCredentialResolver)
      .unwrap();
  assert_eq!(graph_chat["runtime_status"], "completed", "{graph_chat:#}");
  assert_eq!(graph_chat["runtime_adapter_kind"], "codex_sdk_profile", "{graph_chat:#}");
  assert_eq!(graph_chat["patch_import_status"], "accepted_to_graph", "{graph_chat:#}");

  let store = WorkspaceStore::open(&paths.database_path).unwrap();
  let graph_after_chat = store.load_graph_snapshot().unwrap();
  assert!(graph_after_chat["nodes"].as_array().unwrap().len() > compiled_node_count, "{graph_after_chat:#}");
  let verification_node = graph_after_chat["nodes"]
    .as_array()
    .unwrap()
    .iter()
    .find(|node| node["title"] == "Live Verification Boundary")
    .unwrap_or_else(|| panic!("live graph node was not persisted: {graph_after_chat:#}"));
  let verification_node_id = verification_node["id"].as_str().unwrap().to_string();
  let before_node_chat = store.load_graph_node_detail(&verification_node_id).unwrap();
  let before_version = before_node_chat["body_version"].as_i64().unwrap();
  drop(store);

  let node_chat = send_node_chat_turn_with_credentials(
    &paths,
    &runtime,
    &verification_node_id,
    concat!(
      "Update only this focused node with one append_section proposal. ",
      "The section_text must contain the exact marker LIVE_NODE_DOCUMENT_V2 and explain that the persisted ",
      "node document ",
      "was revised by a separate live Codex instance. Do not create nodes, edges, ambiguities, or merge candidates."
    ),
    true,
    ChatRuntimeExecution::new(&NoopCredentialResolver, AgentTaskCancellation::new()),
  )
  .unwrap();
  assert_eq!(node_chat["runtime_status"], "completed", "{node_chat:#}");
  assert_eq!(node_chat["patch_import_status"], "accepted_to_graph", "{node_chat:#}");

  let store = WorkspaceStore::open(&paths.database_path).unwrap();
  let final_detail = store.load_graph_node_detail(&verification_node_id).unwrap();
  assert!(final_detail["body_version"].as_i64().unwrap() > before_version, "{final_detail:#}");
  assert!(final_detail["compiled_body"].as_str().unwrap().contains("LIVE_NODE_DOCUMENT_V2"), "{final_detail:#}");
  assert!(final_detail["update_history"].as_array().unwrap().len() >= 2, "{final_detail:#}");
  assert!(!final_detail["evidence"].as_array().unwrap().is_empty(), "{final_detail:#}");
  assert!(store.list_graph_messages().unwrap().as_array().unwrap().len() >= 2);
  assert!(store.list_node_messages(&verification_node_id).unwrap().as_array().unwrap().len() >= 2);
  let final_graph = store.load_graph_snapshot().unwrap();

  let report = json!({
    "schema_version": 1,
    "brain_status": brain_status,
    "runtime": runtime,
    "workspace_dir": paths.workspace_dir,
    "database_path": paths.database_path,
    "source_stats": stats,
    "raw_source_path": raw_sources[0],
    "compile": {
      "status": compile_result["status"],
      "job_id": compile_result["job"]["jobId"],
      "proposal_count": compile_result["proposalCount"],
      "accepted_nodes": accepted_nodes,
      "accepted_edges": accepted_edges
    },
    "graph_chat": {
      "runtime_status": graph_chat["runtime_status"],
      "patch_import_status": graph_chat["patch_import_status"],
      "proposal_count": graph_chat["proposal_count"]
    },
    "node_chat": {
      "runtime_status": node_chat["runtime_status"],
      "patch_import_status": node_chat["patch_import_status"],
      "proposal_count": node_chat["proposal_count"],
      "node_id": verification_node_id,
      "body_version": final_detail["body_version"]
    },
    "final_graph": final_graph
  });
  let report_path = run_root.join("live-e2e-report.json");
  fs::write(&report_path, serde_json::to_vec_pretty(&report).unwrap()).unwrap();
  println!("live Codex report: {}", report_path.display());

  if !preserve_artifacts {
    fs::remove_dir_all(run_root).unwrap();
  }
}

fn accept_review_items(store: &mut WorkspaceStore, queue: &Value, item_type: &str) -> usize {
  let ids = queue["items"]
    .as_array()
    .unwrap()
    .iter()
    .filter(|item| item["type"] == item_type && item["status"] == "proposed")
    .filter_map(|item| item["id"].as_str().map(str::to_string))
    .collect::<Vec<_>>();
  for id in &ids {
    store.accept_graph_proposal(id, Some("Accepted by the live Codex E2E test.")).unwrap();
  }
  ids.len()
}

fn live_run_root() -> (PathBuf, bool) {
  if let Some(root) = env::var_os(LIVE_TEST_ROOT) {
    return (PathBuf::from(root).join(format!("run-{}", Uuid::new_v4())), true);
  }
  (env::temp_dir().join(format!("soma-live-codex-e2e-{}", Uuid::new_v4())), false)
}

fn files_in(path: &Path) -> Vec<PathBuf> {
  let mut files = fs::read_dir(path)
    .unwrap()
    .filter_map(Result::ok)
    .map(|entry| entry.path())
    .filter(|path| path.is_file())
    .collect::<Vec<_>>();
  files.sort();
  files
}
