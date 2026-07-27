use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::process::Command;

use rusqlite::OptionalExtension;
use serde_json::{json, Value};
use soma_ai_runtime::CredentialResolver;
use time::format_description::well_known::Rfc3339;
use time::OffsetDateTime;
use uuid::Uuid;

use crate::app_data_io::atomic_write;
use crate::contracts::{
  attach_source_message_id, empty_graph_patch, graph_patch_proposal_count, graph_patch_schema,
  normalize_graph_patch_for_review, validate_graph_patch_for_review, GRAPH_PATCH_SCHEMA_VERSION,
};
use crate::database::{open_existing_database, with_write_transaction};
use crate::error::{CommandError, CommandResult};
use crate::graph_read_model::active_graph_snapshot;
use crate::graph_write_model::{active_edge_ids, active_node_ids, persist_graph_patch_proposals, PersistPatchOptions};
use crate::job_files::{
  assert_safe_job_id, job_import_state, job_metadata, job_run_from_dir, known_job_chunk_ids, output_patch_state,
  read_output_patch, JobFiles,
};
use crate::runtime_adapters::run_compile_job_with_credentials as run_runtime_compile_job_with_credentials;
use crate::source_import::select_chunks_for_job;
use crate::workspace::{WorkspacePaths, JOB_DIR};

const DEFAULT_JOB_CHUNK_LIMIT: i64 = 500;
const JOB_STAGING_PREFIX: &str = ".soma-job-staging-";
const JOB_ARTIFACT_NAMES: &[&str] = &[
  "metadata.json",
  "instructions.md",
  "runtime.json",
  "runtime_result.json",
  "chunks.json",
  "message.json",
  "context_packet.json",
  "relevant_graph.json",
  "focused_node.json",
  "neighbors.json",
  "bridge_texts.json",
  "evidence.json",
  "current_graph_snapshot.json",
  "graph_patch.schema.json",
  "output_patch.json",
];

pub fn create_graph_extraction_job_with_runtime(paths: &WorkspacePaths, runtime: &Value) -> CommandResult<Value> {
  let conn = open_existing_database(&paths.database_path)?;
  let selection = select_chunks_for_job(&conn, DEFAULT_JOB_CHUNK_LIMIT)?;
  let chunks = selection.chunks;
  if chunks.is_empty() {
    return Err(CommandError::validation("Cannot create graph extraction job: workspace has no imported chunks."));
  }
  let included_chunk_count = chunks.len();
  let total_chunk_count = selection.total_count;
  let truncated = total_chunk_count > included_chunk_count as i64;
  let graph_snapshot = active_graph_snapshot(&conn)?;
  drop(conn);
  let created_at = now_string()?;
  let job_id = default_job_id(&created_at);
  assert_safe_job_id(&job_id)?;
  let source_count =
    chunks.iter().filter_map(|chunk| chunk.get("source_id").and_then(Value::as_str)).collect::<HashSet<_>>().len();
  let metadata = json!({
    "job_id": job_id,
    "created_at": created_at,
    "schema_version": GRAPH_PATCH_SCHEMA_VERSION,
    "chunk_count": included_chunk_count,
    "included_chunk_count": included_chunk_count,
    "total_chunk_count": total_chunk_count,
    "truncated": truncated,
    "source_count": source_count,
    "runtime": runtime
  });

  let jobs_dir = resolve_jobs_root(paths)?;
  let job_dir = publish_job_directory(&jobs_dir, &job_id, |staging_dir| {
    let files = JobFiles::new(staging_dir);
    write_json(&files.metadata, &metadata)?;
    write_json(&files.runtime, runtime)?;
    write_json(
      &files.chunks,
      &json!({
        "schema_version": GRAPH_PATCH_SCHEMA_VERSION,
        "generated_at": created_at,
        "chunks": chunks
      }),
    )?;
    write_json(&files.current_graph_snapshot, &graph_snapshot)?;
    write_json(&files.graph_patch_schema, &graph_patch_schema())?;
    write_json(&files.output_patch, &empty_graph_patch())?;
    fs::write(&files.instructions, job_instructions(&metadata))?;
    Ok(())
  })?;
  let files = JobFiles::new(&job_dir);

  Ok(json!({
    "jobId": job_id,
    "jobDir": job_dir.to_string_lossy(),
    "files": files.to_json(),
    "chunkCount": included_chunk_count,
    "includedChunkCount": included_chunk_count,
    "totalChunkCount": total_chunk_count,
    "truncated": truncated
  }))
}

pub fn list_jobs(paths: &WorkspacePaths) -> CommandResult<Value> {
  let jobs_dir = match resolve_jobs_root(paths) {
    Ok(jobs_dir) => jobs_dir,
    Err(error) if error.code == "Soma_NOT_FOUND" => {
      return Ok(json!({ "jobs": [] }));
    }
    Err(error) => return Err(error),
  };
  if !jobs_dir.is_dir() {
    return Ok(json!({ "jobs": [] }));
  }
  let conn = open_existing_database(&paths.database_path)?;
  let imports_by_job = job_import_state(&conn)?;

  let mut jobs = Vec::new();
  for entry in fs::read_dir(&jobs_dir)? {
    let Ok(entry) = entry else {
      continue;
    };
    let Ok(file_type) = entry.file_type() else {
      continue;
    };
    if !file_type.is_dir() {
      continue;
    }
    let job_id = entry.file_name().to_string_lossy().to_string();
    if job_id.starts_with(JOB_STAGING_PREFIX) {
      continue;
    }
    let Ok(job_dir) = resolve_existing_job_dir(paths, &job_id) else {
      continue;
    };
    if validate_known_job_artifacts(&job_dir).is_err() || !JobFiles::new(&job_dir).is_complete() {
      continue;
    }
    if let Ok(job) = job_run_from_dir(&job_dir, &job_id, imports_by_job.get(&job_id).copied()) {
      jobs.push(job);
    }
  }
  jobs.sort_by(|a, b| {
    let created_a = a.get("createdAt").and_then(Value::as_str).unwrap_or("");
    let created_b = b.get("createdAt").and_then(Value::as_str).unwrap_or("");
    created_b.cmp(created_a).then_with(|| b["jobId"].as_str().unwrap_or("").cmp(a["jobId"].as_str().unwrap_or("")))
  });

  Ok(json!({ "jobs": jobs }))
}

pub fn clear_job_history(paths: &WorkspacePaths) -> CommandResult<Value> {
  let root = match resolve_jobs_root(paths) {
    Ok(root) => root,
    Err(error) if error.code == "Soma_NOT_FOUND" => return Ok(json!({ "removed": 0 })),
    Err(error) => return Err(error),
  };
  let mut removed = 0;
  for entry in fs::read_dir(&root)? {
    let entry = entry?;
    let metadata = fs::symlink_metadata(entry.path())?;
    if is_link_or_reparse(&metadata) {
      return Err(CommandError::validation("Refusing to delete a linked job history entry."));
    }
    if !metadata.is_dir() {
      continue;
    }

    let target = entry.path().canonicalize()?;
    if target.parent() != Some(root.as_path()) {
      return Err(CommandError::validation("Refusing to delete a path outside the job history directory."));
    }

    fs::remove_dir_all(target)?;
    removed += 1;
  }

  Ok(json!({ "removed": removed }))
}

#[cfg(test)]
pub fn get_job(paths: &WorkspacePaths, job_id: &str) -> CommandResult<Value> {
  let job_dir = resolve_existing_job_dir(paths, job_id)?;
  validate_known_job_artifacts(&job_dir)?;
  let conn = open_existing_database(&paths.database_path)?;
  let imports_by_job = job_import_state(&conn)?;
  Ok(json!({ "job": job_run_from_dir(&job_dir, job_id, imports_by_job.get(job_id).copied())? }))
}

pub fn open_job_folder(paths: &WorkspacePaths, job_id: &str) -> CommandResult<Value> {
  let job_dir = resolve_existing_job_dir(paths, job_id)?;
  open_folder(&job_dir)?;
  Ok(json!({
    "jobId": job_id,
    "jobDir": job_dir.to_string_lossy(),
    "opened": true
  }))
}

pub fn run_compile_job_with_credentials(
  paths: &WorkspacePaths,
  job_id: &str,
  runtime: &Value,
  credentials: &dyn CredentialResolver,
) -> CommandResult<Value> {
  let job_dir = resolve_existing_job_dir(paths, job_id)?;
  validate_known_job_artifacts(&job_dir)?;
  for file_name in
    ["instructions.md", "runtime.json", "chunks.json", "current_graph_snapshot.json", "graph_patch.schema.json"]
  {
    resolve_required_job_artifact(&job_dir, file_name)?;
  }
  let output_path = job_dir.join("output_patch.json");
  resolve_optional_job_artifact(&job_dir, "output_patch.json")?;
  resolve_optional_job_artifact(&job_dir, "runtime_result.json")?;
  write_json_atomic(&output_path, &empty_graph_patch())?;
  write_json_atomic(&job_dir.join("runtime.json"), runtime)?;
  let run = match run_runtime_compile_job_with_credentials(&job_dir, runtime, credentials) {
    Ok(run) => run,
    Err(error) => crate::runtime_adapters::RuntimeRunResult {
      adapter_kind: runtime_adapter_kind(runtime),
      status: "failed",
      failure_kind: Some(error.runtime_failure_kind()),
      message: error.message,
      wrote_output_patch: false,
    },
  };
  let output_path = resolve_required_job_artifact(&job_dir, "output_patch.json")?;
  let output_patch = output_patch_state(&output_path);
  let output_patch_importable = run.wrote_output_patch && output_patch.importable;
  let ran_at = now_string()?;
  let result = json!({
    "jobId": job_id,
    "jobDir": job_dir.to_string_lossy(),
    "adapterKind": run.adapter_kind,
    "status": run.status,
    "failureKind": run.failure_kind,
    "message": run.message,
    "wroteOutputPatch": run.wrote_output_patch,
    "outputPatchStatus": output_patch.status,
    "outputPatchProposalCount": output_patch.proposal_count,
    "outputPatchImportable": output_patch_importable,
    "ranAt": ran_at
  });
  resolve_optional_job_artifact(&job_dir, "runtime_result.json")?;
  write_json_atomic(&job_dir.join("runtime_result.json"), &result)?;
  Ok(result)
}

pub fn compile_graph_workspace_with_runtime_and_credentials(
  paths: &WorkspacePaths,
  runtime: &Value,
  credentials: &dyn CredentialResolver,
) -> CommandResult<Value> {
  let created_job = create_graph_extraction_job_with_runtime(paths, runtime)?;
  let job_id = created_job
    .get("jobId")
    .and_then(Value::as_str)
    .ok_or_else(|| CommandError::storage("Created compile job has no jobId."))?
    .to_string();
  let run = run_compile_job_with_credentials(paths, &job_id, runtime, credentials)?;
  let output_importable = run.get("outputPatchImportable").and_then(Value::as_bool).unwrap_or(false);

  let import_result = if output_importable {
    import_graph_patch_for_review(paths, &job_id)?
  } else {
    let run_message =
      run.get("message").and_then(Value::as_str).unwrap_or("Compile Graph did not produce reviewable updates.");
    json!({
      "jobId": job_id,
      "valid": false,
      "imported": false,
      "trusted": false,
      "proposalCount": 0,
      "proposals": [],
      "errors": [{
        "path": "$",
        "message": run_message
      }],
      "warnings": []
    })
  };

  let imported = import_result.get("imported").and_then(Value::as_bool).unwrap_or(false);
  let proposal_count = import_result.get("proposalCount").and_then(Value::as_i64).unwrap_or(0);
  let status = if imported && proposal_count > 0 { "review_ready" } else { "failed" };
  let message = if status == "review_ready" {
    format!("Review Updates has {proposal_count} new proposals.")
  } else {
    compile_graph_workspace_failure_message(&run, &import_result)
  };
  let job_dir = resolve_existing_job_dir(paths, &job_id)?;
  validate_known_job_artifacts(&job_dir)?;
  let conn = open_existing_database(&paths.database_path)?;
  let imports_by_job = job_import_state(&conn)?;
  let job = job_run_from_dir(&job_dir, &job_id, imports_by_job.get(&job_id).copied())?;

  Ok(json!({
    "status": status,
    "message": message,
    "job": job,
    "createdJob": created_job,
    "run": run,
    "importResult": import_result,
    "proposalCount": proposal_count
  }))
}

pub fn import_graph_patch_for_review(paths: &WorkspacePaths, job_id: &str) -> CommandResult<Value> {
  let job_dir = resolve_existing_job_dir(paths, job_id)?;
  validate_known_job_artifacts(&job_dir)?;
  let Some(output_path) = resolve_optional_job_artifact(&job_dir, "output_patch.json")? else {
    let output_path = job_dir.join("output_patch.json");
    return Ok(invalid_patch_result(job_id, &job_dir, &output_path, "$", "output_patch.json does not exist."));
  };
  let raw_output = match read_output_patch(&output_path) {
    Ok(raw_output) => raw_output,
    Err(error) => {
      return Ok(invalid_patch_result(job_id, &job_dir, &output_path, "$", &error.message));
    }
  };
  let raw_patch: Value = match serde_json::from_str(&raw_output) {
    Ok(value) => value,
    Err(error) => {
      return Ok(invalid_patch_result(
        job_id,
        &job_dir,
        &output_path,
        "$",
        &format!("output_patch.json is invalid JSON: {error}"),
      ));
    }
  };
  let metadata = job_metadata(&job_dir)?;
  let source_message_id = metadata.get("source_message_id").and_then(Value::as_str);
  let (patch, repair_warnings) = normalize_graph_patch_for_review(&raw_patch);
  let patch = attach_source_message_id(patch, source_message_id);
  let snapshot_path = match resolve_optional_job_artifact(&job_dir, "current_graph_snapshot.json")? {
    Some(path) => path,
    None => {
      return Ok(invalid_patch_result(
        job_id,
        &job_dir,
        &output_path,
        "$",
        "current_graph_snapshot.json is unavailable.",
      ));
    }
  };
  let snapshot: Value = match fs::read_to_string(&snapshot_path)
    .map_err(CommandError::from)
    .and_then(|raw| serde_json::from_str(&raw).map_err(|error| CommandError::storage(error.to_string())))
  {
    Ok(snapshot) => snapshot,
    Err(error) => {
      return Ok(invalid_patch_result(
        job_id,
        &job_dir,
        &output_path,
        "$",
        &format!("current_graph_snapshot.json is unavailable or invalid: {}", error.message),
      ));
    }
  };
  let (patch, snapshot_errors) = attach_job_snapshot_preconditions(patch, &snapshot);

  let conn = open_existing_database(&paths.database_path)?;
  let validation = validate_graph_patch_for_review(&patch, &active_node_ids(&conn)?, &active_edge_ids(&conn)?);
  let mut errors = validation.errors;
  errors.extend(snapshot_errors);
  let mut warnings = repair_warnings;
  warnings.extend(validation.warnings);
  validate_known_chunk_refs(&patch, &known_job_chunk_ids(&job_dir)?, "$", &mut errors);
  if !errors.is_empty() {
    return Ok(json!({
      "jobId": job_id,
      "valid": false,
      "imported": false,
      "trusted": false,
      "patch": null,
      "errors": errors,
      "warnings": warnings
    }));
  }
  let proposal_count = graph_patch_proposal_count(&patch);
  if proposal_count == 0 {
    return Ok(json!({
      "jobId": job_id,
      "valid": true,
      "imported": false,
      "trusted": false,
      "proposalCount": 0,
      "proposals": [],
      "patch": null,
      "errors": [{
        "path": "$",
        "message": "output_patch.json contains no proposed graph updates yet."
      }],
      "warnings": warnings
    }));
  }

  let source = match metadata.get("job_kind").and_then(Value::as_str) {
    Some("node_chat_update") => "node_chat_update_job",
    _ if source_message_id.is_some() => "graph_thread_message",
    _ => "job_output_patch",
  };
  with_write_transaction(&conn, |conn| {
    let existing = conn
      .query_row(
        concat!(
          "SELECT graph_patches.id, COUNT(graph_proposals.id) ",
          "FROM graph_patches ",
          "LEFT JOIN graph_proposals ON graph_proposals.patch_id = graph_patches.id ",
          "WHERE graph_patches.job_id = ?1 ",
          "GROUP BY graph_patches.id ",
          "ORDER BY graph_patches.created_at ASC ",
          "LIMIT 1"
        ),
        [job_id],
        |row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?)),
      )
      .optional()?;
    if let Some((patch_id, existing_proposal_count)) = existing {
      return Ok(json!({
        "jobId": job_id,
        "patchId": patch_id,
        "valid": true,
        "imported": false,
        "alreadyImported": true,
        "trusted": false,
        "proposalCount": existing_proposal_count,
        "proposals": [],
        "errors": [{
          "path": "$",
          "message": "This job output was already imported into Review Updates."
        }],
        "warnings": warnings
      }));
    }
    let persisted = persist_graph_patch_proposals(
      conn,
      &patch,
      PersistPatchOptions { source, source_message_id, job_id: Some(job_id), proposal_status: "proposed" },
    )?;
    Ok(json!({
      "jobId": job_id,
      "patchId": persisted.patch_id,
      "valid": true,
      "imported": true,
      "trusted": false,
      "proposalCount": persisted.proposals.len(),
      "proposals": persisted.proposals,
      "errors": [],
      "warnings": warnings
    }))
  })
}

fn attach_job_snapshot_preconditions(mut patch: Value, snapshot: &Value) -> (Value, Vec<Value>) {
  let node_versions = snapshot
    .get("nodes")
    .and_then(Value::as_array)
    .into_iter()
    .flatten()
    .filter_map(|node| {
      Some((node.get("id")?.as_str()?.to_string(), node.get("body_version_id")?.as_str()?.to_string()))
    })
    .collect::<HashMap<_, _>>();
  let edge_revisions = snapshot
    .get("edges")
    .and_then(Value::as_array)
    .into_iter()
    .flatten()
    .filter_map(|edge| Some((edge.get("id")?.as_str()?.to_string(), edge.get("updated_at")?.as_str()?.to_string())))
    .collect::<HashMap<_, _>>();
  let mut errors = Vec::new();

  if let Some(updates) = patch.get_mut("proposed_node_body_updates").and_then(Value::as_array_mut) {
    for (index, update) in updates.iter_mut().enumerate() {
      let Some(update) = update.as_object_mut() else {
        continue;
      };
      update.remove("base_body_version_id");
      let target_id =
        update.get("target_node_id").or_else(|| update.get("node_id")).and_then(Value::as_str).map(str::to_string);
      let Some(target_id) = target_id else {
        continue;
      };
      match node_versions.get(&target_id) {
        Some(version_id) => {
          update.insert("base_body_version_id".to_string(), json!(version_id));
        }
        None => errors.push(json!({
          "path": format!("$.proposed_node_body_updates[{index}].target_node_id"),
          "message": format!(
            "Node body update target was not present in the job snapshot: {target_id}. Regenerate the job."
          )
        })),
      }
    }
  }

  if let Some(updates) = patch.get_mut("proposed_edge_bridge_updates").and_then(Value::as_array_mut) {
    for (index, update) in updates.iter_mut().enumerate() {
      let Some(update) = update.as_object_mut() else {
        continue;
      };
      update.remove("base_edge_updated_at");
      let target_id =
        update.get("target_edge_id").or_else(|| update.get("edge_id")).and_then(Value::as_str).map(str::to_string);
      let Some(target_id) = target_id else {
        continue;
      };
      match edge_revisions.get(&target_id) {
        Some(updated_at) => {
          update.insert("base_edge_updated_at".to_string(), json!(updated_at));
        }
        None => errors.push(json!({
          "path": format!("$.proposed_edge_bridge_updates[{index}].target_edge_id"),
          "message": format!(
            "Edge bridge update target was not present in the job snapshot: {target_id}. Regenerate the job."
          )
        })),
      }
    }
  }

  (patch, errors)
}

fn compile_graph_workspace_failure_message(run: &Value, import_result: &Value) -> String {
  let import_errors = import_result
    .get("errors")
    .and_then(Value::as_array)
    .map(|errors| {
      errors
        .iter()
        .filter_map(|error| {
          error
            .get("message")
            .and_then(Value::as_str)
            .map(str::to_string)
            .or_else(|| error.as_str().map(str::to_string))
        })
        .collect::<Vec<_>>()
        .join("; ")
    })
    .unwrap_or_default();
  if !import_errors.trim().is_empty() {
    return import_errors;
  }
  run.get("message").and_then(Value::as_str).unwrap_or("Compile Graph did not produce reviewable updates.").to_string()
}

fn runtime_adapter_kind(runtime: &Value) -> String {
  runtime
    .get("adapter")
    .and_then(|adapter| adapter.get("kind"))
    .and_then(Value::as_str)
    .unwrap_or("unknown")
    .to_string()
}

fn validate_known_chunk_refs(value: &Value, known_chunk_ids: &HashSet<String>, path: &str, errors: &mut Vec<Value>) {
  match value {
    Value::Array(items) => {
      for (index, item) in items.iter().enumerate() {
        validate_known_chunk_refs(item, known_chunk_ids, &format!("{path}[{index}]"), errors);
      }
    }
    Value::Object(map) => {
      if let Some(ids) = map.get("source_chunk_ids").and_then(Value::as_array) {
        for (index, id) in ids.iter().enumerate() {
          match id.as_str() {
            Some(id) if known_chunk_ids.contains(id) => {}
            Some(id) => errors.push(json!({
              "path": format!("{path}.source_chunk_ids[{index}]"),
              "message": format!("Unknown chunk id: {id}")
            })),
            None => errors.push(json!({
              "path": format!("{path}.source_chunk_ids[{index}]"),
              "message": "Chunk id must be a non-empty string."
            })),
          }
        }
      }
      for (key, child) in map {
        if key != "source_chunk_ids" {
          validate_known_chunk_refs(child, known_chunk_ids, &format!("{path}.{key}"), errors);
        }
      }
    }
    _ => {}
  }
}

fn invalid_patch_result(job_id: &str, job_dir: &Path, output_path: &Path, issue_path: &str, message: &str) -> Value {
  json!({
    "jobId": job_id,
    "jobDir": job_dir.to_string_lossy(),
    "outputPath": output_path.to_string_lossy(),
    "valid": false,
    "trusted": false,
    "patch": null,
    "errors": [{ "path": issue_path, "message": message }],
    "warnings": []
  })
}

fn resolve_jobs_root(paths: &WorkspacePaths) -> CommandResult<PathBuf> {
  let workspace_root = paths.workspace_dir.canonicalize()?;
  let jobs_path = paths.workspace_dir.join(JOB_DIR);
  let metadata = match fs::symlink_metadata(&jobs_path) {
    Ok(metadata) => metadata,
    Err(error) if error.kind() == ErrorKind::NotFound => {
      return Err(CommandError::not_found("Job history directory not found."));
    }
    Err(error) => return Err(error.into()),
  };
  if is_link_or_reparse(&metadata) || !metadata.is_dir() {
    return Err(CommandError::validation("Job history must be a regular directory inside the workspace."));
  }

  let jobs_root = jobs_path.canonicalize()?;
  if jobs_root.parent() != Some(workspace_root.as_path()) {
    return Err(CommandError::validation("Job history resolves outside the workspace."));
  }
  Ok(jobs_root)
}

fn resolve_existing_job_dir(paths: &WorkspacePaths, job_id: &str) -> CommandResult<PathBuf> {
  assert_safe_job_id(job_id)?;
  let jobs_root = resolve_jobs_root(paths)?;
  let candidate = jobs_root.join(job_id);
  let metadata = match fs::symlink_metadata(&candidate) {
    Ok(metadata) => metadata,
    Err(error) if error.kind() == ErrorKind::NotFound => {
      return Err(CommandError::not_found(format!("Job not found: {job_id}")));
    }
    Err(error) => return Err(error.into()),
  };
  if is_link_or_reparse(&metadata) || !metadata.is_dir() {
    return Err(CommandError::validation(format!("Job is not a regular directory: {job_id}")));
  }

  let job_dir = candidate.canonicalize()?;
  if job_dir.parent() != Some(jobs_root.as_path()) {
    return Err(CommandError::validation(format!("Job resolves outside the job history directory: {job_id}")));
  }
  Ok(job_dir)
}

fn resolve_optional_job_artifact(job_dir: &Path, file_name: &str) -> CommandResult<Option<PathBuf>> {
  let path = job_dir.join(file_name);
  let metadata = match fs::symlink_metadata(&path) {
    Ok(metadata) => metadata,
    Err(error) if error.kind() == ErrorKind::NotFound => return Ok(None),
    Err(error) => return Err(error.into()),
  };
  if is_link_or_reparse(&metadata) || !metadata.is_file() {
    return Err(CommandError::validation(format!("{file_name} must be a regular file inside its job directory.")));
  }

  let artifact = path.canonicalize()?;
  if artifact.parent() != Some(job_dir) {
    return Err(CommandError::validation(format!("{file_name} resolves outside its job directory.")));
  }
  Ok(Some(artifact))
}

fn resolve_required_job_artifact(job_dir: &Path, file_name: &str) -> CommandResult<PathBuf> {
  resolve_optional_job_artifact(job_dir, file_name)?
    .ok_or_else(|| CommandError::validation(format!("Job artifact is missing: {file_name}")))
}

fn validate_known_job_artifacts(job_dir: &Path) -> CommandResult<()> {
  for file_name in JOB_ARTIFACT_NAMES {
    resolve_optional_job_artifact(job_dir, file_name)?;
  }
  Ok(())
}

fn is_link_or_reparse(metadata: &fs::Metadata) -> bool {
  if metadata.file_type().is_symlink() {
    return true;
  }
  #[cfg(target_os = "windows")]
  {
    use std::os::windows::fs::MetadataExt;
    const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
    metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
  }
  #[cfg(not(target_os = "windows"))]
  {
    false
  }
}

fn publish_job_directory(
  jobs_dir: &Path,
  job_id: &str,
  write_job: impl FnOnce(&Path) -> CommandResult<()>,
) -> CommandResult<PathBuf> {
  assert_safe_job_id(job_id)?;
  fs::create_dir_all(jobs_dir)?;
  let job_dir = jobs_dir.join(job_id);
  if job_dir.exists() {
    return Err(CommandError::validation(format!("Job already exists: {job_id}")));
  }

  let staging_dir = jobs_dir.join(format!("{JOB_STAGING_PREFIX}{job_id}-{}", new_id()));
  fs::create_dir(&staging_dir)?;
  let result = write_job(&staging_dir).and_then(|()| {
    if job_dir.exists() {
      return Err(CommandError::validation(format!("Job already exists: {job_id}")));
    }
    fs::rename(&staging_dir, &job_dir)?;
    Ok(job_dir)
  });
  if result.is_err() && staging_dir.exists() {
    let _ = fs::remove_dir_all(&staging_dir);
  }
  result
}

fn default_job_id(created_at: &str) -> String {
  let digits = created_at.chars().filter(|ch| ch.is_ascii_digit()).take(14).collect::<String>();
  format!("job_{}_{}", digits, &new_id()[..8])
}

fn write_json(path: &Path, value: &Value) -> CommandResult<()> {
  fs::write(path, json_bytes(value)?)?;
  Ok(())
}

fn write_json_atomic(path: &Path, value: &Value) -> CommandResult<()> {
  atomic_write(path, &json_bytes(value)?)
}

fn json_bytes(value: &Value) -> CommandResult<Vec<u8>> {
  let mut contents = serde_json::to_vec_pretty(value).map_err(|error| CommandError::storage(error.to_string()))?;
  contents.push(b'\n');
  Ok(contents)
}

fn open_folder(path: &Path) -> CommandResult<()> {
  #[cfg(target_os = "windows")]
  let mut command = {
    let mut command = Command::new("explorer.exe");
    command.arg(path);
    command
  };

  #[cfg(target_os = "macos")]
  let mut command = {
    let mut command = Command::new("open");
    command.arg(path);
    command
  };

  #[cfg(all(not(target_os = "windows"), not(target_os = "macos")))]
  let mut command = {
    let mut command = Command::new("xdg-open");
    command.arg(path);
    command
  };

  command.spawn().map(|_| ()).map_err(Into::into)
}

fn job_instructions(metadata: &Value) -> String {
  format!(
    concat!(
      "# Soma Graph Extraction Job\n\n",
      "Job: {}\n\n",
      "## Files\n\n",
      "- Read `chunks.json` for source chunks and provenance IDs.\n",
      "- Read `current_graph_snapshot.json` for existing trusted graph state.\n",
      "- Read `runtime.json` for the selected runtime/model/profile. ",
      "It is redacted and contains no credentials.\n",
      "- Use `graph_patch.schema.json` as the output contract.\n",
      "- Write only `output_patch.json`.\n\n",
      "## Task\n\n",
      "Compile useful conversation sections into proposed graph changes. ",
      "Treat every result as proposed and untrusted; the app will review and validate it later.\n\n",
      "## Rules\n\n",
      "- Create fewer, stronger nodes. ",
      "A proposed node is a compiled conversation section, not a topic tag.\n",
      "- Target 300-1500 words for each new node `compiled_body` when the source material allows it. ",
      "Use 2-6 coherent paragraphs.\n",
      "- Use short graph labels: `title` should usually be 2-6 words, ",
      "and `preview` should be one compact sentence.\n",
      "- Synthesize and organize the important reasoning; ",
      "do not dump raw transcript or create one node per message.\n",
      "- Avoid duplicate or overlapping nodes. ",
      "Use merge candidates or node body updates when an existing section should absorb the material.\n",
      "- Give every proposed node a stable `temp_id`; ",
      "edges can reference it with `source_temp_id` and `target_temp_id`.\n",
      "- Every proposed node must include `type` with one of: ",
      "project, concept, claim, decision, question, task, artifact, source_conversation, tool.\n",
      "- Every proposed edge must include `type` with one of: ",
      "part_of, supports, contradicts, depends_on, answers, implements, mentions, derived_from, ",
      "alternative_to, blocks, next_step, mitigates.\n",
      "- Every proposed edge must include `reason`. ",
      "Do not invent edge types such as clarifies, precedes, would_next_test, or related_to.\n",
      "- Edge `bridge_text` is optional. ",
      "When useful, keep it to one specific sentence, usually 5-25 words.\n",
      "- Reference source chunks with `source_chunk_ids` for proposed nodes, edges, body updates, ",
      "and bridge updates.\n",
      "- Do not invent optimistic preconditions. ",
      "The app stamps body and edge update proposals from `current_graph_snapshot.json` during import.\n",
      "- Never write credentials, tokens, or secrets into `output_patch.json`.\n",
      "- Do not create accepted graph state, mutate raw chats, or remove source material.\n"
    ),
    metadata["job_id"].as_str().unwrap_or("job")
  )
}

fn now_string() -> CommandResult<String> {
  Ok(OffsetDateTime::now_utc().format(&Rfc3339)?)
}

fn new_id() -> String {
  Uuid::new_v4().to_string()
}

#[cfg(test)]
#[path = "jobs_tests.rs"]
mod tests;
