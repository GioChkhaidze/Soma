use std::collections::HashSet;

use serde_json::{json, Value};

pub const GRAPH_PATCH_SCHEMA_VERSION: i64 = 1;
pub const GRAPH_PATCH_MAX_PROPOSALS: usize = 200;
pub const GRAPH_PATCH_MAX_EVIDENCE_REFERENCES: usize = 200;
pub const GRAPH_PATCH_EVIDENCE_ID_MAX_CHARS: usize = 256;
pub const NODE_BODY_MAX_CHARS: usize = 32_000;
pub const NODE_BODY_MAX_WORDS: usize = 1500;
const GRAPH_PATCH_WARNING_PATH_MAX_CHARS: usize = 256;
const GRAPH_PATCH_WARNING_MESSAGE_MAX_CHARS: usize = 1_000;
const NODE_TITLE_MAX_CHARS: usize = 80;
const NODE_PREVIEW_MAX_CHARS: usize = 180;
const EDGE_BRIDGE_MAX_CHARS: usize = 240;
const EDGE_BRIDGE_MAX_WORDS: usize = 40;

const GRAPH_PATCH_ARRAY_FIELDS: [&str; 9] = [
  "proposed_nodes",
  "proposed_edges",
  "proposed_node_body_updates",
  "proposed_edge_bridge_updates",
  "proposed_message_evidence_attachments",
  "proposed_paths",
  "ambiguities",
  "merge_candidates",
  "warnings",
];

const GRAPH_PATCH_PROPOSAL_FIELDS: [&str; 8] = [
  "proposed_nodes",
  "proposed_edges",
  "proposed_node_body_updates",
  "proposed_edge_bridge_updates",
  "proposed_message_evidence_attachments",
  "proposed_paths",
  "ambiguities",
  "merge_candidates",
];

const EVIDENCE_REFERENCE_FIELDS: [(&str, &str); 2] =
  [("source_chunk_ids", "sourceChunkIds"), ("source_message_ids", "sourceMessageIds")];

const NODE_TYPES: [&str; 9] =
  ["project", "concept", "claim", "decision", "question", "task", "artifact", "source_conversation", "tool"];

const EDGE_TYPES: [&str; 12] = [
  "part_of",
  "supports",
  "contradicts",
  "depends_on",
  "answers",
  "implements",
  "mentions",
  "derived_from",
  "alternative_to",
  "blocks",
  "next_step",
  "mitigates",
];

const NODE_BODY_UPDATE_KINDS: [&str; 2] = ["replace_body", "append_section"];
const AMBIGUITY_KINDS: [&str; 5] =
  ["unclear_node_target", "possible_duplicate", "edge_type_unclear", "insufficient_evidence", "merge_risk"];

#[derive(Debug)]
pub struct ValidationResult {
  pub valid: bool,
  pub errors: Vec<Value>,
  pub warnings: Vec<Value>,
}

pub fn validate_graph_patch_for_review(
  patch: &Value,
  active_node_ids: &HashSet<String>,
  active_edge_ids: &HashSet<String>,
) -> ValidationResult {
  let mut errors = Vec::new();
  let mut warnings = Vec::new();

  if !patch.is_object() {
    errors.push(issue("$", "Graph patch must be a JSON object."));
    return ValidationResult { valid: false, errors, warnings };
  }

  if patch.get("schema_version").and_then(Value::as_i64) != Some(GRAPH_PATCH_SCHEMA_VERSION) {
    errors.push(issue("$.schema_version", &format!("schema_version must be {GRAPH_PATCH_SCHEMA_VERSION}.")));
  }

  for field in GRAPH_PATCH_ARRAY_FIELDS {
    if !patch.get(field).is_some_and(Value::is_array) {
      errors.push(issue(&format!("$.{field}"), "Field is required and must be an array."));
    }
  }
  let proposal_count = graph_patch_proposal_count(patch);
  if proposal_count > GRAPH_PATCH_MAX_PROPOSALS {
    errors.push(issue(
      "$",
      &format!("Graph patch contains {proposal_count} proposals; the maximum is {GRAPH_PATCH_MAX_PROPOSALS}."),
    ));
    return ValidationResult { valid: false, errors, warnings };
  }
  if array_items(patch, "warnings").len() > GRAPH_PATCH_MAX_PROPOSALS {
    errors
      .push(issue("$.warnings", &format!("Graph patch warnings exceed the maximum of {GRAPH_PATCH_MAX_PROPOSALS}.")));
    return ValidationResult { valid: false, errors, warnings };
  }
  validate_patch_warnings(patch, &mut errors, &mut warnings);
  let error_count_before_reference_bounds = errors.len();
  validate_evidence_reference_bounds(patch, &mut errors);
  if errors.len() > error_count_before_reference_bounds {
    return ValidationResult { valid: false, errors, warnings };
  }

  let mut node_refs = active_node_ids.clone();
  for (index, node) in array_items(patch, "proposed_nodes").iter().enumerate() {
    let path = format!("$.proposed_nodes[{index}]");
    if !node.is_object() {
      errors.push(issue(&path, "Patch array items must be objects."));
      continue;
    }
    match proposal_ref(node) {
      Some(temp_id) if node_refs.contains(&temp_id) => {
        errors.push(issue(&format!("{path}.temp_id"), &format!("Duplicate node reference: {temp_id}")));
      }
      Some(temp_id) => {
        node_refs.insert(temp_id);
      }
      None => errors
        .push(issue(&format!("{path}.temp_id"), "Proposed nodes must include temp_id for review and edge references.")),
    }
    validate_enum(node_type(node), &NODE_TYPES, &format!("{path}.type"), "Unsupported node type.", &mut errors);
    validate_required_string(node.get("title"), &format!("{path}.title"), "title is required.", &mut errors);
    validate_char_limit(node.get("title"), &format!("{path}.title"), NODE_TITLE_MAX_CHARS, &mut errors);
    validate_optional_text(node.get("preview"), &format!("{path}.preview"), NODE_PREVIEW_MAX_CHARS, &mut errors);
    validate_required_string(
      node.get("compiled_body"),
      &format!("{path}.compiled_body"),
      "compiled_body is required.",
      &mut errors,
    );
    validate_word_limit(node.get("compiled_body"), &format!("{path}.compiled_body"), NODE_BODY_MAX_WORDS, &mut errors);
    validate_char_limit(node.get("compiled_body"), &format!("{path}.compiled_body"), NODE_BODY_MAX_CHARS, &mut errors);
    validate_required_evidence(node, &path, &mut errors);
    validate_no_trusted_status(node, &path, &mut errors);
  }

  for (index, edge) in array_items(patch, "proposed_edges").iter().enumerate() {
    let path = format!("$.proposed_edges[{index}]");
    if !edge.is_object() {
      errors.push(issue(&path, "Patch array items must be objects."));
      continue;
    }
    validate_enum(edge_type(edge), &EDGE_TYPES, &format!("{path}.type"), "Unsupported edge type.", &mut errors);
    validate_node_ref(edge_source_ref(edge), &node_refs, &format!("{path}.source"), &mut errors);
    validate_node_ref(edge_target_ref(edge), &node_refs, &format!("{path}.target"), &mut errors);
    validate_required_string(edge.get("reason"), &format!("{path}.reason"), "reason is required.", &mut errors);
    validate_required_evidence(edge, &path, &mut errors);
    validate_bridge_text(edge.get("bridge_text"), &format!("{path}.bridge_text"), &mut errors);
    validate_no_trusted_status(edge, &path, &mut errors);
  }

  for (index, update) in array_items(patch, "proposed_node_body_updates").iter().enumerate() {
    let path = format!("$.proposed_node_body_updates[{index}]");
    if !update.is_object() {
      errors.push(issue(&path, "Patch array items must be objects."));
      continue;
    }
    validate_node_ref(
      update.get("target_node_id").or_else(|| update.get("node_id")).and_then(Value::as_str).map(String::from),
      active_node_ids,
      &format!("{path}.target_node_id"),
      &mut errors,
    );
    validate_node_body_update(update, &path, &mut errors);
    validate_optional_string(
      update.get("base_body_version_id"),
      &format!("{path}.base_body_version_id"),
      "base_body_version_id must be a non-empty string when present.",
      &mut errors,
    );
    validate_required_evidence(update, &path, &mut errors);
    validate_no_trusted_status(update, &path, &mut errors);
  }

  for (index, update) in array_items(patch, "proposed_edge_bridge_updates").iter().enumerate() {
    let path = format!("$.proposed_edge_bridge_updates[{index}]");
    if !update.is_object() {
      errors.push(issue(&path, "Patch array items must be objects."));
      continue;
    }
    let edge_id = update.get("target_edge_id").or_else(|| update.get("edge_id")).and_then(Value::as_str);
    if edge_id.is_none_or(|value| value.trim().is_empty()) {
      errors.push(issue(&format!("{path}.edge_id"), "edge_id is required."));
    }
    if let Some(edge_id) = edge_id {
      if !active_edge_ids.contains(edge_id) {
        errors.push(issue(&format!("{path}.edge_id"), &format!("Unknown edge id: {edge_id}")));
      }
    }
    validate_required_string(
      update.get("bridge_text"),
      &format!("{path}.bridge_text"),
      "bridge_text is required.",
      &mut errors,
    );
    validate_bridge_text(update.get("bridge_text"), &format!("{path}.bridge_text"), &mut errors);
    validate_optional_string(
      update.get("base_edge_updated_at"),
      &format!("{path}.base_edge_updated_at"),
      "base_edge_updated_at must be a non-empty string when present.",
      &mut errors,
    );
    validate_required_string(update.get("reason"), &format!("{path}.reason"), "reason is required.", &mut errors);
    validate_required_evidence(update, &path, &mut errors);
    validate_no_trusted_status(update, &path, &mut errors);
  }

  for (index, attachment) in array_items(patch, "proposed_message_evidence_attachments").iter().enumerate() {
    let path = format!("$.proposed_message_evidence_attachments[{index}]");
    if !attachment.is_object() {
      errors.push(issue(&path, "Patch array items must be objects."));
      continue;
    }
    validate_required_string(
      attachment.get("message_id"),
      &format!("{path}.message_id"),
      "message_id is required.",
      &mut errors,
    );
    validate_enum(
      attachment.get("target_entity_type").and_then(Value::as_str).map(String::from),
      &["node", "edge", "node_body_version"],
      &format!("{path}.target_entity_type"),
      "Unsupported evidence target entity type.",
      &mut errors,
    );
    validate_required_string(
      attachment.get("target_entity_id"),
      &format!("{path}.target_entity_id"),
      "target_entity_id is required.",
      &mut errors,
    );
    validate_required_string(attachment.get("reason"), &format!("{path}.reason"), "reason is required.", &mut errors);
    validate_no_trusted_status(attachment, &path, &mut errors);
  }

  for (index, path_value) in array_items(patch, "proposed_paths").iter().enumerate() {
    let path = format!("$.proposed_paths[{index}]");
    validate_required_string(path_value.get("title"), &format!("{path}.title"), "title is required.", &mut errors);
    validate_required_array(
      path_value.get("node_ids"),
      &format!("{path}.node_ids"),
      "node_ids must contain at least one node id.",
      &mut errors,
    );
    validate_required_array(
      path_value.get("edge_ids"),
      &format!("{path}.edge_ids"),
      "edge_ids must contain at least one edge id.",
      &mut errors,
    );
    validate_required_string(path_value.get("reason"), &format!("{path}.reason"), "reason is required.", &mut errors);
  }

  for (index, ambiguity) in array_items(patch, "ambiguities").iter().enumerate() {
    let path = format!("$.ambiguities[{index}]");
    validate_enum(
      ambiguity.get("kind").and_then(Value::as_str).map(String::from),
      &AMBIGUITY_KINDS,
      &format!("{path}.kind"),
      "Unsupported ambiguity kind.",
      &mut errors,
    );
    validate_required_string(ambiguity.get("prompt"), &format!("{path}.prompt"), "prompt is required.", &mut errors);
  }

  for (index, candidate) in array_items(patch, "merge_candidates").iter().enumerate() {
    let path = format!("$.merge_candidates[{index}]");
    let refs = candidate
      .get("candidate_node_ids")
      .or_else(|| candidate.get("candidate_node_refs"))
      .or_else(|| candidate.get("node_refs"))
      .and_then(Value::as_array)
      .map(Vec::len)
      .unwrap_or(0);
    if refs < 2 {
      errors.push(issue(&format!("{path}.candidate_node_ids"), "Merge candidates must reference at least two nodes."));
    }
    validate_required_string(candidate.get("reason"), &format!("{path}.reason"), "reason is required.", &mut errors);
    validate_char_limit(
      candidate.get("proposed_title"),
      &format!("{path}.proposed_title"),
      NODE_TITLE_MAX_CHARS,
      &mut errors,
    );
    validate_char_limit(
      candidate.get("proposed_compiled_body"),
      &format!("{path}.proposed_compiled_body"),
      NODE_BODY_MAX_CHARS,
      &mut errors,
    );
    validate_required_evidence(candidate, &path, &mut errors);
  }

  ValidationResult { valid: errors.is_empty(), errors, warnings }
}

pub fn empty_graph_patch() -> Value {
  json!({
    "schema_version": GRAPH_PATCH_SCHEMA_VERSION,
    "proposed_nodes": [],
    "proposed_edges": [],
    "proposed_node_body_updates": [],
    "proposed_edge_bridge_updates": [],
    "proposed_message_evidence_attachments": [],
    "proposed_paths": [],
    "ambiguities": [],
    "merge_candidates": [],
    "warnings": []
  })
}

pub(crate) fn graph_patch_proposal_count(patch: &Value) -> usize {
  GRAPH_PATCH_PROPOSAL_FIELDS.iter().filter_map(|field| patch.get(field).and_then(Value::as_array)).map(Vec::len).sum()
}

pub(crate) fn graph_patch_is_empty(patch: &Value) -> bool {
  patch.get("schema_version").and_then(Value::as_i64) == Some(GRAPH_PATCH_SCHEMA_VERSION)
    && GRAPH_PATCH_ARRAY_FIELDS.iter().all(|field| patch.get(field).is_some_and(Value::is_array))
    && graph_patch_proposal_count(patch) == 0
    && patch.get("warnings").and_then(Value::as_array).is_some_and(Vec::is_empty)
}

pub fn complete_graph_patch(value: &Value) -> Value {
  let mut patch = empty_graph_patch();
  let Some(input) = value.as_object() else {
    return value.clone();
  };
  if let Some(version) = input.get("schema_version").or_else(|| input.get("schemaVersion")) {
    patch["schema_version"] = version.clone();
  }
  for field in GRAPH_PATCH_ARRAY_FIELDS {
    if let Some(items) = input.get(field).or_else(|| graph_patch_array_alias(field).and_then(|alias| input.get(alias)))
    {
      patch[field] = items.clone();
    }
  }
  patch
}

fn graph_patch_array_alias(field: &str) -> Option<&'static str> {
  match field {
    "proposed_nodes" => Some("proposedNodes"),
    "proposed_edges" => Some("proposedEdges"),
    "proposed_node_body_updates" => Some("proposedNodeBodyUpdates"),
    "proposed_edge_bridge_updates" => Some("proposedEdgeBridgeUpdates"),
    "proposed_message_evidence_attachments" => Some("proposedMessageEvidenceAttachments"),
    "proposed_paths" => Some("proposedPaths"),
    "merge_candidates" => Some("mergeCandidates"),
    _ => None,
  }
}

pub fn normalize_graph_patch_for_review(patch: &Value) -> (Value, Vec<Value>) {
  let mut normalized = patch.clone();
  let mut warnings = Vec::new();
  let Some(root) = normalized.as_object_mut() else {
    return (normalized, warnings);
  };

  if let Some(nodes) = root.get_mut("proposed_nodes").and_then(Value::as_array_mut) {
    for (index, node) in nodes.iter_mut().enumerate() {
      let Some(node) = node.as_object_mut() else {
        continue;
      };
      let current = node
        .get("node_type")
        .or_else(|| node.get("type"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string);
      match current.as_deref() {
        Some(value) => {
          if let Some(canonical) = canonical_node_type(value) {
            if canonical != value {
              set_type_field(node, "node_type", "type", canonical);
              warnings.push(issue(
                &format!("$.proposed_nodes[{index}].type"),
                &format!("Normalized node type `{value}` to `{canonical}`."),
              ));
            }
          }
        }
        None => {
          node.insert("type".to_string(), Value::String("concept".to_string()));
          warnings
            .push(issue(&format!("$.proposed_nodes[{index}].type"), "Missing node type normalized to `concept`."));
        }
      }
    }
  }

  if let Some(edges) = root.get_mut("proposed_edges").and_then(Value::as_array_mut) {
    for (index, edge) in edges.iter_mut().enumerate() {
      let Some(edge) = edge.as_object_mut() else {
        continue;
      };
      let current = edge
        .get("edge_type")
        .or_else(|| edge.get("type"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string);
      if let Some(value) = current.as_deref() {
        if let Some(canonical) = canonical_edge_type(value) {
          if canonical != value {
            set_type_field(edge, "edge_type", "type", canonical);
            warnings.push(issue(
              &format!("$.proposed_edges[{index}].type"),
              &format!("Normalized edge type `{value}` to `{canonical}`."),
            ));
          }
        }
      }

      let has_reason = edge.get("reason").and_then(Value::as_str).is_some_and(|reason| !reason.trim().is_empty());
      if !has_reason {
        let reason = edge
          .get("bridge_text")
          .and_then(Value::as_str)
          .map(str::trim)
          .filter(|text| !text.is_empty())
          .unwrap_or("AI compiler proposed this source-backed connection for review.")
          .to_string();
        edge.insert("reason".to_string(), Value::String(reason));
        warnings
          .push(issue(&format!("$.proposed_edges[{index}].reason"), "Missing edge reason filled from bridge text."));
      }
    }
  }

  (normalized, warnings)
}

pub fn graph_patch_schema() -> Value {
  let evidence_reference = json!({
    "type": "string",
    "minLength": 1,
    "maxLength": GRAPH_PATCH_EVIDENCE_ID_MAX_CHARS
  });
  let source_chunk_ids = json!({
    "type": "array",
    "minItems": 1,
    "maxItems": GRAPH_PATCH_MAX_EVIDENCE_REFERENCES,
    "description": "Source chunk ids that directly support this proposed graph object.",
    "items": evidence_reference.clone()
  });
  let source_message_ids = json!({
    "type": "array",
    "maxItems": GRAPH_PATCH_MAX_EVIDENCE_REFERENCES,
    "items": evidence_reference.clone()
  });
  let node_ref = json!({ "type": "string", "minLength": 1 });
  json!({
    "$schema": "https://json-schema.org/draft/2020-12/schema",
    "$id": "soma.graph_patch.v0.schema.json",
    "title": "Soma Graph Patch v0",
    "type": "object",
    "required": [
      "schema_version",
      "proposed_nodes",
      "proposed_edges",
      "proposed_node_body_updates",
      "proposed_edge_bridge_updates",
      "proposed_message_evidence_attachments",
      "proposed_paths",
      "ambiguities",
      "merge_candidates",
      "warnings"
    ],
    "properties": {
      "schema_version": { "const": GRAPH_PATCH_SCHEMA_VERSION },
      "proposed_nodes": {
        "type": "array",
        "maxItems": GRAPH_PATCH_MAX_PROPOSALS,
        "description": concat!(
          "New compiled conversation sections. Use short titles and target 300-1500 word ",
          "compiled_body values when source material allows."
        ),
        "items": {
          "type": "object",
          "required": ["temp_id", "type", "title", "compiled_body"],
          "properties": {
            "temp_id": { "type": "string", "minLength": 1 },
            "type": { "enum": NODE_TYPES },
            "node_type": { "enum": NODE_TYPES },
            "title": {
              "type": "string",
              "minLength": 1,
              "maxLength": NODE_TITLE_MAX_CHARS,
              "description": "Short graph label. Prefer 2-6 words; no sentence titles."
            },
            "preview": {
              "type": "string",
              "maxLength": NODE_PREVIEW_MAX_CHARS,
              "description": "One compact graph-card preview sentence."
            },
            "compiled_body": {
              "type": "string",
              "minLength": 1,
              "maxLength": NODE_BODY_MAX_CHARS,
              "description": concat!(
                "Substantial compiled conversation section. Target 300-1500 words when source material allows; ",
                "synthesize reasoning instead of dumping transcript."
              )
            },
            "reason": { "type": "string" },
            "source_chunk_ids": source_chunk_ids.clone(),
            "source_message_ids": source_message_ids.clone()
          }
        }
      },
      "proposed_edges": {
        "type": "array",
        "maxItems": GRAPH_PATCH_MAX_PROPOSALS,
        "description": concat!(
          "Typed source-backed connections. bridge_text should be one short useful sentence, ",
          "usually 5-25 words."
        ),
        "items": {
          "type": "object",
          "required": ["type", "reason"],
          "allOf": [
            {
              "anyOf": [
                { "required": ["source_temp_id"] },
                { "required": ["source_node_id"] }
              ]
            },
            {
              "anyOf": [
                { "required": ["target_temp_id"] },
                { "required": ["target_node_id"] }
              ]
            }
          ],
          "properties": {
            "temp_id": { "type": "string" },
            "type": { "enum": EDGE_TYPES },
            "edge_type": { "enum": EDGE_TYPES },
            "source_temp_id": node_ref.clone(),
            "target_temp_id": node_ref.clone(),
            "source_node_id": node_ref.clone(),
            "target_node_id": node_ref.clone(),
            "bridge_text": {
              "type": "string",
              "maxLength": EDGE_BRIDGE_MAX_CHARS,
              "description": concat!(
                "Short readable bridge for path walking. Prefer 5-25 words and only include it when it ",
                "clarifies the relation."
              )
            },
            "reason": { "type": "string", "minLength": 1 },
            "source_chunk_ids": source_chunk_ids.clone(),
            "source_message_ids": source_message_ids.clone()
          }
        }
      },
      "proposed_node_body_updates": {
        "type": "array",
        "maxItems": GRAPH_PATCH_MAX_PROPOSALS,
        "description": "Source-backed updates to existing compiled node bodies.",
        "items": {
          "type": "object",
          "required": ["target_node_id", "update_kind", "reason"],
          "properties": {
            "target_node_id": node_ref.clone(),
            "node_id": node_ref.clone(),
            "base_body_version_id": {
              "type": "string",
              "minLength": 1,
              "description": "Host-managed optimistic precondition copied from the job or chat snapshot."
            },
            "update_kind": { "enum": NODE_BODY_UPDATE_KINDS },
            "compiled_body": {
              "type": "string",
              "maxLength": NODE_BODY_MAX_CHARS,
              "description": "Replacement compiled body. Target 300-1500 words when replacing a full node body."
            },
            "section_text": {
              "type": "string",
              "maxLength": NODE_BODY_MAX_CHARS,
              "description": "Substantial new section text, source-backed and readable."
            },
            "reason": { "type": "string", "minLength": 1 },
            "source_chunk_ids": source_chunk_ids.clone(),
            "source_message_ids": source_message_ids.clone()
          }
        }
      },
      "proposed_edge_bridge_updates": {
        "type": "array",
        "maxItems": GRAPH_PATCH_MAX_PROPOSALS,
        "description": "Short source-backed bridge text revisions for existing edges.",
        "items": {
          "type": "object",
          "required": ["target_edge_id", "bridge_text", "reason"],
          "properties": {
            "target_edge_id": { "type": "string", "minLength": 1 },
            "edge_id": { "type": "string", "minLength": 1 },
            "base_edge_updated_at": {
              "type": "string",
              "minLength": 1,
              "description": "Host-managed optimistic precondition copied from the job snapshot."
            },
            "bridge_text": {
              "type": "string",
              "minLength": 1,
              "maxLength": EDGE_BRIDGE_MAX_CHARS,
              "pattern": r"\S",
              "description": "Short readable bridge text, usually 5-25 words."
            },
            "reason": { "type": "string", "minLength": 1 },
            "source_chunk_ids": source_chunk_ids.clone(),
            "source_message_ids": source_message_ids.clone()
          }
        }
      },
      "proposed_message_evidence_attachments": {
        "type": "array",
        "maxItems": GRAPH_PATCH_MAX_PROPOSALS,
        "items": {
          "type": "object",
          "required": ["message_id", "target_entity_type", "target_entity_id", "reason"],
          "properties": {
            "message_id": { "type": "string", "minLength": 1 },
            "target_entity_type": { "enum": ["node", "edge", "node_body_version"] },
            "target_entity_id": { "type": "string", "minLength": 1 },
            "quote_excerpt": { "type": "string" },
            "reason": { "type": "string", "minLength": 1 }
          }
        }
      },
      "proposed_paths": {
        "type": "array",
        "maxItems": GRAPH_PATCH_MAX_PROPOSALS,
        "items": {
          "type": "object",
          "required": ["title", "node_ids", "edge_ids", "reason"],
          "properties": {
            "title": { "type": "string", "minLength": 1 },
            "node_ids": { "type": "array", "minItems": 1, "items": { "type": "string" } },
            "edge_ids": { "type": "array", "minItems": 1, "items": { "type": "string" } },
            "reason": { "type": "string", "minLength": 1 },
            "source_chunk_ids": source_chunk_ids.clone(),
            "source_message_ids": source_message_ids.clone()
          }
        }
      },
      "ambiguities": {
        "type": "array",
        "maxItems": GRAPH_PATCH_MAX_PROPOSALS,
        "items": {
          "type": "object",
          "required": ["kind", "prompt"],
          "properties": {
            "id": { "type": "string" },
            "kind": { "enum": AMBIGUITY_KINDS },
            "prompt": { "type": "string", "minLength": 1 },
            "candidate_node_ids": { "type": "array", "items": { "type": "string" } },
            "candidate_edge_ids": { "type": "array", "items": { "type": "string" } },
            "source_chunk_ids": source_chunk_ids.clone(),
            "source_message_ids": source_message_ids.clone()
          }
        }
      },
      "merge_candidates": {
        "type": "array",
        "maxItems": GRAPH_PATCH_MAX_PROPOSALS,
        "description": "Use when proposed or existing nodes overlap instead of creating duplicates.",
        "items": {
          "type": "object",
          "required": ["candidate_node_ids", "reason"],
          "properties": {
            "candidate_node_ids": { "type": "array", "minItems": 2, "items": { "type": "string" } },
            "candidate_node_refs": { "type": "array", "minItems": 2, "items": { "type": "string" } },
            "preferred_survivor_node_id": { "type": "string" },
            "reason": { "type": "string", "minLength": 1 },
            "proposed_title": { "type": "string", "maxLength": NODE_TITLE_MAX_CHARS },
            "proposed_compiled_body": {
              "type": "string",
              "maxLength": NODE_BODY_MAX_CHARS,
              "description": "Suggested fused body when two nodes overlap. Target 300-1500 words."
            },
            "source_chunk_ids": source_chunk_ids.clone(),
            "source_message_ids": source_message_ids.clone()
          }
        }
      },
      "warnings": {
        "type": "array",
        "maxItems": GRAPH_PATCH_MAX_PROPOSALS,
        "items": {
          "type": "object",
          "required": ["message"],
          "properties": {
            "path": {
              "type": "string",
              "minLength": 1,
              "maxLength": GRAPH_PATCH_WARNING_PATH_MAX_CHARS,
              "pattern": r"\S"
            },
            "message": {
              "type": "string",
              "minLength": 1,
              "maxLength": GRAPH_PATCH_WARNING_MESSAGE_MAX_CHARS,
              "pattern": r"\S"
            }
          }
        }
      }
    }
  })
}

fn validate_evidence_reference_bounds(patch: &Value, errors: &mut Vec<Value>) {
  for proposal_field in GRAPH_PATCH_PROPOSAL_FIELDS {
    for (proposal_index, proposal) in array_items(patch, proposal_field).iter().enumerate() {
      let Some(proposal) = proposal.as_object() else {
        continue;
      };
      for (canonical_field, alias_field) in EVIDENCE_REFERENCE_FIELDS {
        for field in [canonical_field, alias_field] {
          let Some(value) = proposal.get(field) else {
            continue;
          };
          let path = format!("$.{proposal_field}[{proposal_index}].{field}");
          let Some(references) = value.as_array() else {
            errors.push(issue(&path, "Evidence references must be an array."));
            continue;
          };
          if references.len() > GRAPH_PATCH_MAX_EVIDENCE_REFERENCES {
            errors.push(issue(
              &path,
              &format!("Evidence references must not exceed {GRAPH_PATCH_MAX_EVIDENCE_REFERENCES} items."),
            ));
            continue;
          }
          for (reference_index, reference) in references.iter().enumerate() {
            let item_path = format!("{path}[{reference_index}]");
            let Some(identifier) = reference.as_str() else {
              errors.push(issue(&item_path, "Evidence reference identifiers must be strings."));
              continue;
            };
            if identifier.chars().take(GRAPH_PATCH_EVIDENCE_ID_MAX_CHARS + 1).count()
              > GRAPH_PATCH_EVIDENCE_ID_MAX_CHARS
            {
              errors.push(issue(
                &item_path,
                &format!(
                  "Evidence reference identifiers must not exceed {GRAPH_PATCH_EVIDENCE_ID_MAX_CHARS} characters."
                ),
              ));
            } else if identifier.trim().is_empty() {
              errors.push(issue(&item_path, "Evidence reference identifiers must not be empty."));
            }
          }
        }
      }
    }
  }
}

fn validate_patch_warnings(patch: &Value, errors: &mut Vec<Value>, warnings: &mut Vec<Value>) {
  for (index, warning) in array_items(patch, "warnings").iter().enumerate() {
    let warning_path = format!("$.warnings[{index}]");
    if !warning.is_object() {
      errors.push(issue(&warning_path, "Patch warnings must be objects."));
      continue;
    }

    let error_count = errors.len();
    validate_optional_string(
      warning.get("path"),
      &format!("{warning_path}.path"),
      "Warning path must be a non-empty string when present.",
      errors,
    );
    validate_required_string(
      warning.get("message"),
      &format!("{warning_path}.message"),
      "Warning message is required.",
      errors,
    );
    validate_char_limit(
      warning.get("path"),
      &format!("{warning_path}.path"),
      GRAPH_PATCH_WARNING_PATH_MAX_CHARS,
      errors,
    );
    validate_char_limit(
      warning.get("message"),
      &format!("{warning_path}.message"),
      GRAPH_PATCH_WARNING_MESSAGE_MAX_CHARS,
      errors,
    );
    if errors.len() != error_count {
      continue;
    }

    let path = warning.get("path").and_then(Value::as_str).map(str::trim).unwrap_or("$");
    let message = warning.get("message").and_then(Value::as_str).map(str::trim).unwrap_or_default();
    warnings.push(issue(path, message));
  }
}

pub fn source_chunk_ids(value: &Value) -> Vec<String> {
  let mut seen = HashSet::new();
  value
    .get("source_chunk_ids")
    .or_else(|| value.get("sourceChunkIds"))
    .and_then(Value::as_array)
    .into_iter()
    .flatten()
    .filter_map(Value::as_str)
    .map(str::trim)
    .filter(|id| !id.is_empty())
    .filter(|id| seen.insert(*id))
    .map(String::from)
    .collect()
}

pub fn source_message_ids(value: &Value) -> Vec<String> {
  let mut seen = HashSet::new();
  value
    .get("source_message_ids")
    .or_else(|| value.get("sourceMessageIds"))
    .and_then(Value::as_array)
    .into_iter()
    .flatten()
    .filter_map(Value::as_str)
    .map(str::trim)
    .filter(|id| !id.is_empty())
    .filter(|id| seen.insert(*id))
    .map(String::from)
    .collect()
}

pub fn attach_source_message_id(mut patch: Value, message_id: Option<&str>) -> Value {
  let Some(message_id) = message_id.map(str::trim).filter(|value| !value.is_empty()) else {
    return patch;
  };
  for field in [
    "proposed_nodes",
    "proposed_edges",
    "proposed_node_body_updates",
    "proposed_edge_bridge_updates",
    "proposed_paths",
    "merge_candidates",
  ] {
    attach_source_message_id_to_items(&mut patch, field, message_id);
  }
  patch
}

fn attach_source_message_id_to_items(patch: &mut Value, field: &str, message_id: &str) {
  let Some(items) = patch.get_mut(field).and_then(Value::as_array_mut) else {
    return;
  };
  for item in items {
    if !item.is_object() {
      continue;
    }
    if source_chunk_ids(item).is_empty() && source_message_ids(item).is_empty() {
      item["source_message_ids"] = json!([message_id]);
    }
  }
}

pub fn proposal_ref(value: &Value) -> Option<String> {
  value
    .get("temp_id")
    .or_else(|| value.get("id"))
    .and_then(Value::as_str)
    .map(str::trim)
    .filter(|id| !id.is_empty())
    .map(String::from)
}

pub fn edge_source_ref(edge: &Value) -> Option<String> {
  edge
    .get("source_node_id")
    .or_else(|| edge.get("source_temp_id"))
    .or_else(|| edge.get("source_node_ref"))
    .or_else(|| edge.get("source"))
    .and_then(Value::as_str)
    .map(String::from)
}

pub fn edge_target_ref(edge: &Value) -> Option<String> {
  edge
    .get("target_node_id")
    .or_else(|| edge.get("target_temp_id"))
    .or_else(|| edge.get("target_node_ref"))
    .or_else(|| edge.get("target"))
    .and_then(Value::as_str)
    .map(String::from)
}

pub fn edge_type(edge: &Value) -> Option<String> {
  edge.get("edge_type").or_else(|| edge.get("type")).and_then(Value::as_str).map(String::from)
}

pub fn node_type(node: &Value) -> Option<String> {
  node.get("node_type").or_else(|| node.get("type")).and_then(Value::as_str).map(String::from)
}

pub fn word_count(value: &str) -> usize {
  value.split_whitespace().count()
}

fn array_items<'a>(value: &'a Value, field: &str) -> Vec<&'a Value> {
  value.get(field).and_then(Value::as_array).map(|items| items.iter().collect()).unwrap_or_default()
}

fn validate_enum(value: Option<String>, allowed: &[&str], path: &str, message: &str, errors: &mut Vec<Value>) {
  if !value.as_deref().is_some_and(|item| allowed.contains(&item)) {
    errors.push(issue(path, &format!("{message} Allowed values: {}.", allowed.join(", "))));
  }
}

fn validate_required_string(value: Option<&Value>, path: &str, message: &str, errors: &mut Vec<Value>) {
  if value.and_then(Value::as_str).is_none_or(|item| item.trim().is_empty()) {
    errors.push(issue(path, message));
  }
}

fn validate_optional_string(value: Option<&Value>, path: &str, message: &str, errors: &mut Vec<Value>) {
  if value.is_some_and(|value| value.as_str().is_none_or(|item| item.trim().is_empty())) {
    errors.push(issue(path, message));
  }
}

fn validate_optional_text(value: Option<&Value>, path: &str, limit: usize, errors: &mut Vec<Value>) {
  let Some(value) = value else {
    return;
  };
  if !value.is_string() {
    errors.push(issue(path, "Optional text must be a string when present."));
    return;
  }
  validate_char_limit(Some(value), path, limit, errors);
}

fn validate_char_limit(value: Option<&Value>, path: &str, limit: usize, errors: &mut Vec<Value>) {
  if value.and_then(Value::as_str).is_some_and(|text| text.chars().count() > limit) {
    errors.push(issue(path, &format!("Text must not exceed {limit} characters.")));
  }
}

fn validate_required_array(value: Option<&Value>, path: &str, message: &str, errors: &mut Vec<Value>) {
  let valid = value.and_then(Value::as_array).is_some_and(|items| {
    !items.is_empty() && items.iter().all(|item| item.as_str().is_some_and(|id| !id.trim().is_empty()))
  });
  if !valid {
    errors.push(issue(path, message));
  }
}

fn validate_node_ref(value: Option<String>, known_refs: &HashSet<String>, path: &str, errors: &mut Vec<Value>) {
  match value {
    Some(value) if known_refs.contains(&value) => {}
    Some(value) => errors.push(issue(path, &format!("Unknown node reference: {value}"))),
    None => errors.push(issue(path, "Node reference is required.")),
  }
}

fn validate_node_body_update(update: &Value, path: &str, errors: &mut Vec<Value>) {
  let kind = update.get("update_kind").and_then(Value::as_str);
  validate_enum(
    kind.map(String::from),
    &NODE_BODY_UPDATE_KINDS,
    &format!("{path}.update_kind"),
    "Unsupported node body update kind.",
    errors,
  );
  validate_required_string(update.get("reason"), &format!("{path}.reason"), "reason is required.", errors);

  match kind {
    Some("replace_body") => {
      validate_required_string(
        update.get("compiled_body"),
        &format!("{path}.compiled_body"),
        "compiled_body is required for replace_body.",
        errors,
      );
      validate_word_limit(update.get("compiled_body"), &format!("{path}.compiled_body"), NODE_BODY_MAX_WORDS, errors);
      validate_char_limit(update.get("compiled_body"), &format!("{path}.compiled_body"), NODE_BODY_MAX_CHARS, errors);
    }
    Some("append_section") => {
      validate_required_string(
        update.get("section_text"),
        &format!("{path}.section_text"),
        "section_text is required for append_section.",
        errors,
      );
      validate_word_limit(update.get("section_text"), &format!("{path}.section_text"), NODE_BODY_MAX_WORDS, errors);
      validate_char_limit(update.get("section_text"), &format!("{path}.section_text"), NODE_BODY_MAX_CHARS, errors);
    }
    _ => {}
  }
}

fn validate_required_evidence(value: &Value, path: &str, errors: &mut Vec<Value>) {
  if source_chunk_ids(value).is_empty() && source_message_ids(value).is_empty() {
    errors.push(issue(
      &format!("{path}.source_chunk_ids"),
      "AI-compiled graph proposals must include at least one source chunk id or source message id.",
    ));
  }
}

fn validate_bridge_text(value: Option<&Value>, path: &str, errors: &mut Vec<Value>) {
  let Some(value) = value else {
    return;
  };
  if value.is_null() || value.as_str() == Some("") {
    return;
  }
  let Some(text) = value.as_str() else {
    errors.push(issue(path, "bridge_text must be a string when present."));
    return;
  };
  if word_count(text) > EDGE_BRIDGE_MAX_WORDS {
    errors.push(issue(path, &format!("bridge_text must not exceed {EDGE_BRIDGE_MAX_WORDS} words.")));
  }
  if text.chars().count() > EDGE_BRIDGE_MAX_CHARS {
    errors.push(issue(path, &format!("bridge_text must not exceed {EDGE_BRIDGE_MAX_CHARS} characters.")));
  }
}

fn validate_word_limit(value: Option<&Value>, path: &str, limit: usize, errors: &mut Vec<Value>) {
  if let Some(text) = value.and_then(Value::as_str) {
    if word_count(text) > limit {
      errors.push(issue(path, &format!("compiled_body must not exceed {limit} words.")));
    }
  }
}

fn validate_no_trusted_status(value: &Value, path: &str, errors: &mut Vec<Value>) {
  if value.get("status").and_then(Value::as_str).is_some_and(|status| status != "proposed") {
    errors.push(issue(path, "Patch output cannot create trusted graph state directly."));
  }
  if value.get("trusted").and_then(Value::as_bool) == Some(true) {
    errors.push(issue(path, "Patch output cannot mark itself trusted."));
  }
}

fn issue(path: &str, message: &str) -> Value {
  json!({ "path": path, "message": message })
}

fn set_type_field(
  value: &mut serde_json::Map<String, Value>,
  preferred_key: &str,
  fallback_key: &str,
  canonical: &str,
) {
  let key = if value.contains_key(preferred_key) { preferred_key } else { fallback_key };
  value.insert(key.to_string(), Value::String(canonical.to_string()));
}

fn canonical_node_type(value: &str) -> Option<&'static str> {
  let key = canonical_type_key(value);
  if let Some(allowed) = NODE_TYPES.iter().copied().find(|item| *item == key) {
    return Some(allowed);
  }
  match key.as_str() {
    "topic" | "theme" | "idea" | "insight" | "section" | "summary" => Some("concept"),
    "finding" | "observation" | "assertion" => Some("claim"),
    "decision_point" | "choice" => Some("decision"),
    "question_mark" | "open_question" => Some("question"),
    "todo" | "action" | "action_item" | "next_action" => Some("task"),
    "file" | "document" | "doc" | "output" => Some("artifact"),
    "conversation" | "chat" | "transcript" | "source" => Some("source_conversation"),
    "utility" | "tooling" => Some("tool"),
    "project_goal" | "workspace" => Some("project"),
    _ => None,
  }
}

fn canonical_edge_type(value: &str) -> Option<&'static str> {
  let key = canonical_type_key(value);
  if let Some(allowed) = EDGE_TYPES.iter().copied().find(|item| *item == key) {
    return Some(allowed);
  }
  match key.as_str() {
    "clarifies" | "explains" | "elaborates" | "reinforces" | "justifies" => Some("supports"),
    "precedes" | "then" | "next" | "next_test" | "would_next_test" | "would_test_next" | "follow_up" => {
      Some("next_step")
    }
    "requires" | "needs" | "requires_first" | "depends" => Some("depends_on"),
    "references" | "relates_to" | "related_to" | "connects_to" | "associated_with" => Some("mentions"),
    "solves" | "resolves" | "responds_to" => Some("answers"),
    "creates" | "builds" | "realizes" => Some("implements"),
    "comes_from" | "extracted_from" | "based_on" => Some("derived_from"),
    "alternative" | "instead_of" => Some("alternative_to"),
    "prevents" | "reduces" | "softens" => Some("mitigates"),
    _ => None,
  }
}

fn canonical_type_key(value: &str) -> String {
  value
    .trim()
    .to_ascii_lowercase()
    .chars()
    .map(|ch| if ch == '-' || ch == ' ' || ch == '/' { '_' } else { ch })
    .collect()
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn complete_graph_patch_maps_camel_case_arrays() {
    let patch = complete_graph_patch(&json!({
      "schemaVersion": 1,
      "proposedNodes": [{
        "temp_id": "node_alias",
        "type": "question",
        "title": "Alias Patch",
        "compiled_body": "Camel case arrays should survive normalization.",
        "source_message_ids": ["message_1"]
      }],
      "proposedEdges": [],
      "proposedNodeBodyUpdates": [],
      "proposedEdgeBridgeUpdates": [],
      "proposedMessageEvidenceAttachments": [],
      "proposedPaths": [],
      "ambiguities": [],
      "mergeCandidates": [],
      "warnings": [{ "path": "$", "message": "Keep runtime warnings." }]
    }));

    assert_eq!(patch["schema_version"], 1);
    assert_eq!(patch["proposed_nodes"][0]["title"], "Alias Patch");
    assert!(patch["proposed_edges"].as_array().unwrap().is_empty());
    assert!(patch["merge_candidates"].as_array().unwrap().is_empty());
    assert_eq!(patch["warnings"][0]["message"], "Keep runtime warnings.");
  }

  #[test]
  fn warnings_are_required_but_not_counted_as_proposals() {
    let mut patch = empty_graph_patch();
    patch["warnings"] = json!([{ "path": "$", "message": "Informational." }]);

    assert_eq!(graph_patch_proposal_count(&patch), 0);
    assert!(!graph_patch_is_empty(&patch));

    patch.as_object_mut().unwrap().remove("warnings");
    let result = validate_graph_patch_for_review(&patch, &HashSet::new(), &HashSet::new());

    assert!(!result.valid);
    assert!(result.errors.iter().any(|error| error["path"] == "$.warnings"));
  }

  #[test]
  fn model_warnings_are_bounded_and_normalized_for_import_results() {
    let mut patch = empty_graph_patch();
    patch["warnings"] = json!([{
      "path": " $.proposed_nodes[0] ",
      "message": " Check the proposed concept. ",
      "provider_detail": { "must_not_cross": true }
    }]);

    let result = validate_graph_patch_for_review(&patch, &HashSet::new(), &HashSet::new());

    assert!(result.valid, "{:?}", result.errors);
    assert_eq!(
      result.warnings,
      [json!({
        "path": "$.proposed_nodes[0]",
        "message": "Check the proposed concept."
      })]
    );
    let warning_schema = &graph_patch_schema()["properties"]["warnings"]["items"];
    assert_eq!(warning_schema["required"], json!(["message"]));
    assert_eq!(warning_schema["properties"]["path"]["maxLength"], GRAPH_PATCH_WARNING_PATH_MAX_CHARS);
    assert_eq!(warning_schema["properties"]["message"]["maxLength"], GRAPH_PATCH_WARNING_MESSAGE_MAX_CHARS);

    patch["warnings"][0]["message"] = json!("x".repeat(GRAPH_PATCH_WARNING_MESSAGE_MAX_CHARS + 1));
    let invalid = validate_graph_patch_for_review(&patch, &HashSet::new(), &HashSet::new());
    assert!(!invalid.valid);
    assert!(invalid.errors.iter().any(|error| error["path"] == "$.warnings[0].message"));
    assert!(invalid.warnings.is_empty());
  }

  #[test]
  fn edge_bridge_updates_require_acceptance_ready_text_and_match_the_schema() {
    let mut active_edge_ids = HashSet::new();
    active_edge_ids.insert("edge_existing".to_string());
    let mut patch = empty_graph_patch();
    patch["proposed_edge_bridge_updates"] = json!([{
      "target_edge_id": "edge_existing",
      "reason": "Keep validation aligned with acceptance.",
      "source_message_ids": ["message_1"]
    }]);

    let missing = validate_graph_patch_for_review(&patch, &HashSet::new(), &active_edge_ids);
    assert!(!missing.valid);
    assert!(missing.errors.iter().any(|error| error["path"] == "$.proposed_edge_bridge_updates[0].bridge_text"));

    patch["proposed_edge_bridge_updates"][0]["bridge_text"] = json!("  ");
    let blank = validate_graph_patch_for_review(&patch, &HashSet::new(), &active_edge_ids);
    assert!(!blank.valid);
    assert!(blank.errors.iter().any(|error| error["path"] == "$.proposed_edge_bridge_updates[0].bridge_text"));

    patch["proposed_edge_bridge_updates"][0]["bridge_text"] = json!("The revised bridge remains acceptance-ready.");
    let valid = validate_graph_patch_for_review(&patch, &HashSet::new(), &active_edge_ids);
    assert!(valid.valid, "{:?}", valid.errors);

    let update_schema = &graph_patch_schema()["properties"]["proposed_edge_bridge_updates"]["items"];
    assert!(update_schema["required"].as_array().unwrap().iter().any(|field| field == "bridge_text"));
    assert_eq!(update_schema["properties"]["bridge_text"]["pattern"], r"\S");
  }

  #[test]
  fn rejects_graph_patches_above_the_total_proposal_limit() {
    let mut patch = empty_graph_patch();
    patch["ambiguities"] = Value::Array(
      (0..=GRAPH_PATCH_MAX_PROPOSALS)
        .map(|index| json!({ "kind": "merge_risk", "prompt": format!("Question {index}") }))
        .collect(),
    );

    let result = validate_graph_patch_for_review(&patch, &HashSet::new(), &HashSet::new());

    assert!(!result.valid);
    assert!(result.errors.iter().any(|error| {
      error["path"] == "$" && error["message"].as_str().is_some_and(|message| message.contains("maximum is 200"))
    }));
    assert_eq!(graph_patch_schema()["properties"]["ambiguities"]["maxItems"], GRAPH_PATCH_MAX_PROPOSALS);
  }

  #[test]
  fn evidence_reference_bounds_accept_the_caps_and_match_the_schema() {
    let chunk_ids =
      (0..GRAPH_PATCH_MAX_EVIDENCE_REFERENCES).map(|index| Value::String(format!("chunk_{index}"))).collect::<Vec<_>>();
    let mut message_ids = (0..GRAPH_PATCH_MAX_EVIDENCE_REFERENCES)
      .map(|index| Value::String(format!("message_{index}")))
      .collect::<Vec<_>>();
    message_ids[0] = Value::String("x".repeat(GRAPH_PATCH_EVIDENCE_ID_MAX_CHARS));
    let mut patch = empty_graph_patch();
    patch["proposed_nodes"] = json!([{
      "temp_id": "bounded_evidence",
      "type": "concept",
      "title": "Bounded evidence",
      "compiled_body": "The maximum supported evidence reference arrays remain valid.",
      "source_chunk_ids": chunk_ids,
      "source_message_ids": message_ids
    }]);

    let result = validate_graph_patch_for_review(&patch, &HashSet::new(), &HashSet::new());
    assert!(result.valid, "{:?}", result.errors);

    let schema = graph_patch_schema();
    for proposal_field in [
      "proposed_nodes",
      "proposed_edges",
      "proposed_node_body_updates",
      "proposed_edge_bridge_updates",
      "proposed_paths",
      "ambiguities",
      "merge_candidates",
    ] {
      assert_eq!(
        schema["properties"][proposal_field]["items"]["properties"]["source_chunk_ids"]["maxItems"],
        GRAPH_PATCH_MAX_EVIDENCE_REFERENCES,
        "missing source chunk cap for {proposal_field}"
      );
      assert_eq!(
        schema["properties"][proposal_field]["items"]["properties"]["source_chunk_ids"]["items"]["maxLength"],
        GRAPH_PATCH_EVIDENCE_ID_MAX_CHARS,
        "missing source chunk id cap for {proposal_field}"
      );
      assert_eq!(
        schema["properties"][proposal_field]["items"]["properties"]["source_message_ids"]["maxItems"],
        GRAPH_PATCH_MAX_EVIDENCE_REFERENCES,
        "missing source message cap for {proposal_field}"
      );
      assert_eq!(
        schema["properties"][proposal_field]["items"]["properties"]["source_message_ids"]["items"]["maxLength"],
        GRAPH_PATCH_EVIDENCE_ID_MAX_CHARS,
        "missing source message id cap for {proposal_field}"
      );
    }
  }

  #[test]
  fn evidence_reference_bounds_reject_excessive_arrays_and_identifiers() {
    let mut patch = empty_graph_patch();
    patch["proposed_nodes"] = json!([{
      "temp_id": "excessive_evidence",
      "type": "concept",
      "title": "Excessive evidence",
      "compiled_body": "Unbounded evidence references must not reach persistence.",
      "source_message_ids": (0..=GRAPH_PATCH_MAX_EVIDENCE_REFERENCES)
        .map(|index| format!("message_{index}"))
        .collect::<Vec<_>>()
    }]);

    let too_many = validate_graph_patch_for_review(&patch, &HashSet::new(), &HashSet::new());
    assert!(!too_many.valid);
    assert!(too_many.errors.iter().any(|error| {
      error["path"] == "$.proposed_nodes[0].source_message_ids"
        && error["message"].as_str().is_some_and(|message| message.contains("must not exceed 200 items"))
    }));

    patch["proposed_nodes"][0]["source_message_ids"] = json!(["message_1"]);
    patch["proposed_nodes"][0]["source_chunk_ids"] =
      json!((0..=GRAPH_PATCH_MAX_EVIDENCE_REFERENCES).map(|index| format!("chunk_{index}")).collect::<Vec<_>>());
    let too_many_chunks = validate_graph_patch_for_review(&patch, &HashSet::new(), &HashSet::new());
    assert!(!too_many_chunks.valid);
    assert!(too_many_chunks.errors.iter().any(|error| {
      error["path"] == "$.proposed_nodes[0].source_chunk_ids"
        && error["message"].as_str().is_some_and(|message| message.contains("must not exceed 200 items"))
    }));

    patch["proposed_nodes"][0]["source_chunk_ids"] = json!([]);
    patch["proposed_nodes"][0]["source_message_ids"] = json!(["🧠".repeat(GRAPH_PATCH_EVIDENCE_ID_MAX_CHARS + 1)]);
    let overlong = validate_graph_patch_for_review(&patch, &HashSet::new(), &HashSet::new());
    assert!(!overlong.valid);
    assert!(overlong.errors.iter().any(|error| {
      error["path"] == "$.proposed_nodes[0].source_message_ids[0]"
        && error["message"].as_str().is_some_and(|message| message.contains("must not exceed 256 characters"))
    }));
  }

  #[test]
  fn evidence_reference_extraction_deduplicates_without_reordering() {
    let payload = json!({
      "source_chunk_ids": [" chunk_1 ", "chunk_2", "chunk_1"],
      "source_message_ids": ["message_1", " message_1 ", "message_2"]
    });

    assert_eq!(source_chunk_ids(&payload), ["chunk_1", "chunk_2"]);
    assert_eq!(source_message_ids(&payload), ["message_1", "message_2"]);
  }

  #[test]
  fn graph_patch_string_bounds_match_the_generated_schema() {
    let mut patch = empty_graph_patch();
    patch["proposed_nodes"] = json!([{
      "temp_id": "bounded_node",
      "type": "concept",
      "title": "t".repeat(NODE_TITLE_MAX_CHARS + 1),
      "preview": "p".repeat(NODE_PREVIEW_MAX_CHARS + 1),
      "compiled_body": "b".repeat(NODE_BODY_MAX_CHARS + 1),
      "source_message_ids": ["message_1"]
    }]);

    let result = validate_graph_patch_for_review(&patch, &HashSet::new(), &HashSet::new());

    for path in ["$.proposed_nodes[0].title", "$.proposed_nodes[0].preview", "$.proposed_nodes[0].compiled_body"] {
      assert!(result.errors.iter().any(|error| error["path"] == path), "missing bound error for {path}");
    }

    let mut active_node_ids = HashSet::new();
    active_node_ids.extend(["node_a".to_string(), "node_b".to_string()]);
    let mut edge_patch = empty_graph_patch();
    edge_patch["proposed_edges"] = json!([{
      "type": "supports",
      "source_node_id": "node_a",
      "target_node_id": "node_b",
      "bridge_text": "x".repeat(EDGE_BRIDGE_MAX_CHARS + 1),
      "reason": "Bound the persisted bridge.",
      "source_message_ids": ["message_1"]
    }]);
    let edge_result = validate_graph_patch_for_review(&edge_patch, &active_node_ids, &HashSet::new());
    assert!(edge_result.errors.iter().any(|error| error["path"] == "$.proposed_edges[0].bridge_text"));

    let schema = graph_patch_schema();
    assert_eq!(schema["properties"]["proposed_nodes"]["items"]["properties"]["title"]["maxLength"], 80);
    assert_eq!(
      schema["properties"]["proposed_nodes"]["items"]["properties"]["compiled_body"]["maxLength"],
      NODE_BODY_MAX_CHARS
    );
    assert_eq!(schema["properties"]["proposed_edges"]["items"]["properties"]["bridge_text"]["maxLength"], 240);
  }

  #[test]
  fn malformed_patch_items_reach_validation_without_panicking() {
    let patch = complete_graph_patch(&json!({
      "schema_version": 1,
      "proposed_nodes": [1]
    }));
    let patch = attach_source_message_id(patch, Some("message_1"));
    let result = validate_graph_patch_for_review(&patch, &HashSet::new(), &HashSet::new());

    assert!(!result.valid);
    assert!(result.errors.iter().any(|error| error["path"] == "$.proposed_nodes[0]"));
  }

  #[test]
  fn malformed_patch_shapes_survive_completion_for_validation() {
    let scalar = complete_graph_patch(&json!("not a patch"));
    assert_eq!(scalar, "not a patch");
    assert!(!graph_patch_is_empty(&scalar));
    let scalar_result = validate_graph_patch_for_review(&scalar, &HashSet::new(), &HashSet::new());
    assert!(scalar_result.errors.iter().any(|error| error["path"] == "$"));

    let wrong_field = complete_graph_patch(&json!({ "proposed_nodes": "not an array" }));
    assert_eq!(wrong_field["proposed_nodes"], "not an array");
    assert!(!graph_patch_is_empty(&wrong_field));
    let field_result = validate_graph_patch_for_review(&wrong_field, &HashSet::new(), &HashSet::new());
    assert!(field_result.errors.iter().any(|error| error["path"] == "$.proposed_nodes"));
  }

  #[test]
  fn chat_evidence_completion_includes_merge_candidates() {
    let mut active_node_ids = HashSet::new();
    active_node_ids.extend(["node_alpha".to_string(), "node_beta".to_string()]);
    let mut patch = empty_graph_patch();
    patch["merge_candidates"] = json!([{
      "candidate_node_ids": ["node_alpha", "node_beta"],
      "reason": "The current message identifies overlapping concepts."
    }]);

    let patch = attach_source_message_id(patch, Some("message_1"));
    assert_eq!(patch["merge_candidates"][0]["source_message_ids"][0], "message_1");
    assert!(validate_graph_patch_for_review(&patch, &active_node_ids, &HashSet::new()).valid);
  }

  #[test]
  fn node_body_update_contract_rejects_replace_section() {
    let mut active_node_ids = HashSet::new();
    active_node_ids.insert("node_existing".to_string());
    let mut patch = empty_graph_patch();
    patch["proposed_node_body_updates"] = json!([{
      "target_node_id": "node_existing",
      "update_kind": "replace_section",
      "section_id": "old-version:section:1",
      "section_text": "A replacement section.",
      "reason": "Legacy operation.",
      "source_message_ids": ["message_1"]
    }]);

    let result = validate_graph_patch_for_review(&patch, &active_node_ids, &HashSet::new());

    assert!(!result.valid);
    assert!(result.errors.iter().any(|error| {
      error["path"] == "$.proposed_node_body_updates[0].update_kind"
        && error["message"].as_str().is_some_and(|message| message.contains("Unsupported node body update kind"))
    }));
    assert_eq!(
      graph_patch_schema()["properties"]["proposed_node_body_updates"]["items"]["properties"]["update_kind"]["enum"],
      json!(["replace_body", "append_section"])
    );
  }

  #[test]
  fn mixed_existing_and_temporary_edge_endpoints_match_the_schema() {
    let mut active_node_ids = HashSet::new();
    active_node_ids.insert("node_existing".to_string());
    let patch = json!({
      "schema_version": 1,
      "proposed_nodes": [{
        "temp_id": "node_new",
        "type": "concept",
        "title": "New node",
        "compiled_body": "A source-backed compiled section.",
        "source_message_ids": ["message_1"]
      }],
      "proposed_edges": [{
        "type": "supports",
        "source_node_id": "node_existing",
        "target_temp_id": "node_new",
        "reason": "The existing node supports the new node.",
        "source_message_ids": ["message_1"]
      }, {
        "type": "depends_on",
        "source_temp_id": "node_new",
        "target_node_id": "node_existing",
        "reason": "The new node depends on the existing node.",
        "source_message_ids": ["message_1"]
      }],
      "proposed_node_body_updates": [],
      "proposed_edge_bridge_updates": [],
      "proposed_message_evidence_attachments": [],
      "proposed_paths": [],
      "ambiguities": [],
      "merge_candidates": [],
      "warnings": []
    });

    let result = validate_graph_patch_for_review(&patch, &active_node_ids, &HashSet::new());
    assert!(result.valid, "{:?}", result.errors);

    let edge_schema = &graph_patch_schema()["properties"]["proposed_edges"]["items"];
    assert_eq!(edge_schema["allOf"][0]["anyOf"][0]["required"], json!(["source_temp_id"]));
    assert_eq!(edge_schema["allOf"][0]["anyOf"][1]["required"], json!(["source_node_id"]));
    assert_eq!(edge_schema["allOf"][1]["anyOf"][0]["required"], json!(["target_temp_id"]));
    assert_eq!(edge_schema["allOf"][1]["anyOf"][1]["required"], json!(["target_node_id"]));
  }
}
