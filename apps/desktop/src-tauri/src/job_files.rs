use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

use serde_json::{json, Value};

use crate::contracts::graph_patch_proposal_count;
use crate::error::{CommandError, CommandResult};

pub(crate) const OUTPUT_PATCH_MAX_BYTES: u64 = 4 * 1024 * 1024;

#[derive(Clone, Copy, Default)]
pub(crate) struct JobImportState {
  pub(crate) imported_proposal_count: i64,
  pub(crate) accepted_proposal_count: i64,
}

pub(crate) struct OutputPatchState {
  pub(crate) exists: bool,
  pub(crate) status: &'static str,
  pub(crate) proposal_count: usize,
  pub(crate) importable: bool,
}

pub(crate) fn assert_safe_job_id(job_id: &str) -> CommandResult<()> {
  if !job_id.is_empty()
    && !matches!(job_id, "." | "..")
    && job_id.chars().all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '.' | '_' | '-'))
  {
    Ok(())
  } else {
    Err(CommandError::validation(
      "Job id must be a directory name containing only letters, numbers, dots, underscores, and dashes.",
    ))
  }
}

pub(crate) fn job_run_from_dir(
  job_dir: &Path,
  job_id: &str,
  import_state: Option<JobImportState>,
) -> CommandResult<Value> {
  assert_safe_job_id(job_id)?;
  let files = job_run_files(job_dir);
  let metadata_exists = job_dir.join("metadata.json").exists();
  let metadata = job_metadata(job_dir)?;
  let job_kind = metadata.get("job_kind").and_then(Value::as_str).unwrap_or("graph_extraction");
  let chunk_count = metadata.get("chunk_count").and_then(Value::as_i64).unwrap_or(0);
  let included_chunk_count = metadata.get("included_chunk_count").and_then(Value::as_i64).unwrap_or(chunk_count);
  let total_chunk_count = metadata.get("total_chunk_count").and_then(Value::as_i64).unwrap_or(included_chunk_count);
  let truncated =
    metadata.get("truncated").and_then(Value::as_bool).unwrap_or(total_chunk_count > included_chunk_count);
  let source_count = metadata.get("source_count").and_then(Value::as_i64).unwrap_or(0);
  let schema_version = metadata.get("schema_version").and_then(Value::as_i64).map(Value::from).unwrap_or(Value::Null);
  let output_patch = output_patch_state(&job_dir.join("output_patch.json"));
  let runtime_result = runtime_result(job_dir);
  let imported_proposal_count = import_state.map(|state| state.imported_proposal_count).unwrap_or(0);
  let accepted_proposal_count = import_state.map(|state| state.accepted_proposal_count).unwrap_or(0);
  let latest_run_wrote_output = runtime_result.get("wroteOutputPatch").and_then(Value::as_bool).unwrap_or(true);
  let output_patch_importable = output_patch.importable && imported_proposal_count == 0 && latest_run_wrote_output;

  Ok(json!({
    "jobId": job_id,
    "jobDir": job_dir.to_string_lossy(),
    "jobKind": job_kind,
    "createdAt": metadata.get("created_at").and_then(Value::as_str),
    "schemaVersion": schema_version,
    "chunkCount": chunk_count,
    "includedChunkCount": included_chunk_count,
    "totalChunkCount": total_chunk_count,
    "truncated": truncated,
    "sourceCount": source_count,
    "sourceMessageId": metadata.get("source_message_id").and_then(Value::as_str),
    "sourceNodeId": metadata.get("source_node_id").and_then(Value::as_str),
    "files": files,
    "metadataExists": metadata_exists,
    "outputPatchExists": output_patch.exists,
    "outputPatchStatus": output_patch.status,
    "outputPatchProposalCount": output_patch.proposal_count,
    "outputPatchImportable": output_patch_importable,
    "importedProposalCount": imported_proposal_count,
    "acceptedProposalCount": accepted_proposal_count,
    "runtimeStatus": runtime_result.get("status").and_then(Value::as_str),
    "runtimeFailureKind": runtime_result.get("failureKind").and_then(Value::as_str),
    "runtimeMessage": runtime_result.get("message").and_then(Value::as_str),
    "runtimeAdapterKind": runtime_result.get("adapterKind").and_then(Value::as_str),
    "runtimeRanAt": runtime_result.get("ranAt").and_then(Value::as_str)
  }))
}

pub(crate) fn job_import_state(conn: &rusqlite::Connection) -> CommandResult<HashMap<String, JobImportState>> {
  let mut stmt = conn.prepare(
    "
        SELECT
          graph_patches.job_id,
          COUNT(CASE WHEN graph_proposals.proposal_type != 'warning' THEN 1 END) AS imported_count,
          COUNT(
            CASE
              WHEN graph_proposals.proposal_type != 'warning'
                AND graph_proposals.status = 'accepted'
              THEN 1
            END
          ) AS accepted_count
        FROM graph_patches
        LEFT JOIN graph_proposals ON graph_proposals.patch_id = graph_patches.id
        WHERE graph_patches.job_id IS NOT NULL
        GROUP BY graph_patches.job_id
        ",
  )?;
  let rows = stmt.query_map([], |row| {
    Ok((
      row.get::<_, String>(0)?,
      JobImportState { imported_proposal_count: row.get::<_, i64>(1)?, accepted_proposal_count: row.get::<_, i64>(2)? },
    ))
  })?;

  let mut state = HashMap::new();
  for row in rows {
    let (job_id, import_state) = row?;
    state.insert(job_id, import_state);
  }
  Ok(state)
}

pub(crate) fn output_patch_state(output_path: &Path) -> OutputPatchState {
  if !output_path.exists() {
    return OutputPatchState { exists: false, status: "missing", proposal_count: 0, importable: false };
  }

  let patch: Value = match read_output_patch(output_path).ok().and_then(|content| serde_json::from_str(&content).ok()) {
    Some(value) => value,
    None => {
      return OutputPatchState { exists: true, status: "invalid", proposal_count: 0, importable: false };
    }
  };

  let proposal_count = graph_patch_proposal_count(&patch);
  OutputPatchState {
    exists: true,
    status: if proposal_count > 0 { "ready" } else { "empty" },
    proposal_count,
    importable: proposal_count > 0,
  }
}

pub(crate) fn read_output_patch(output_path: &Path) -> CommandResult<String> {
  let mut reader = fs::File::open(output_path)?.take(OUTPUT_PATCH_MAX_BYTES + 1);
  let mut raw = Vec::new();
  reader.read_to_end(&mut raw)?;
  if raw.len() as u64 > OUTPUT_PATCH_MAX_BYTES {
    return Err(CommandError::validation(format!(
      "output_patch.json exceeds the {OUTPUT_PATCH_MAX_BYTES}-byte safety limit."
    )));
  }
  String::from_utf8(raw)
    .map_err(|error| CommandError::validation(format!("output_patch.json is not valid UTF-8: {error}")))
}

pub(crate) fn job_metadata(job_dir: &Path) -> CommandResult<Value> {
  let metadata_path = job_dir.join("metadata.json");
  if !metadata_path.exists() {
    return Ok(json!({}));
  }
  serde_json::from_str(&fs::read_to_string(metadata_path)?)
    .map_err(|error| CommandError::validation(format!("metadata.json is invalid JSON: {error}")))
}

pub(crate) fn known_job_chunk_ids(job_dir: &Path) -> CommandResult<HashSet<String>> {
  let chunks_path = job_dir.join("chunks.json");
  if !chunks_path.exists() {
    return Ok(HashSet::new());
  }
  let chunks: Value = serde_json::from_str(&fs::read_to_string(chunks_path)?)
    .map_err(|error| CommandError::validation(format!("chunks.json is invalid JSON: {error}")))?;
  Ok(
    chunks
      .get("chunks")
      .and_then(Value::as_array)
      .into_iter()
      .flatten()
      .filter_map(|chunk| chunk.get("chunk_id").and_then(Value::as_str).map(String::from))
      .collect(),
  )
}

pub(crate) struct JobFiles {
  pub(crate) metadata: PathBuf,
  pub(crate) instructions: PathBuf,
  pub(crate) runtime: PathBuf,
  pub(crate) chunks: PathBuf,
  pub(crate) current_graph_snapshot: PathBuf,
  pub(crate) graph_patch_schema: PathBuf,
  pub(crate) output_patch: PathBuf,
}

impl JobFiles {
  pub(crate) fn new(job_dir: &Path) -> Self {
    Self {
      metadata: job_dir.join("metadata.json"),
      instructions: job_dir.join("instructions.md"),
      runtime: job_dir.join("runtime.json"),
      chunks: job_dir.join("chunks.json"),
      current_graph_snapshot: job_dir.join("current_graph_snapshot.json"),
      graph_patch_schema: job_dir.join("graph_patch.schema.json"),
      output_patch: job_dir.join("output_patch.json"),
    }
  }

  pub(crate) fn to_json(&self) -> Value {
    json!({
      "metadata": self.metadata.to_string_lossy(),
      "instructions": self.instructions.to_string_lossy(),
      "runtime": self.runtime.to_string_lossy(),
      "chunks": self.chunks.to_string_lossy(),
      "currentGraphSnapshot": self.current_graph_snapshot.to_string_lossy(),
      "graphPatchSchema": self.graph_patch_schema.to_string_lossy(),
      "outputPatch": self.output_patch.to_string_lossy()
    })
  }

  pub(crate) fn is_complete(&self) -> bool {
    [
      &self.metadata,
      &self.instructions,
      &self.runtime,
      &self.chunks,
      &self.current_graph_snapshot,
      &self.graph_patch_schema,
      &self.output_patch,
    ]
    .into_iter()
    .all(|path| path.is_file())
  }
}

fn runtime_result(job_dir: &Path) -> Value {
  fs::read_to_string(job_dir.join("runtime_result.json"))
    .ok()
    .and_then(|content| serde_json::from_str(&content).ok())
    .unwrap_or(Value::Null)
}

fn job_run_files(job_dir: &Path) -> Value {
  let paths = [
    ("metadata", job_dir.join("metadata.json"), true),
    ("instructions", job_dir.join("instructions.md"), false),
    ("runtime", job_dir.join("runtime.json"), false),
    ("runtimeResult", job_dir.join("runtime_result.json"), false),
    ("chunks", job_dir.join("chunks.json"), false),
    ("message", job_dir.join("message.json"), false),
    ("contextPacket", job_dir.join("context_packet.json"), false),
    ("relevantGraph", job_dir.join("relevant_graph.json"), false),
    ("focusedNode", job_dir.join("focused_node.json"), false),
    ("neighbors", job_dir.join("neighbors.json"), false),
    ("bridgeTexts", job_dir.join("bridge_texts.json"), false),
    ("evidence", job_dir.join("evidence.json"), false),
    ("currentGraphSnapshot", job_dir.join("current_graph_snapshot.json"), false),
    ("graphPatchSchema", job_dir.join("graph_patch.schema.json"), false),
    ("outputPatch", job_dir.join("output_patch.json"), false),
  ];
  let mut files = serde_json::Map::new();
  for (key, path, always_include) in paths {
    if always_include || path.exists() {
      files.insert(key.to_string(), json!(path.to_string_lossy()));
    }
  }
  Value::Object(files)
}

#[cfg(test)]
mod tests {
  use super::assert_safe_job_id;

  #[test]
  fn rejects_empty_current_and_parent_job_ids() {
    for job_id in ["", ".", ".."] {
      assert!(assert_safe_job_id(job_id).is_err(), "{job_id:?} must not resolve outside a concrete job directory");
    }
  }

  #[test]
  fn accepts_generated_job_id_shape() {
    assert!(assert_safe_job_id("job_20260724153000_abc123ef").is_ok());
  }
}
