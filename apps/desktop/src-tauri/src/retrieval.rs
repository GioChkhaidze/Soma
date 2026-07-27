use std::collections::{HashMap, HashSet};

use serde_json::{json, Value};

use crate::error::{CommandError, CommandResult};

pub(crate) const GRAPH_CONTEXT_NODE_LIMIT: usize = 6;
const NODE_CONTEXT_EDGE_LIMIT: usize = 8;
const NODE_CONTEXT_NEIGHBOR_LIMIT: usize = 6;
pub(crate) const NODE_CONTEXT_NODE_LIMIT: usize = NODE_CONTEXT_NEIGHBOR_LIMIT + 1;

pub(crate) fn graph_search_terms(user_message: &str, reading_context: Option<&Value>) -> Vec<String> {
  tokenize(&graph_search_text(user_message, reading_context))
}

#[cfg(test)]
pub(crate) fn build_graph_context_packet(
  snapshot: &Value,
  user_message: &str,
  recent_messages: Vec<Value>,
  focus_node_ids: &[String],
) -> Value {
  build_graph_context_packet_with_reading_context(snapshot, user_message, recent_messages, focus_node_ids, None, true)
}

pub(crate) fn build_graph_context_packet_with_reading_context(
  snapshot: &Value,
  user_message: &str,
  recent_messages: Vec<Value>,
  focus_node_ids: &[String],
  reading_context: Option<&Value>,
  graph_capture_enabled: bool,
) -> Value {
  let reading_context = normalized_reading_context(reading_context);
  let search_text = graph_search_text(user_message, reading_context.as_ref());
  let nodes = active_snapshot_nodes(snapshot);
  let edges = active_snapshot_edges(snapshot, &nodes);
  let top_matches = top_matching_nodes(&nodes, &search_text, focus_node_ids, GRAPH_CONTEXT_NODE_LIMIT);
  let focus_nodes = focused_nodes(&nodes, focus_node_ids);
  let top_ids: HashSet<String> =
    top_matches.iter().filter_map(|node| node.get("id").and_then(Value::as_str).map(String::from)).collect();
  let relevant_edges: Vec<Value> = edges
    .iter()
    .filter(|edge| {
      edge.get("source_node_id").and_then(Value::as_str).is_some_and(|id| top_ids.contains(id))
        || edge.get("target_node_id").and_then(Value::as_str).is_some_and(|id| top_ids.contains(id))
    })
    .take(8)
    .cloned()
    .collect();
  let include_global_lists = !top_matches.is_empty() || graph_overview_requested(user_message);
  let unresolved_questions = if include_global_lists {
    nodes
      .iter()
      .filter(|node| node.get("type").and_then(Value::as_str) == Some("question"))
      .take(5)
      .map(node_card_ref)
      .collect::<Vec<_>>()
  } else {
    Vec::new()
  };
  let open_tasks = if include_global_lists {
    nodes
      .iter()
      .filter(|node| node.get("type").and_then(Value::as_str) == Some("task"))
      .take(5)
      .map(node_card_ref)
      .collect::<Vec<_>>()
  } else {
    Vec::new()
  };

  json!({
    "mode": "graph_chat",
    "user_message": user_message,
    "reading_context": reading_context,
    "graph_capture_enabled": graph_capture_enabled,
    "focus_node_ids": focus_nodes.iter().filter_map(|node| node.get("id").and_then(Value::as_str)).collect::<Vec<_>>(),
    "focus_set_node_bodies": focus_nodes.iter().map(node_body_ref).collect::<Vec<_>>(),
    "top_matching_nodes": top_matches.iter().map(|node| json!({
      "id": node["id"].clone(),
      "title": node["title"].clone(),
      "type": node["type"].clone(),
      "preview": node["preview"].clone(),
      "score": node["score"].clone()
    })).collect::<Vec<_>>(),
    "top_matching_node_bodies": top_matches.iter().map(node_body_ref).collect::<Vec<_>>(),
    "relevant_path_fragments": relevant_edges.iter().map(|edge| path_fragment(edge, &nodes)).collect::<Vec<_>>(),
    "unresolved_questions": unresolved_questions,
    "open_tasks": open_tasks,
    "recent_graph_thread_messages": recent_messages.into_iter().take(6).collect::<Vec<_>>(),
    "source_evidence_excerpts": evidence_excerpts(&top_matches, &relevant_edges, 10),
    "used_graph_areas": top_matches.iter().map(|node| json!({
      "id": node["id"].clone(),
      "title": node["title"].clone(),
      "type": node["type"].clone()
    })).collect::<Vec<_>>()
  })
}

fn graph_search_text(user_message: &str, reading_context: Option<&Value>) -> String {
  let mut search_text = user_message.to_string();
  let Some(context) = normalized_reading_context(reading_context) else {
    return search_text;
  };
  let selected = context.get("selected_text").and_then(Value::as_str).unwrap_or("");
  let page_text = context.get("page_text").and_then(Value::as_str).unwrap_or("");
  let grounding = if selected.is_empty() { page_text } else { selected };
  if !grounding.is_empty() {
    search_text.push(' ');
    search_text.push_str(&truncate_chars(grounding, 2_000));
  }
  search_text
}

fn normalized_reading_context(value: Option<&Value>) -> Option<Value> {
  let value = value?;
  let object = value.as_object()?;
  const FIELDS: [&str; 7] =
    ["kind", "document_name", "page_number", "page_count", "page_text", "selected_text", "selection_page_number"];
  if object.keys().any(|field| !FIELDS.contains(&field.as_str())) {
    return None;
  }
  if value.get("kind").and_then(Value::as_str) != Some("pdf") {
    return None;
  }
  let document_name =
    value.get("document_name").and_then(Value::as_str).map(str::trim).filter(|value| !value.is_empty())?;
  let page_number = value.get("page_number").and_then(Value::as_u64)?;
  let page_count = value.get("page_count").and_then(Value::as_u64)?;
  let page_text = value.get("page_text").and_then(Value::as_str)?;
  if page_number == 0 || page_count == 0 || page_number > page_count {
    return None;
  }
  let selected_text = match value.get("selected_text") {
    None | Some(Value::Null) => None,
    Some(Value::String(selected_text)) => {
      let selected_text = selected_text.trim();
      (!selected_text.is_empty()).then(|| truncate_chars(selected_text, 6_000))
    }
    Some(_) => return None,
  };
  let selection_page_number = match value.get("selection_page_number") {
    None | Some(Value::Null) => None,
    Some(Value::Number(page)) => {
      let page = page.as_u64()?;
      if page == 0 || page > page_count {
        return None;
      }
      Some(page)
    }
    Some(_) => return None,
  };
  let mut context = json!({
    "kind": "pdf",
    "document_name": truncate_chars(document_name, 256),
    "page_number": page_number,
    "page_count": page_count,
    "page_text": truncate_chars(page_text, 12_000)
  });
  if let Some(selected_text) = selected_text {
    context["selected_text"] = json!(selected_text);
    if let Some(selection_page_number) = selection_page_number {
      context["selection_page_number"] = json!(selection_page_number);
    }
  }
  Some(context)
}

fn truncate_chars(value: &str, max_chars: usize) -> String {
  value.chars().take(max_chars).collect()
}

fn top_matching_nodes(nodes: &[Value], user_message: &str, focus_node_ids: &[String], limit: usize) -> Vec<Value> {
  let mut top_matches = focused_nodes(nodes, focus_node_ids);
  let mut seen_ids: HashSet<String> =
    top_matches.iter().filter_map(|node| node.get("id").and_then(Value::as_str).map(String::from)).collect();
  for node in search_nodes(nodes, user_message, limit) {
    let Some(node_id) = node.get("id").and_then(Value::as_str) else {
      continue;
    };
    if seen_ids.insert(node_id.to_string()) {
      top_matches.push(node);
    }
    if top_matches.len() >= limit {
      break;
    }
  }
  top_matches
}

pub(crate) fn build_node_context_packet(
  snapshot: &Value,
  node_id: &str,
  user_message: &str,
  recent_messages: Vec<Value>,
  graph_capture_enabled: bool,
) -> CommandResult<Value> {
  let nodes = active_snapshot_nodes(snapshot);
  let focused_node = nodes
    .iter()
    .find(|node| node.get("id").and_then(Value::as_str) == Some(node_id))
    .cloned()
    .ok_or_else(|| CommandError::not_found(format!("Active node not found: {node_id}")))?;
  let edges = active_snapshot_edges(snapshot, &nodes);
  let local_edges: Vec<Value> = edges
    .into_iter()
    .filter(|edge| {
      edge.get("source_node_id").and_then(Value::as_str) == Some(node_id)
        || edge.get("target_node_id").and_then(Value::as_str) == Some(node_id)
    })
    .take(NODE_CONTEXT_EDGE_LIMIT)
    .collect();
  let neighbor_nodes = local_edges
    .iter()
    .filter_map(|edge| {
      let source_id = edge.get("source_node_id").and_then(Value::as_str)?;
      let target_id = edge.get("target_node_id").and_then(Value::as_str)?;
      let neighbor_id = if source_id == node_id { target_id } else { source_id };
      let neighbor = nodes.iter().find(|node| node.get("id").and_then(Value::as_str) == Some(neighbor_id))?;
      Some((edge.clone(), neighbor.clone()))
    })
    .take(NODE_CONTEXT_NEIGHBOR_LIMIT)
    .collect::<Vec<_>>();
  let neighbor_bodies = neighbor_nodes
    .iter()
    .map(|(edge, neighbor)| {
      let mut body = node_body_ref(neighbor);
      body["via_edge_id"] = edge.get("id").cloned().unwrap_or(Value::Null);
      body
    })
    .collect::<Vec<_>>();
  let mut evidence_nodes = Vec::with_capacity(1 + neighbor_nodes.len());
  evidence_nodes.push(focused_node.clone());
  evidence_nodes.extend(neighbor_nodes.iter().map(|(_, node)| node.clone()));

  Ok(json!({
    "mode": "node_chat",
    "focused_node_id": node_id,
    "user_message": user_message,
    "graph_capture_enabled": graph_capture_enabled,
    "focused_node_body": node_body_ref(&focused_node),
    "neighbor_bodies": neighbor_bodies,
    "bridge_texts": local_edges.iter().map(edge_ref).collect::<Vec<_>>(),
    "node_thread_recent_messages": recent_messages.into_iter().take(6).collect::<Vec<_>>(),
    "source_evidence_excerpts": evidence_excerpts(&evidence_nodes, &local_edges, 8)
  }))
}

fn graph_overview_requested(user_message: &str) -> bool {
  tokenize(user_message).iter().any(|term| {
    matches!(
      term.as_str(),
      "task" | "tasks" | "todo" | "todos" | "question" | "questions" | "unresolved" | "open" | "next" | "review"
    )
  })
}

fn focused_nodes(nodes: &[Value], focus_node_ids: &[String]) -> Vec<Value> {
  let mut seen = HashSet::new();
  let mut focused = Vec::new();
  for node_id in focus_node_ids {
    if !seen.insert(node_id.clone()) {
      continue;
    }
    if let Some(node) = nodes.iter().find(|node| node.get("id").and_then(Value::as_str) == Some(node_id.as_str())) {
      let mut node = node.clone();
      node["score"] = json!(i64::MAX);
      focused.push(node);
    }
  }
  focused
}

fn search_nodes(nodes: &[Value], query: &str, limit: usize) -> Vec<Value> {
  let terms = tokenize(query);
  let mut ranked: Vec<Value> = nodes
    .iter()
    .map(|node| {
      let mut node = node.clone();
      node["score"] = json!(score_node(&node, &terms));
      node
    })
    .filter(|node| terms.is_empty() || node.get("score").and_then(Value::as_i64).unwrap_or(0) > 0)
    .collect();
  ranked.sort_by(|a, b| {
    b.get("score")
      .and_then(Value::as_i64)
      .cmp(&a.get("score").and_then(Value::as_i64))
      .then_with(|| a.get("title").and_then(Value::as_str).cmp(&b.get("title").and_then(Value::as_str)))
  });
  ranked.truncate(limit);
  ranked
}

fn score_node(node: &Value, terms: &[String]) -> i64 {
  if terms.is_empty() {
    return 1;
  }
  let fields = [
    (node.get("title").and_then(Value::as_str).unwrap_or(""), 8),
    (node.get("type").and_then(Value::as_str).unwrap_or(""), 4),
    (node.get("preview").and_then(Value::as_str).unwrap_or(""), 3),
    (node.get("compiled_body").and_then(Value::as_str).unwrap_or(""), 1),
  ];
  let mut score = 0;
  for term in terms {
    for (value, weight) in fields {
      score += occurrences(&value.to_lowercase(), term) * weight;
    }
  }
  score
}

fn active_snapshot_nodes(snapshot: &Value) -> Vec<Value> {
  snapshot
    .get("nodes")
    .and_then(Value::as_array)
    .into_iter()
    .flatten()
    .filter(|node| node.get("id").is_some() && node.get("status").and_then(Value::as_str) == Some("active"))
    .cloned()
    .collect()
}

fn active_snapshot_edges(snapshot: &Value, nodes: &[Value]) -> Vec<Value> {
  let ids: HashSet<String> =
    nodes.iter().filter_map(|node| node.get("id").and_then(Value::as_str).map(String::from)).collect();
  snapshot
    .get("edges")
    .and_then(Value::as_array)
    .into_iter()
    .flatten()
    .filter(|edge| {
      let carries_endpoint_titles = edge.get("source_title").is_some() && edge.get("target_title").is_some();
      edge.get("id").is_some()
        && edge.get("status").and_then(Value::as_str) == Some("active")
        && (carries_endpoint_titles
          || edge.get("source_node_id").and_then(Value::as_str).is_some_and(|id| ids.contains(id)))
        && (carries_endpoint_titles
          || edge.get("target_node_id").and_then(Value::as_str).is_some_and(|id| ids.contains(id)))
    })
    .cloned()
    .collect()
}

fn node_body_ref(node: &Value) -> Value {
  json!({
    "id": node["id"].clone(),
    "title": node["title"].clone(),
    "type": node["type"].clone(),
    "preview": node["preview"].clone(),
    "compiled_body": node["compiled_body"].clone(),
    "body_version": node["body_version"].clone(),
    "body_version_id": node["body_version_id"].clone(),
    "source_chunk_ids": node.get("source_chunk_ids").cloned().unwrap_or_else(|| json!([]))
  })
}

fn node_card_ref(node: &Value) -> Value {
  json!({
    "id": node["id"].clone(),
    "title": node["title"].clone(),
    "type": node["type"].clone(),
    "preview": node["preview"].clone()
  })
}

fn path_fragment(edge: &Value, nodes: &[Value]) -> Value {
  let source_id = edge.get("source_node_id").and_then(Value::as_str).unwrap_or("");
  let target_id = edge.get("target_node_id").and_then(Value::as_str).unwrap_or("");
  let title_by_id: HashMap<String, String> = nodes
    .iter()
    .filter_map(|node| Some((node.get("id")?.as_str()?.to_string(), node.get("title")?.as_str()?.to_string())))
    .collect();
  let source_title = edge
    .get("source_title")
    .and_then(Value::as_str)
    .map(String::from)
    .or_else(|| title_by_id.get(source_id).cloned())
    .unwrap_or_else(|| source_id.to_string());
  let target_title = edge
    .get("target_title")
    .and_then(Value::as_str)
    .map(String::from)
    .or_else(|| title_by_id.get(target_id).cloned())
    .unwrap_or_else(|| target_id.to_string());
  json!({
    "edge_id": edge["id"].clone(),
    "source_node_id": source_id,
    "source_title": source_title,
    "target_node_id": target_id,
    "target_title": target_title,
    "type": edge["type"].clone(),
    "bridge_text": edge.get("bridge_text").and_then(Value::as_str).unwrap_or(""),
    "updated_at": edge["updated_at"].clone()
  })
}

fn edge_ref(edge: &Value) -> Value {
  json!({
    "edge_id": edge["id"].clone(),
    "source_node_id": edge["source_node_id"].clone(),
    "target_node_id": edge["target_node_id"].clone(),
    "type": edge["type"].clone(),
    "bridge_text": edge.get("bridge_text").and_then(Value::as_str).unwrap_or(""),
    "updated_at": edge["updated_at"].clone()
  })
}

fn evidence_excerpts(nodes: &[Value], edges: &[Value], limit: usize) -> Vec<Value> {
  let mut seen = HashSet::new();
  let mut excerpts = Vec::new();
  for item in nodes.iter().chain(edges.iter()) {
    for evidence in item.get("evidence").and_then(Value::as_array).into_iter().flatten() {
      let key = evidence.get("id").or_else(|| evidence.get("chunk_id")).and_then(Value::as_str).unwrap_or("");
      if key.is_empty() || !seen.insert(key.to_string()) {
        continue;
      }
      excerpts.push(json!({
        "id": evidence["id"].clone(),
        "chunk_id": evidence["chunk_id"].clone(),
        "excerpt": evidence
          .get("excerpt")
          .or_else(|| evidence.get("quote_excerpt"))
          .cloned()
          .unwrap_or_else(|| json!("")),
        "source_title": evidence.pointer("/source/title").cloned().unwrap_or(Value::Null),
        "conversation_title": evidence.pointer("/conversation/title").cloned().unwrap_or(Value::Null),
        "message_role": evidence.pointer("/message/role").cloned().unwrap_or(Value::Null),
        "entity_id": item["id"].clone(),
        "entity_title": item.get("title").cloned().unwrap_or(Value::Null)
      }));
      if excerpts.len() >= limit {
        return excerpts;
      }
    }
  }
  excerpts
}

fn tokenize(value: &str) -> Vec<String> {
  value
    .to_lowercase()
    .split(|ch: char| !ch.is_alphanumeric() && ch != '_')
    .filter(|term| !term.is_empty() && !SEARCH_STOP_WORDS.contains(term))
    .map(String::from)
    .collect()
}

fn occurrences(value: &str, term: &str) -> i64 {
  if value.is_empty() || term.is_empty() {
    return 0;
  }
  tokenize(value).iter().filter(|value_term| value_term.as_str() == term).count() as i64
}

const SEARCH_STOP_WORDS: &[&str] = &[
  "a", "an", "and", "are", "as", "at", "be", "but", "by", "can", "do", "does", "for", "from", "how", "i", "if", "in",
  "is", "it", "me", "my", "of", "on", "or", "should", "that", "the", "this", "to", "up", "what", "when", "where",
  "which", "who", "why", "with", "you", "your",
];

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn tokenize_preserves_non_ascii_search_terms() {
    assert_eq!(tokenize("Graph الشبكات Граф"), vec!["graph", "الشبكات", "граф"]);
  }

  #[test]
  fn graph_chat_context_ignores_generic_off_topic_question_words() {
    let snapshot = json!({
      "schema_version": 1,
      "nodes": [{
        "id": "node_arc_bottleneck",
        "type": "task",
        "title": "ARC Patch Bottleneck",
        "status": "active",
        "preview": "This is the task to improve an ARC-AGI patch loop.",
        "compiled_body": "This is the current plan to raise a score with graph metrics and verifier work.",
        "source_chunk_ids": [],
        "evidence": []
      }],
      "edges": [],
      "paths": []
    });

    let packet = build_graph_context_packet(&snapshot, "What is the best time to wake up?", Vec::new(), &[]);

    assert!(packet["top_matching_nodes"].as_array().unwrap().is_empty());
    assert!(packet["used_graph_areas"].as_array().unwrap().is_empty());
    assert!(packet["open_tasks"].as_array().unwrap().is_empty());
    assert!(packet["unresolved_questions"].as_array().unwrap().is_empty());
  }

  #[test]
  fn source_reading_context_matches_shared_canonical_cases() {
    let fixture: Value =
      serde_json::from_str(include_str!("../../../../test/source-reading-context-cases.json")).unwrap();
    for test_case in fixture["cases"].as_array().unwrap() {
      let expected = match &test_case["canonical"] {
        Value::Null => None,
        canonical => Some(canonical.clone()),
      };
      assert_eq!(normalized_reading_context(test_case.get("input")), expected, "{}", test_case["name"]);
    }

    let test_case = &fixture["truncation_case"];
    let character = test_case["character"].as_str().unwrap();
    let mut input = test_case["input"].clone();
    let mut expected = input.clone();
    for (field, max_characters) in fixture["bounds"].as_object().unwrap() {
      let max_characters = max_characters.as_u64().unwrap() as usize;
      input[field.as_str()] = json!(character.repeat(max_characters + 1));
      expected[field.as_str()] = json!(character.repeat(max_characters));
    }
    assert_eq!(normalized_reading_context(Some(&input)), Some(expected), "{}", test_case["name"]);
  }
}
