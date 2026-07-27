use super::*;
use crate::repository::WorkspaceStore;
use crate::runtime_adapters::default_runtime_descriptor;
use crate::source_import::import_source_file;
use crate::workspace::create_workspace_dir;
use soma_ai_runtime::NoopCredentialResolver;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Barrier};
use std::thread;

fn create_graph_extraction_job(paths: &WorkspacePaths) -> CommandResult<Value> {
  let runtime = default_runtime_descriptor();
  create_graph_extraction_job_with_runtime(paths, &runtime)
}

fn run_compile_job(paths: &WorkspacePaths, job_id: &str, runtime: &Value) -> CommandResult<Value> {
  run_compile_job_with_credentials(paths, job_id, runtime, &NoopCredentialResolver)
}

fn compile_graph_workspace_with_runtime(paths: &WorkspacePaths, runtime: &Value) -> CommandResult<Value> {
  compile_graph_workspace_with_runtime_and_credentials(paths, runtime, &NoopCredentialResolver)
}

#[test]
fn creates_job_folder_and_imports_output_patch_for_review() {
  let root = std::env::temp_dir().join(format!("soma-job-test-{}", new_id()));
  let paths = create_workspace_dir(&root).unwrap();
  let source = root.join("source.md");
  fs::write(&source, "User: Graph patches need evidence.\n\nAssistant: Review keeps graph truth safe.").unwrap();
  import_source_file(&paths, &source).unwrap();

  let runtime = json!({
    "providerId": "openai",
    "model": "gpt-test",
    "endpoint": "https://api.example.test",
    "authProfile": "",
    "credentialConfigured": true,
    "adapter": {
      "kind": "api_provider",
      "status": "configured",
      "endpoint": "https://api.example.test",
      "requireApiKey": true
    }
  });
  let job = create_graph_extraction_job_with_runtime(&paths, &runtime).unwrap();
  let job_dir = PathBuf::from(job["jobDir"].as_str().unwrap());
  assert!(job_dir.join("instructions.md").exists());
  assert!(job_dir.join("chunks.json").exists());
  assert!(job_dir.join("output_patch.json").exists());
  assert_runtime_file(&job, &runtime);
  let empty_import = import_graph_patch_for_review(&paths, job["jobId"].as_str().unwrap()).unwrap();
  assert_eq!(empty_import["valid"], true);
  assert_eq!(empty_import["imported"], false);
  assert_eq!(empty_import["proposalCount"], 0);
  let listed = list_jobs(&paths).unwrap();
  let listed_job = listed["jobs"].as_array().unwrap().iter().find(|item| item["jobId"] == job["jobId"]).unwrap();
  assert_eq!(listed_job["outputPatchStatus"], "empty");
  assert_eq!(listed_job["outputPatchProposalCount"], 0);
  assert_eq!(listed_job["outputPatchImportable"], false);

  let warning_patch = json!({
    "schema_version": 1,
    "proposed_nodes": [],
    "proposed_edges": [],
    "proposed_node_body_updates": [],
    "proposed_edge_bridge_updates": [],
    "proposed_message_evidence_attachments": [],
    "proposed_paths": [],
    "ambiguities": [],
    "merge_candidates": [],
    "warnings": [{
      "title": "Patch warning",
      "message": "Source material already appears covered."
    }]
  });
  fs::write(job_dir.join("output_patch.json"), serde_json::to_string_pretty(&warning_patch).unwrap()).unwrap();
  let warning_job = get_job(&paths, job["jobId"].as_str().unwrap()).unwrap();
  assert_eq!(warning_job["job"]["outputPatchStatus"], "empty");
  assert_eq!(warning_job["job"]["outputPatchProposalCount"], 0);
  assert_eq!(warning_job["job"]["outputPatchImportable"], false);
  let warning_import = import_graph_patch_for_review(&paths, job["jobId"].as_str().unwrap()).unwrap();
  assert_eq!(warning_import["valid"], true);
  assert_eq!(warning_import["imported"], false);
  assert_eq!(warning_import["proposalCount"], 0);

  let chunks: Value = serde_json::from_str(&fs::read_to_string(job_dir.join("chunks.json")).unwrap()).unwrap();
  let chunk_id = chunks["chunks"][0]["chunk_id"].as_str().unwrap();
  let patch = json!({
    "schema_version": 1,
    "proposed_nodes": [{
      "temp_id": "node_review",
      "type": "concept",
      "title": "Reviewable Graph Patches",
      "compiled_body": "Graph patches stay reviewable before they become graph truth.",
      "source_chunk_ids": [chunk_id],
      "reason": "Source discusses patch review."
    }],
    "proposed_edges": [],
    "proposed_node_body_updates": [],
    "proposed_edge_bridge_updates": [],
    "proposed_message_evidence_attachments": [],
    "proposed_paths": [],
    "ambiguities": [],
    "merge_candidates": [],
    "warnings": []
  });
  fs::write(job_dir.join("output_patch.json"), serde_json::to_string_pretty(&patch).unwrap()).unwrap();
  let ready = get_job(&paths, job["jobId"].as_str().unwrap()).unwrap();
  assert_eq!(ready["job"]["outputPatchStatus"], "ready");
  assert_eq!(ready["job"]["outputPatchProposalCount"], 1);
  assert_eq!(ready["job"]["outputPatchImportable"], true);

  let job_id = job["jobId"].as_str().unwrap().to_string();
  let barrier = Arc::new(Barrier::new(2));
  let imports = (0..2)
    .map(|_| {
      let barrier = Arc::clone(&barrier);
      let paths = paths.clone();
      let job_id = job_id.clone();
      thread::spawn(move || {
        barrier.wait();
        import_graph_patch_for_review(&paths, &job_id).unwrap()
      })
    })
    .collect::<Vec<_>>()
    .into_iter()
    .map(|handle| handle.join().unwrap())
    .collect::<Vec<_>>();
  let imported = imports.iter().find(|result| result["imported"] == true).unwrap();
  assert_eq!(imported["valid"], true);
  assert_eq!(imported["imported"], true);
  assert_eq!(imported["proposalCount"], 1);
  let patch_id = imported["patchId"].as_str().unwrap();
  let duplicate = imports.iter().find(|result| result["alreadyImported"] == true).unwrap();
  assert_eq!(duplicate["valid"], true);
  assert_eq!(duplicate["imported"], false);
  assert_eq!(duplicate["alreadyImported"], true);
  assert_eq!(duplicate["patchId"], patch_id);
  assert_eq!(duplicate["proposalCount"], 1);
  assert!(has_patch_error(duplicate, "$", "already imported"));
  let conn = open_existing_database(&paths.database_path).unwrap();
  let patch_count: i64 =
    conn.query_row("SELECT COUNT(*) FROM graph_patches WHERE job_id = ?1", [&job_id], |row| row.get(0)).unwrap();
  let proposal_count: i64 =
    conn.query_row("SELECT COUNT(*) FROM graph_proposals WHERE patch_id = ?1", [patch_id], |row| row.get(0)).unwrap();
  assert_eq!(patch_count, 1);
  assert_eq!(proposal_count, 1);
  drop(conn);
  let imported_job = get_job(&paths, &job_id).unwrap();
  assert_eq!(imported_job["job"]["outputPatchImportable"], false);
  assert_eq!(imported_job["job"]["importedProposalCount"], 1);
  let _ = fs::remove_dir_all(root);
}

#[test]
fn graph_job_reports_bounded_deterministic_chunk_coverage() {
  let root = std::env::temp_dir().join(format!("soma-job-coverage-test-{}", new_id()));
  let paths = create_workspace_dir(&root).unwrap();
  let source = root.join("coverage.md");
  let content = (0..501).map(|index| format!("User: coverage chunk {index:03}")).collect::<Vec<_>>().join("\n");
  fs::write(&source, content).unwrap();
  let imported = import_source_file(&paths, &source).unwrap();
  assert_eq!(imported["chunkCount"], 501);

  let job = create_graph_extraction_job(&paths).unwrap();
  assert_eq!(job["chunkCount"], 500);
  assert_eq!(job["includedChunkCount"], 500);
  assert_eq!(job["totalChunkCount"], 501);
  assert_eq!(job["truncated"], true);

  let job_dir = PathBuf::from(job["jobDir"].as_str().unwrap());
  let metadata: Value = serde_json::from_str(&fs::read_to_string(job_dir.join("metadata.json")).unwrap()).unwrap();
  assert_eq!(metadata["included_chunk_count"], 500);
  assert_eq!(metadata["total_chunk_count"], 501);
  assert_eq!(metadata["truncated"], true);

  let chunks: Value = serde_json::from_str(&fs::read_to_string(job_dir.join("chunks.json")).unwrap()).unwrap();
  let chunks = chunks["chunks"].as_array().unwrap();
  assert_eq!(chunks.len(), 500);
  assert_eq!(chunks.first().unwrap()["content"], "coverage chunk 000");
  assert_eq!(chunks.last().unwrap()["content"], "coverage chunk 499");

  let listed = list_jobs(&paths).unwrap();
  let listed_job = &listed["jobs"][0];
  assert_eq!(listed_job["includedChunkCount"], 500);
  assert_eq!(listed_job["totalChunkCount"], 501);
  assert_eq!(listed_job["truncated"], true);
  let _ = fs::remove_dir_all(root);
}

#[test]
fn failed_job_publication_removes_its_staging_directory() {
  let root = std::env::temp_dir().join(format!("soma-job-publish-failure-test-{}", new_id()));
  let jobs_dir = root.join("jobs");

  let error = publish_job_directory(&jobs_dir, "job_forced_failure", |staging_dir| {
    fs::write(staging_dir.join("metadata.json"), "{}")?;
    Err(CommandError::storage("forced job write failure"))
  })
  .unwrap_err();

  assert_eq!(error.message, "forced job write failure");
  assert!(!jobs_dir.join("job_forced_failure").exists());
  assert_eq!(fs::read_dir(&jobs_dir).unwrap().count(), 0);
  let _ = fs::remove_dir_all(root);
}

#[test]
fn list_jobs_skips_incomplete_and_malformed_directories() {
  let root = std::env::temp_dir().join(format!("soma-job-list-isolation-test-{}", new_id()));
  let paths = create_workspace_dir(&root).unwrap();
  let source = root.join("source.md");
  fs::write(&source, "User: Job listing stays available.\n\nAssistant: Broken job folders remain isolated.").unwrap();
  import_source_file(&paths, &source).unwrap();

  let valid_job = create_graph_extraction_job(&paths).unwrap();
  let malformed_job = create_graph_extraction_job(&paths).unwrap();
  let malformed_dir = PathBuf::from(malformed_job["jobDir"].as_str().unwrap());
  fs::write(malformed_dir.join("metadata.json"), "{ invalid").unwrap();
  let jobs_dir = paths.workspace_dir.join(JOB_DIR);
  let incomplete_dir = jobs_dir.join("job_incomplete");
  fs::create_dir(&incomplete_dir).unwrap();
  fs::write(incomplete_dir.join("metadata.json"), "{}").unwrap();
  fs::create_dir(jobs_dir.join(format!("{JOB_STAGING_PREFIX}abandoned"))).unwrap();

  let listed = list_jobs(&paths).unwrap();
  let jobs = listed["jobs"].as_array().unwrap();
  assert_eq!(jobs.len(), 1);
  assert_eq!(jobs[0]["jobId"], valid_job["jobId"]);
  let _ = fs::remove_dir_all(root);
}

#[test]
fn oversized_output_patch_is_not_loaded_or_imported() {
  let (root, paths, job) = create_runtime_test_job(default_runtime_descriptor());
  let job_id = job["jobId"].as_str().unwrap();
  let output_path = PathBuf::from(job["jobDir"].as_str().unwrap()).join("output_patch.json");
  fs::File::create(&output_path).unwrap().set_len(crate::job_files::OUTPUT_PATCH_MAX_BYTES + 1).unwrap();

  let listed = list_jobs(&paths).unwrap();
  let listed_job = listed["jobs"].as_array().unwrap().iter().find(|item| item["jobId"] == job_id).unwrap();
  assert_eq!(listed_job["outputPatchStatus"], "invalid");
  assert_eq!(listed_job["outputPatchImportable"], false);

  let imported = import_graph_patch_for_review(&paths, job_id).unwrap();
  assert_eq!(imported["valid"], false);
  assert!(has_patch_error(&imported, "$", "byte safety limit"));
  let queue = WorkspaceStore::open(&paths.database_path).unwrap().load_review_queue().unwrap();
  assert!(queue["items"].as_array().unwrap().is_empty());
  let _ = fs::remove_dir_all(root);
}

#[test]
fn patch_import_reports_malformed_shape_and_unknown_chunk_without_persisting_review() {
  let (root, paths, job) = create_runtime_test_job(default_runtime_descriptor());
  let job_id = job["jobId"].as_str().unwrap();
  let output_path = PathBuf::from(job["jobDir"].as_str().unwrap()).join("output_patch.json");

  fs::write(&output_path, "{ nope").unwrap();
  let malformed = import_graph_patch_for_review(&paths, job_id).unwrap();
  assert_eq!(malformed["valid"], false);
  assert!(has_patch_error(&malformed, "$", "invalid JSON"));

  fs::write(
    &output_path,
    serde_json::to_string(&json!({
      "schema_version": 1,
      "proposed_nodes": {}
    }))
    .unwrap(),
  )
  .unwrap();
  let wrong_shape = import_graph_patch_for_review(&paths, job_id).unwrap();
  assert_eq!(wrong_shape["valid"], false);
  assert_eq!(wrong_shape["imported"], false);
  assert!(has_patch_error(&wrong_shape, "$.proposed_nodes", "must be an array"));

  let mut unknown_chunk = empty_graph_patch();
  unknown_chunk["proposed_nodes"] = json!([{
    "temp_id": "node_unknown_chunk",
    "type": "concept",
    "title": "Unknown evidence",
    "compiled_body": "This proposal cites evidence outside the compile job.",
    "source_chunk_ids": ["missing_chunk"],
    "reason": "Exercise the job evidence boundary."
  }]);
  fs::write(&output_path, serde_json::to_string(&unknown_chunk).unwrap()).unwrap();
  let unknown_chunk = import_graph_patch_for_review(&paths, job_id).unwrap();
  assert_eq!(unknown_chunk["valid"], false);
  assert_eq!(unknown_chunk["imported"], false);
  assert!(has_patch_error(&unknown_chunk, ".source_chunk_ids[0]", "Unknown chunk id"));

  let queue = WorkspaceStore::open(&paths.database_path).unwrap().load_review_queue().unwrap();
  assert!(queue["items"].as_array().unwrap().is_empty());
  let _ = fs::remove_dir_all(root);
}

#[test]
fn patch_import_rejects_missing_evidence_and_unsupported_node_type() {
  let (root, paths, job) = create_runtime_test_job(default_runtime_descriptor());
  let job_id = job["jobId"].as_str().unwrap();
  let job_dir = PathBuf::from(job["jobDir"].as_str().unwrap());
  let output_path = job_dir.join("output_patch.json");

  let mut missing_evidence = empty_graph_patch();
  missing_evidence["proposed_nodes"] = json!([{
    "temp_id": "node_without_evidence",
    "type": "concept",
    "title": "Unsupported assertion",
    "compiled_body": "AI-authored graph truth must remain traceable.",
    "reason": "Exercise the evidence invariant."
  }]);
  fs::write(&output_path, serde_json::to_string(&missing_evidence).unwrap()).unwrap();
  let missing_evidence = import_graph_patch_for_review(&paths, job_id).unwrap();
  assert_eq!(missing_evidence["valid"], false);
  assert_eq!(missing_evidence["imported"], false);
  assert!(has_patch_error(&missing_evidence, ".source_chunk_ids", "source chunk id or source message id"));

  let chunks: Value = serde_json::from_str(&fs::read_to_string(job_dir.join("chunks.json")).unwrap()).unwrap();
  let chunk_id = chunks["chunks"][0]["chunk_id"].as_str().unwrap();
  let mut unsupported_type = empty_graph_patch();
  unsupported_type["proposed_nodes"] = json!([{
    "temp_id": "node_unsupported_type",
    "type": "related_to",
    "title": "Invalid node taxonomy",
    "compiled_body": "Node and edge taxonomies are separate.",
    "source_chunk_ids": [chunk_id],
    "reason": "Exercise the graph contract."
  }]);
  fs::write(&output_path, serde_json::to_string(&unsupported_type).unwrap()).unwrap();
  let unsupported_type = import_graph_patch_for_review(&paths, job_id).unwrap();
  assert_eq!(unsupported_type["valid"], false);
  assert_eq!(unsupported_type["imported"], false);
  assert!(has_patch_error(&unsupported_type, ".type", "Unsupported node type"));

  let queue = WorkspaceStore::open(&paths.database_path).unwrap().load_review_queue().unwrap();
  assert!(queue["items"].as_array().unwrap().is_empty());
  let _ = fs::remove_dir_all(root);
}

#[test]
fn stale_job_mutations_cannot_overwrite_newer_node_or_edge_state() {
  let root = std::env::temp_dir().join(format!("soma-stale-job-test-{}", new_id()));
  let paths = create_workspace_dir(&root).unwrap();
  let source = root.join("source.md");
  fs::write(
    &source,
    "User: Jobs may propose delayed graph updates.\n\nAssistant: Acceptance must preserve newer graph state.",
  )
  .unwrap();
  import_source_file(&paths, &source).unwrap();

  let mut store = WorkspaceStore::open(&paths.database_path).unwrap();
  let seed_message = store.append_graph_message("Create a source and target node.", Vec::new()).unwrap();
  let seed_message_id = seed_message["message"]["id"].as_str().unwrap();
  let mut seed_patch = empty_graph_patch();
  seed_patch["proposed_nodes"] = json!([
    {
      "temp_id": "node_job_source",
      "type": "concept",
      "title": "Job Source",
      "compiled_body": "The original node body captured by the delayed job."
    },
    {
      "temp_id": "node_job_target",
      "type": "concept",
      "title": "Job Target",
      "compiled_body": "A target node anchors the bridge update."
    }
  ]);
  seed_patch["proposed_edges"] = json!([{
    "source_temp_id": "node_job_source",
    "target_temp_id": "node_job_target",
    "type": "supports",
    "bridge_text": "The original edge bridge captured by the delayed job.",
    "reason": "Seed an existing edge for the optimistic concurrency test."
  }]);
  let seeded = store.propose_graph_updates(seed_message_id, seed_patch).unwrap();
  assert_eq!(seeded["valid"], true);
  let seed_patch_id = seeded["patchId"].as_str().unwrap();
  let queue = store.load_review_queue().unwrap();
  let seed_node_proposal_ids = queue["items"]
    .as_array()
    .unwrap()
    .iter()
    .filter(|item| item["patch_id"] == seed_patch_id && item["type"] == "node")
    .map(|item| item["id"].as_str().unwrap().to_string())
    .collect::<Vec<_>>();
  assert_eq!(seed_node_proposal_ids.len(), 2);
  for proposal_id in seed_node_proposal_ids {
    store.accept_graph_proposal(&proposal_id, None).unwrap();
  }
  let queue = store.load_review_queue().unwrap();
  let seed_edge_proposal_id = queue["items"]
    .as_array()
    .unwrap()
    .iter()
    .find(|item| item["patch_id"] == seed_patch_id && item["type"] == "edge")
    .unwrap()["id"]
    .as_str()
    .unwrap()
    .to_string();
  store.accept_graph_proposal(&seed_edge_proposal_id, None).unwrap();

  let graph = store.load_graph_snapshot().unwrap();
  let node_id = graph["nodes"].as_array().unwrap().iter().find(|node| node["title"] == "Job Source").unwrap()["id"]
    .as_str()
    .unwrap()
    .to_string();
  let edge_id = graph["edges"][0]["id"].as_str().unwrap().to_string();

  let legacy_message =
    store.append_graph_message("Legacy proposals have no optimistic preconditions.", Vec::new()).unwrap();
  let legacy_message_id = legacy_message["message"]["id"].as_str().unwrap();
  let mut legacy_patch = empty_graph_patch();
  legacy_patch["proposed_node_body_updates"] = json!([{
    "target_node_id": node_id,
    "update_kind": "replace_body",
    "compiled_body": "An unversioned body update must remain reviewable.",
    "reason": "Exercise legacy proposal handling."
  }]);
  legacy_patch["proposed_edge_bridge_updates"] = json!([{
    "target_edge_id": edge_id,
    "bridge_text": "An unversioned bridge update must remain reviewable.",
    "reason": "Exercise legacy proposal handling."
  }]);
  let legacy = store.propose_graph_updates(legacy_message_id, legacy_patch).unwrap();
  let legacy_patch_id = legacy["patchId"].as_str().unwrap();
  let queue = store.load_review_queue().unwrap();
  for (proposal_type, message_fragment) in
    [("node_body_update", "no snapshot precondition"), ("edge_bridge_update", "no snapshot precondition")]
  {
    let proposal_id = queue["items"]
      .as_array()
      .unwrap()
      .iter()
      .find(|item| item["patch_id"] == legacy_patch_id && item["type"] == proposal_type)
      .unwrap()["id"]
      .as_str()
      .unwrap();
    let error = store.accept_graph_proposal(proposal_id, None).unwrap_err();
    assert_eq!(error.code, "Soma_VALIDATION_ERROR");
    assert!(error.message.contains(message_fragment));
  }
  let queue = store.load_review_queue().unwrap();
  assert!(queue["items"]
    .as_array()
    .unwrap()
    .iter()
    .filter(|item| item["patch_id"] == legacy_patch_id)
    .all(|item| item["status"] == "proposed"));

  let job = create_graph_extraction_job(&paths).unwrap();
  let job_id = job["jobId"].as_str().unwrap();
  let job_dir = PathBuf::from(job["jobDir"].as_str().unwrap());
  let job_snapshot: Value =
    serde_json::from_str(&fs::read_to_string(job_dir.join("current_graph_snapshot.json")).unwrap()).unwrap();
  let base_body_version_id =
    job_snapshot["nodes"].as_array().unwrap().iter().find(|node| node["id"] == node_id).unwrap()["body_version_id"]
      .as_str()
      .unwrap()
      .to_string();
  let base_edge_updated_at =
    job_snapshot["edges"].as_array().unwrap().iter().find(|edge| edge["id"] == edge_id).unwrap()["updated_at"]
      .as_str()
      .unwrap()
      .to_string();

  store.update_node_body(&node_id, "A newer user-authored node body must survive stale job acceptance.").unwrap();
  let newer_edge_message =
    store.append_graph_message("Use a newer edge bridge before the delayed job returns.", Vec::new()).unwrap();
  let newer_edge_message_id = newer_edge_message["message"]["id"].as_str().unwrap();
  let mut newer_edge_patch = empty_graph_patch();
  newer_edge_patch["proposed_edge_bridge_updates"] = json!([{
    "target_edge_id": edge_id,
    "base_edge_updated_at": base_edge_updated_at,
    "bridge_text": "A newer accepted edge bridge must survive stale job acceptance.",
    "reason": "Simulate a newer mutation while the compile job is delayed."
  }]);
  let newer_edge = store.propose_graph_updates(newer_edge_message_id, newer_edge_patch).unwrap();
  let newer_edge_patch_id = newer_edge["patchId"].as_str().unwrap();
  let queue = store.load_review_queue().unwrap();
  let newer_edge_proposal_id = queue["items"]
    .as_array()
    .unwrap()
    .iter()
    .find(|item| item["patch_id"] == newer_edge_patch_id && item["type"] == "edge_bridge_update")
    .unwrap()["id"]
    .as_str()
    .unwrap()
    .to_string();
  store.accept_graph_proposal(&newer_edge_proposal_id, None).unwrap();

  let current_graph = store.load_graph_snapshot().unwrap();
  let current_node = current_graph["nodes"].as_array().unwrap().iter().find(|node| node["id"] == node_id).unwrap();
  let current_edge = current_graph["edges"].as_array().unwrap().iter().find(|edge| edge["id"] == edge_id).unwrap();
  let current_body_version_id = current_node["body_version_id"].as_str().unwrap();
  let current_edge_updated_at = current_edge["updated_at"].as_str().unwrap();
  assert_ne!(current_body_version_id, base_body_version_id);
  assert_ne!(current_edge_updated_at, base_edge_updated_at);

  let chunks: Value = serde_json::from_str(&fs::read_to_string(job_dir.join("chunks.json")).unwrap()).unwrap();
  let chunk_id = chunks["chunks"][0]["chunk_id"].as_str().unwrap();
  let mut delayed_patch = empty_graph_patch();
  delayed_patch["proposed_node_body_updates"] = json!([{
    "target_node_id": node_id,
    "base_body_version_id": current_body_version_id,
    "update_kind": "replace_body",
    "compiled_body": "This delayed node body must not replace newer graph truth.",
    "source_chunk_ids": [chunk_id],
    "reason": "Return a delayed node mutation."
  }]);
  delayed_patch["proposed_edge_bridge_updates"] = json!([{
    "target_edge_id": edge_id,
    "base_edge_updated_at": current_edge_updated_at,
    "bridge_text": "This delayed bridge must not replace newer graph truth.",
    "source_chunk_ids": [chunk_id],
    "reason": "Return a delayed edge mutation."
  }]);
  fs::write(job_dir.join("output_patch.json"), serde_json::to_string_pretty(&delayed_patch).unwrap()).unwrap();

  let imported = import_graph_patch_for_review(&paths, job_id).unwrap();
  assert_eq!(imported["valid"], true);
  assert_eq!(imported["proposalCount"], 2);
  let imported_patch_id = imported["patchId"].as_str().unwrap();
  let conn = open_existing_database(&paths.database_path).unwrap();
  let node_payload_json: String = conn
    .query_row(
      "SELECT payload_json FROM graph_proposals WHERE patch_id = ?1 AND proposal_type = 'node_body_update'",
      [imported_patch_id],
      |row| row.get(0),
    )
    .unwrap();
  let edge_payload_json: String = conn
    .query_row(
      "SELECT payload_json FROM graph_proposals WHERE patch_id = ?1 AND proposal_type = 'edge_bridge_update'",
      [imported_patch_id],
      |row| row.get(0),
    )
    .unwrap();
  let node_payload: Value = serde_json::from_str(&node_payload_json).unwrap();
  let edge_payload: Value = serde_json::from_str(&edge_payload_json).unwrap();
  assert_eq!(node_payload["base_body_version_id"], base_body_version_id);
  assert_eq!(edge_payload["base_edge_updated_at"], base_edge_updated_at);
  drop(conn);

  let queue = store.load_review_queue().unwrap();
  let node_update_id = queue["items"]
    .as_array()
    .unwrap()
    .iter()
    .find(|item| item["patch_id"] == imported_patch_id && item["type"] == "node_body_update")
    .unwrap()["id"]
    .as_str()
    .unwrap()
    .to_string();
  let edge_update_id = queue["items"]
    .as_array()
    .unwrap()
    .iter()
    .find(|item| item["patch_id"] == imported_patch_id && item["type"] == "edge_bridge_update")
    .unwrap()["id"]
    .as_str()
    .unwrap()
    .to_string();
  let node_error = store.accept_graph_proposal(&node_update_id, None).unwrap_err();
  let edge_error = store.accept_graph_proposal(&edge_update_id, None).unwrap_err();
  assert_eq!(node_error.code, "Soma_VALIDATION_ERROR");
  assert!(node_error.message.contains("node body changed"));
  assert_eq!(edge_error.code, "Soma_VALIDATION_ERROR");
  assert!(edge_error.message.contains("edge changed"));

  let final_detail = store.load_graph_node_detail(&node_id).unwrap();
  let final_graph = store.load_graph_snapshot().unwrap();
  let final_edge = final_graph["edges"].as_array().unwrap().iter().find(|edge| edge["id"] == edge_id).unwrap();
  assert_eq!(final_detail["compiled_body"], "A newer user-authored node body must survive stale job acceptance.");
  assert_eq!(final_edge["bridge_text"], "A newer accepted edge bridge must survive stale job acceptance.");
  let _ = fs::remove_dir_all(root);
}

#[test]
fn clears_job_history_without_removing_workspace_sources() {
  let root = std::env::temp_dir().join(format!("soma-job-clear-test-{}", new_id()));
  let paths = create_workspace_dir(&root).unwrap();
  let source = root.join("source.md");
  fs::write(&source, "User: Job history is temporary.\n\nAssistant: Source chunks stay searchable.").unwrap();
  import_source_file(&paths, &source).unwrap();

  let job = create_graph_extraction_job(&paths).unwrap();
  let job_dir = PathBuf::from(job["jobDir"].as_str().unwrap());
  assert!(job_dir.exists());
  assert_eq!(list_jobs(&paths).unwrap()["jobs"].as_array().unwrap().len(), 1);

  let cleared = clear_job_history(&paths).unwrap();

  assert_eq!(cleared["removed"], 1);
  assert!(!job_dir.exists());
  assert_eq!(list_jobs(&paths).unwrap()["jobs"].as_array().unwrap().len(), 0);
  let conn = open_existing_database(&paths.database_path).unwrap();
  let chunk_count: i64 = conn.query_row("SELECT COUNT(*) FROM chunks", [], |row| row.get(0)).unwrap();
  assert!(chunk_count > 0);
  let _ = fs::remove_dir_all(root);
}

#[test]
fn run_compile_job_reports_api_adapter_missing_key_as_failed() {
  let runtime = json!({
    "providerId": "openai",
    "model": "gpt-test",
    "endpoint": "https://api.example.test",
    "credentialConfigured": true,
    "adapter": {
      "kind": "api_provider",
      "status": "configured",
      "endpoint": "https://api.example.test",
      "requireApiKey": true
    }
  });
  let (root, paths, job) = create_runtime_test_job(runtime.clone());

  let result = run_compile_job(&paths, job["jobId"].as_str().unwrap(), &runtime).unwrap();

  assert_eq!(result["status"], "failed");
  assert_eq!(result["adapterKind"], "api_provider");
  assert_eq!(result["failureKind"], "credential");
  assert_eq!(result["outputPatchStatus"], "empty");
  assert_eq!(result["outputPatchImportable"], false);
  let listed = list_jobs(&paths).unwrap();
  let listed_job = listed["jobs"].as_array().unwrap().iter().find(|item| item["jobId"] == job["jobId"]).unwrap();
  assert_eq!(listed_job["runtimeStatus"], "failed");
  assert_eq!(listed_job["runtimeFailureKind"], "credential");
  assert_eq!(listed_job["runtimeAdapterKind"], "api_provider");
  assert_eq!(listed_job["runtimeMessage"], result["message"]);
  assert!(PathBuf::from(job["jobDir"].as_str().unwrap()).join("runtime_result.json").exists());
  let _ = fs::remove_dir_all(root);
}

#[test]
fn rerun_ignores_tampered_job_runtime_authority() {
  let listener = TcpListener::bind("127.0.0.1:0").unwrap();
  listener.set_nonblocking(true).unwrap();
  let endpoint = format!("http://{}", listener.local_addr().unwrap());
  let trusted_runtime = json!({
    "providerId": "local_llm",
    "model": "trusted-model",
    "endpoint": endpoint,
    "authProfile": "",
    "credentialConfigured": false,
    "adapter": {
      "kind": "local_offline_endpoint",
      "endpoint": endpoint
    }
  });
  let (root, paths, job) = create_runtime_test_job(trusted_runtime.clone());
  let job_dir = PathBuf::from(job["jobDir"].as_str().unwrap());
  fs::write(
    job_dir.join("runtime.json"),
    serde_json::to_vec_pretty(&json!({
      "providerId": "attacker_provider",
      "model": "attacker-model",
      "endpoint": "http://127.0.0.1:1",
      "authProfile": "stolen",
      "credentialConfigured": true,
      "adapter": {
        "kind": "api_provider",
        "endpoint": "http://127.0.0.1:1",
        "requireApiKey": true
      }
    }))
    .unwrap(),
  )
  .unwrap();
  let server = thread::spawn(move || {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    let (mut stream, _) = loop {
      match listener.accept() {
        Ok(connection) => break connection,
        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock && std::time::Instant::now() < deadline => {
          thread::sleep(std::time::Duration::from_millis(10));
        }
        Err(error) => panic!("trusted runtime request was not received: {error}"),
      }
    };
    let request = read_http_request(&mut stream);
    let content = empty_graph_patch().to_string();
    let body = json!({ "choices": [{ "message": { "content": content } }] }).to_string();
    let response = format!(
      "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
      body.len(),
      body
    );
    stream.write_all(response.as_bytes()).unwrap();
    request
  });

  let result = run_compile_job_with_credentials(
    &paths,
    job["jobId"].as_str().unwrap(),
    &trusted_runtime,
    &RejectCredentialResolver,
  )
  .unwrap();
  let request = server.join().unwrap();

  assert_eq!(result["status"], "completed");
  assert_eq!(result["adapterKind"], "local_offline_endpoint");
  assert!(request.contains("\"model\":\"trusted-model\""));
  let displayed_runtime: Value =
    serde_json::from_str(&fs::read_to_string(job_dir.join("runtime.json")).unwrap()).unwrap();
  assert_eq!(displayed_runtime, trusted_runtime);
  let _ = fs::remove_dir_all(root);
}

#[test]
fn run_compile_job_executes_profile_command_and_keeps_review_flow() {
  let _guard = crate::runtime_adapters::RUNTIME_ENV_LOCK.lock().unwrap();
  let runtime = json!({
    "providerId": "codex_sdk",
    "model": "test-model",
    "endpoint": "",
    "credentialConfigured": false,
    "adapter": {
      "kind": "codex_sdk_profile",
      "profile": "default"
    }
  });
  let (root, paths, job) = create_runtime_test_job(runtime.clone());
  let job_dir = PathBuf::from(job["jobDir"].as_str().unwrap());
  let chunks: Value = serde_json::from_str(&fs::read_to_string(job_dir.join("chunks.json")).unwrap()).unwrap();
  let chunk_id = chunks["chunks"][0]["chunk_id"].as_str().unwrap();
  let patch = json!({
    "schema_version": 1,
    "proposed_nodes": [{
      "temp_id": "node_runtime",
      "type": "concept",
      "title": "Runtime Compile",
      "compiled_body": "Runtime adapters can fill output patches while review keeps graph truth safe.",
      "source_chunk_ids": [chunk_id],
      "reason": "Fixture runtime output."
    }],
    "proposed_edges": [],
    "proposed_node_body_updates": [],
    "proposed_edge_bridge_updates": [],
    "proposed_message_evidence_attachments": [],
    "proposed_paths": [],
    "ambiguities": [],
    "merge_candidates": [],
    "warnings": []
  });
  let script = root.join(if cfg!(windows) { "fake-runtime.cmd" } else { "fake-runtime.sh" });
  write_runtime_script(&script, &patch);
  let previous_command = std::env::var_os("SOMA_CODEX_COMMAND");
  std::env::set_var("SOMA_CODEX_COMMAND", script.to_string_lossy().to_string());

  let result = run_compile_job(&paths, job["jobId"].as_str().unwrap(), &runtime).unwrap();
  restore_env_var("SOMA_CODEX_COMMAND", previous_command);

  assert_eq!(result["status"], "completed");
  assert_eq!(result["adapterKind"], "codex_sdk_profile");
  assert_eq!(result["outputPatchStatus"], "ready");
  assert_eq!(result["outputPatchImportable"], true);
  let listed = list_jobs(&paths).unwrap();
  let listed_job = listed["jobs"].as_array().unwrap().iter().find(|item| item["jobId"] == job["jobId"]).unwrap();
  assert_eq!(listed_job["runtimeStatus"], "completed");
  assert_eq!(listed_job["runtimeAdapterKind"], "codex_sdk_profile");
  let imported = import_graph_patch_for_review(&paths, job["jobId"].as_str().unwrap()).unwrap();
  assert_eq!(imported["valid"], true);
  assert_eq!(imported["proposalCount"], 1);
  let _ = fs::remove_dir_all(root);
}

#[test]
fn failed_rerun_cannot_reuse_a_previous_successful_output_patch() {
  let _guard = crate::runtime_adapters::RUNTIME_ENV_LOCK.lock().unwrap();
  let runtime = json!({
    "providerId": "codex_sdk",
    "model": "test-model",
    "endpoint": "",
    "credentialConfigured": false,
    "adapter": {
      "kind": "codex_sdk_profile",
      "profile": "default"
    }
  });
  let (root, paths, job) = create_runtime_test_job(runtime.clone());
  let job_id = job["jobId"].as_str().unwrap();
  let job_dir = PathBuf::from(job["jobDir"].as_str().unwrap());
  let chunks: Value = serde_json::from_str(&fs::read_to_string(job_dir.join("chunks.json")).unwrap()).unwrap();
  let chunk_id = chunks["chunks"][0]["chunk_id"].as_str().unwrap();
  let mut patch = empty_graph_patch();
  patch["proposed_nodes"] = json!([{
    "temp_id": "node_previous_run",
    "type": "concept",
    "title": "Previous Runtime Output",
    "compiled_body": "A failed rerun must not expose this earlier successful output.",
    "source_chunk_ids": [chunk_id],
    "reason": "Exercise stale runtime output invalidation."
  }]);
  let script = root.join(if cfg!(windows) { "successful-runtime.cmd" } else { "successful-runtime.sh" });
  write_runtime_script(&script, &patch);
  let previous_command = std::env::var_os("SOMA_CODEX_COMMAND");
  std::env::set_var("SOMA_CODEX_COMMAND", script.to_string_lossy().to_string());

  let first_run = run_compile_job(&paths, job_id, &runtime).unwrap();
  assert_eq!(first_run["status"], "completed");
  assert_eq!(first_run["outputPatchStatus"], "ready");
  assert_eq!(first_run["outputPatchImportable"], true);

  let missing_runtime = root.join("missing-runtime-command");
  std::env::set_var("SOMA_CODEX_COMMAND", missing_runtime.to_string_lossy().to_string());
  let failed_rerun = run_compile_job(&paths, job_id, &runtime).unwrap();
  restore_env_var("SOMA_CODEX_COMMAND", previous_command);

  assert_eq!(failed_rerun["status"], "failed");
  assert_eq!(failed_rerun["wroteOutputPatch"], false);
  assert_eq!(failed_rerun["outputPatchStatus"], "empty");
  assert_eq!(failed_rerun["outputPatchProposalCount"], 0);
  assert_eq!(failed_rerun["outputPatchImportable"], false);
  let listed = list_jobs(&paths).unwrap();
  let listed_job = listed["jobs"].as_array().unwrap().iter().find(|item| item["jobId"] == job_id).unwrap();
  assert_eq!(listed_job["outputPatchStatus"], "empty");
  assert_eq!(listed_job["outputPatchImportable"], false);
  let imported = import_graph_patch_for_review(&paths, job_id).unwrap();
  assert_eq!(imported["valid"], true);
  assert_eq!(imported["imported"], false);
  assert_eq!(imported["proposalCount"], 0);
  let _ = fs::remove_dir_all(root);
}

#[test]
fn rejects_job_directory_link_outside_job_root() {
  let root = std::env::temp_dir().join(format!("soma-job-link-test-{}", new_id()));
  let outside = std::env::temp_dir().join(format!("soma-job-link-target-{}", new_id()));
  let paths = create_workspace_dir(&root).unwrap();
  fs::create_dir_all(&outside).unwrap();
  let linked_job = paths.workspace_dir.join(JOB_DIR).join("job_escape");
  create_directory_link(&outside, &linked_job).unwrap();

  let error = resolve_existing_job_dir(&paths, "job_escape").unwrap_err();

  assert_eq!(error.code, "Soma_VALIDATION_ERROR");
  assert!(error.message.contains("regular directory"));
  remove_directory_link(&linked_job).unwrap();
  let _ = fs::remove_dir_all(outside);
  let _ = fs::remove_dir_all(root);
}

#[test]
fn run_and_import_reject_escaping_output_reparse_target() {
  let runtime = default_runtime_descriptor();
  let (root, paths, job) = create_runtime_test_job(runtime.clone());
  let job_id = job["jobId"].as_str().unwrap();
  let job_dir = PathBuf::from(job["jobDir"].as_str().unwrap());
  let output_path = job_dir.join("output_patch.json");
  fs::remove_file(&output_path).unwrap();
  let outside = std::env::temp_dir().join(format!("soma-output-link-target-{}", new_id()));
  fs::create_dir_all(&outside).unwrap();
  let sentinel = outside.join("sentinel.txt");
  fs::write(&sentinel, "unchanged").unwrap();
  create_directory_link(&outside, &output_path).unwrap();

  let run_error = run_compile_job_with_credentials(&paths, job_id, &runtime, &NoopCredentialResolver).unwrap_err();
  let import_error = import_graph_patch_for_review(&paths, job_id).unwrap_err();
  let get_error = get_job(&paths, job_id).unwrap_err();

  for error in [run_error, import_error, get_error] {
    assert_eq!(error.code, "Soma_VALIDATION_ERROR");
    assert!(error.message.contains("output_patch.json"));
  }
  assert_eq!(fs::read_to_string(&sentinel).unwrap(), "unchanged");
  remove_directory_link(&output_path).unwrap();
  let _ = fs::remove_dir_all(outside);
  let _ = fs::remove_dir_all(root);
}

#[test]
fn compile_graph_workspace_runs_and_imports_review_updates() {
  let root = std::env::temp_dir().join(format!("soma-compile-flow-test-{}", new_id()));
  let paths = create_workspace_dir(&root).unwrap();
  let source = root.join("source.md");
  fs::write(
    &source,
    concat!(
      "User: Soma should compile one graph flow.\n\n",
      "Assistant: Compile Graph should open Review Updates with proposed nodes."
    ),
  )
  .unwrap();
  import_source_file(&paths, &source).unwrap();

  let listener = TcpListener::bind("127.0.0.1:0").unwrap();
  let endpoint = format!("http://{}", listener.local_addr().unwrap());
  let server = thread::spawn(move || {
    let (mut stream, _) = listener.accept().unwrap();
    let request = read_http_request(&mut stream);
    let chunk_id = first_uuid_after(&request, "chunk_id");
    let patch = json!({
      "schema_version": 1,
      "proposed_nodes": [
        {
          "temp_id": "node_compile_flow",
          "title": "Compile Flow",
          "compiled_body": concat!(
            "Compile Graph should create a job, run the selected brain, ",
            "and import reviewable proposals in one product action."
          ),
          "source_chunk_ids": [chunk_id],
          "reason": "Regression fixture for the atomic compile workflow."
        },
        {
          "temp_id": "node_review_updates",
          "title": "Review Updates",
          "preview": "Valid compiler output becomes reviewable proposals before graph truth changes.",
          "compiled_body": concat!(
            "Review Updates receives graph proposals produced by the compiler ",
            "and keeps them untrusted until the user accepts them into the active graph."
          ),
          "source_chunk_ids": [chunk_id]
        }
      ],
      "proposed_edges": [{
        "source_temp_id": "node_compile_flow",
        "target_temp_id": "node_review_updates",
        "edge_type": "clarifies",
        "bridge_text": "Compile Graph opens Review Updates instead of mutating graph truth directly.",
        "source_chunk_ids": [chunk_id]
      }],
      "proposed_node_body_updates": [],
      "proposed_edge_bridge_updates": [],
      "proposed_message_evidence_attachments": [],
      "proposed_paths": [],
      "ambiguities": [],
      "merge_candidates": [],
      "warnings": []
    });
    let body = json!({
      "choices": [{
        "message": {
          "content": patch.to_string()
        }
      }]
    })
    .to_string();
    let response = format!(
      "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
      body.len(),
      body
    );
    let _ = stream.write_all(response.as_bytes());
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

  let result = compile_graph_workspace_with_runtime(&paths, &runtime).unwrap();
  server.join().unwrap();

  assert_eq!(result["status"], "review_ready");
  assert_eq!(result["proposalCount"], 3);
  assert_eq!(result["importResult"]["imported"], true);
  assert!(result["importResult"]["warnings"]
    .as_array()
    .unwrap()
    .iter()
    .any(|warning| warning["message"].as_str().unwrap_or("").contains("Missing node type normalized")));
  assert!(result["importResult"]["warnings"]
    .as_array()
    .unwrap()
    .iter()
    .any(|warning| warning["message"].as_str().unwrap_or("").contains("Normalized edge type")));
  assert_eq!(result["run"]["outputPatchImportable"], true);
  let mut store = WorkspaceStore::open(&paths.database_path).unwrap();
  let queue = store.load_review_queue().unwrap();
  assert_eq!(queue["items"].as_array().unwrap().len(), 3);
  let items = queue["items"].as_array().unwrap();
  let node_ids: Vec<String> =
    items.iter().filter(|item| item["type"] == "node").map(|item| item["id"].as_str().unwrap().to_string()).collect();
  let edge_id = items.iter().find(|item| item["type"] == "edge").unwrap()["id"].as_str().unwrap().to_string();
  for proposal_id in node_ids {
    store.accept_graph_proposal(&proposal_id, None).unwrap();
  }
  store.accept_graph_proposal(&edge_id, None).unwrap();
  let graph = store.load_graph_snapshot().unwrap();
  assert_eq!(graph["nodes"].as_array().unwrap().len(), 2);
  assert_eq!(graph["edges"].as_array().unwrap().len(), 1);
  assert!(graph["nodes"].as_array().unwrap().iter().all(|node| node["type"] == "concept"));
  assert_eq!(graph["edges"][0]["type"], "supports");
  let _ = fs::remove_dir_all(root);
}

fn first_uuid_after(value: &str, marker: &str) -> String {
  let search = value.find(marker).map(|index| &value[index..]).unwrap_or(value);
  search
    .split(|ch: char| !(ch.is_ascii_hexdigit() || ch == '-'))
    .find(|part| {
      part.len() == 36
        && part.chars().filter(|ch| *ch == '-').count() == 4
        && part.chars().all(|ch| ch.is_ascii_hexdigit() || ch == '-')
    })
    .unwrap_or("")
    .to_string()
}

fn read_http_request(stream: &mut TcpStream) -> String {
  let mut bytes = Vec::new();
  let mut buffer = [0_u8; 4096];
  loop {
    let read = stream.read(&mut buffer).unwrap_or(0);
    if read == 0 {
      break;
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
      break;
    }
  }
  String::from_utf8_lossy(&bytes).to_string()
}

fn find_header_end(bytes: &[u8]) -> Option<usize> {
  bytes.windows(4).position(|window| window == b"\r\n\r\n")
}

struct RejectCredentialResolver;

impl soma_ai_runtime::CredentialResolver for RejectCredentialResolver {
  fn resolve(
    &self,
    credential: &soma_ai_runtime::CredentialRef,
  ) -> Result<Option<String>, soma_ai_runtime::AiRuntimeError> {
    panic!("tampered job runtime triggered credential lookup: {credential}")
  }
}

#[cfg(target_os = "windows")]
fn create_directory_link(target: &Path, link: &Path) -> std::io::Result<()> {
  let status = std::process::Command::new("cmd.exe")
    .args(["/C", "mklink", "/J"])
    .arg(link)
    .arg(target)
    .stdout(std::process::Stdio::null())
    .stderr(std::process::Stdio::null())
    .status()?;
  status.success().then_some(()).ok_or_else(|| std::io::Error::other("failed to create test directory junction"))
}

#[cfg(not(target_os = "windows"))]
fn create_directory_link(target: &Path, link: &Path) -> std::io::Result<()> {
  std::os::unix::fs::symlink(target, link)
}

#[cfg(target_os = "windows")]
fn remove_directory_link(link: &Path) -> std::io::Result<()> {
  fs::remove_dir(link)
}

#[cfg(not(target_os = "windows"))]
fn remove_directory_link(link: &Path) -> std::io::Result<()> {
  fs::remove_file(link)
}

fn assert_runtime_file(job: &Value, expected: &Value) {
  let job_dir = PathBuf::from(job["jobDir"].as_str().unwrap());
  let runtime_path = job_dir.join("runtime.json");
  let runtime: Value = serde_json::from_str(&fs::read_to_string(&runtime_path).unwrap()).unwrap();
  let metadata: Value = serde_json::from_str(&fs::read_to_string(job_dir.join("metadata.json")).unwrap()).unwrap();

  assert_eq!(job["files"]["runtime"].as_str(), Some(runtime_path.to_string_lossy().as_ref()));
  assert_eq!(runtime["providerId"], expected["providerId"]);
  assert_eq!(runtime["model"], expected["model"]);
  assert_eq!(metadata["runtime"]["providerId"], expected["providerId"]);
  assert!(runtime.get("apiKey").is_none());
  assert!(!fs::read_to_string(runtime_path).unwrap().contains("secret"));
}

fn create_runtime_test_job(runtime: Value) -> (PathBuf, WorkspacePaths, Value) {
  let root = std::env::temp_dir().join(format!("soma-runtime-job-test-{}", new_id()));
  let paths = create_workspace_dir(&root).unwrap();
  let source = root.join("source.md");
  fs::write(&source, "User: Runtime adapters should preserve review.\n\nAssistant: Output patches remain proposed.")
    .unwrap();
  import_source_file(&paths, &source).unwrap();
  let job = create_graph_extraction_job_with_runtime(&paths, &runtime).unwrap();
  (root, paths, job)
}

fn has_patch_error(result: &Value, path_suffix: &str, message_fragment: &str) -> bool {
  result["errors"].as_array().is_some_and(|errors| {
    errors.iter().any(|error| {
      error["path"].as_str().is_some_and(|path| path.ends_with(path_suffix))
        && error["message"].as_str().is_some_and(|message| message.contains(message_fragment))
    })
  })
}

fn write_runtime_script(path: &Path, patch: &Value) {
  let patch_json = serde_json::to_string(patch).unwrap();
  if cfg!(windows) {
    fs::write(path, format!("@echo off\r\necho {}\r\n", patch_json)).unwrap();
  } else {
    fs::write(path, format!("#!/bin/sh\necho '{}'\n", patch_json)).unwrap();
  }
}

fn restore_env_var(name: &str, value: Option<std::ffi::OsString>) {
  if let Some(value) = value {
    std::env::set_var(name, value);
  } else {
    std::env::remove_var(name);
  }
}
