use serde_json::Value;

use crate::error::{is_storage_busy_message, CommandResult, RuntimeFailureKind, STORAGE_BUSY_MESSAGE};

const CHAT_PROMPT_MAX_BYTES: usize = 32_000;
const CHAT_RESPONSE_MAX_CHARS: usize = 180_000;
const CHAT_CONTEXT_TRUNCATION_NOTICE: &str = "\n[Context truncated. Use the most relevant context above.]";

#[derive(Debug)]
pub struct RuntimeChatTurnResult {
  pub adapter_kind: String,
  pub status: &'static str,
  pub failure_kind: Option<RuntimeFailureKind>,
  pub message: String,
  pub assistant_message: Option<String>,
  pub used_graph_areas: Vec<Value>,
  pub proposed_graph_patch: Option<Value>,
}

pub(crate) fn chat_turn_prompt(request: &Value) -> String {
  let user_message = current_chat_user_message(request).trim();
  let mode = request.get("mode").and_then(Value::as_str).unwrap_or("graph_chat");
  let context_packet = request.get("context_packet").cloned().unwrap_or(Value::Null);
  let capture_graph_changes = request.get("capture_graph_changes").and_then(Value::as_bool).unwrap_or(false);
  let mut prompt = String::from(
    "You are Soma's workspace chat runtime. Do not introduce yourself as Codex, ChatGPT, a model, or a coding agent.\n",
  );
  prompt.push_str("Answer the current user message directly and only use graph context when it helps.\n\n");
  prompt.push_str("CURRENT_USER_MESSAGE:\n");
  prompt.push_str(if user_message.is_empty() { "[empty]" } else { user_message });
  prompt.push_str("\n\nMODE:\n");
  prompt.push_str(mode);
  prompt.push_str("\n\n");
  prompt.push_str("Return exactly one JSON object with this shape:\n");
  prompt.push_str(concat!(
    r#"{"assistant_message":"Direct answer to CURRENT_USER_MESSAGE.","used_graph_areas":[],"#,
    r#""proposed_graph_patch":{"schema_version":1,"proposed_nodes":[],"proposed_edges":[],"#,
    r#""proposed_node_body_updates":[],"proposed_edge_bridge_updates":[],"#,
    r#""proposed_message_evidence_attachments":[],"proposed_paths":[],"ambiguities":[],"#,
    r#""merge_candidates":[],"warnings":[]}}"#,
  ));
  prompt.push_str("\n\nRules:\n");
  prompt.push_str("- assistant_message must answer CURRENT_USER_MESSAGE, not a previous recent message.\n");
  prompt.push_str("- Never make assistant_message a self-introduction such as \"I am Codex\".\n");
  prompt.push_str(
    "- If asked what you are, answer as Soma's workspace chat runtime, not as Codex, ChatGPT, a model, or an agent.\n",
  );
  prompt.push_str("- Use graph context only when it directly helps answer the current user_message.\n");
  if capture_graph_changes {
    prompt.push_str("- If the current message has no durable graph update, return proposed_graph_patch as null.\n");
    prompt.push_str(concat!(
      "- If the current message introduces a durable concept, claim, decision, question, ",
      "task, artifact, or useful connection, include a small proposed_graph_patch.\n",
    ));
  } else {
    prompt.push_str(concat!(
      "- Graph capture is off for this turn. Return proposed_graph_patch as null and answer ",
      "without changing the graph.\n",
    ));
  }
  prompt.push_str(concat!(
    "- Split multi-concept messages into separate proposed_nodes instead of one catch-all node; ",
    "usually 2-5 nodes is enough.\n",
  ));
  prompt.push_str(concat!(
    "- For a new node include temp_id, type, title, preview, compiled_body. Keep titles 2-6 words ",
    "and compiled_body concise.\n",
  ));
  prompt.push_str(
    "- Connect new and existing nodes with explicit proposed_edges when the current message relates ideas.\n",
  );
  prompt.push_str(concat!(
    "- For an edge include source_node_id/source_temp_id, target_node_id/target_temp_id, type, ",
    "reason, and short bridge_text.\n",
  ));
  prompt.push_str(concat!(
    "- Choose the narrowest true edge type from part_of, supports, contradicts, depends_on, ",
    "answers, implements, mentions, derived_from, alternative_to, blocks, next_step, mitigates; ",
    "use mentions only for co-mentioned ideas when no stronger relation is explicit.\n",
  ));
  prompt.push_str("- You may omit source_message_ids; Soma will attach this chat message as evidence.\n");
  prompt.push_str("- Return JSON only. Do not wrap it in markdown.\n\n");
  append_bounded_prompt_section(
    &mut prompt,
    "context_packet.json",
    &serde_json::to_string_pretty(&context_packet).unwrap_or_else(|_| context_packet.to_string()),
  );
  prompt
}

pub(crate) fn current_chat_user_message(request: &Value) -> &str {
  request.pointer("/context_packet/user_message").and_then(Value::as_str).unwrap_or("")
}

pub(crate) fn parse_chat_turn_response(
  adapter_kind: &str,
  content: &str,
  current_user_message: &str,
) -> CommandResult<RuntimeChatTurnResult> {
  let Some(value) = extract_chat_turn_json(content) else {
    if is_storage_busy_message(content) {
      return Ok(RuntimeChatTurnResult {
        adapter_kind: adapter_kind.to_string(),
        status: "failed",
        failure_kind: Some(RuntimeFailureKind::Busy),
        message: STORAGE_BUSY_MESSAGE.to_string(),
        assistant_message: None,
        used_graph_areas: Vec::new(),
        proposed_graph_patch: None,
      });
    }
    return Ok(RuntimeChatTurnResult {
      adapter_kind: adapter_kind.to_string(),
      status: "failed",
      failure_kind: Some(RuntimeFailureKind::InvalidResponse),
      message: "Chat runtime did not return valid Soma chat JSON. No graph updates were imported.".to_string(),
      assistant_message: None,
      used_graph_areas: Vec::new(),
      proposed_graph_patch: None,
    });
  };
  let Some(assistant_message) = value
    .get("assistant_message")
    .or_else(|| value.get("assistantMessage"))
    .and_then(Value::as_str)
    .map(str::trim)
    .filter(|message| !message.is_empty())
    .map(str::to_string)
  else {
    return Ok(RuntimeChatTurnResult {
      adapter_kind: adapter_kind.to_string(),
      status: "failed",
      failure_kind: Some(RuntimeFailureKind::InvalidResponse),
      message: "Chat runtime JSON did not include assistant_message. No graph updates were imported.".to_string(),
      assistant_message: None,
      used_graph_areas: Vec::new(),
      proposed_graph_patch: None,
    });
  };
  if is_storage_busy_message(&assistant_message) {
    return Ok(RuntimeChatTurnResult {
      adapter_kind: adapter_kind.to_string(),
      status: "failed",
      failure_kind: Some(RuntimeFailureKind::Busy),
      message: STORAGE_BUSY_MESSAGE.to_string(),
      assistant_message: None,
      used_graph_areas: Vec::new(),
      proposed_graph_patch: None,
    });
  }
  if is_runtime_error_answer(&assistant_message) {
    return Ok(RuntimeChatTurnResult {
      adapter_kind: adapter_kind.to_string(),
      status: "failed",
      failure_kind: Some(RuntimeFailureKind::InvalidResponse),
      message: concat!(
        "Chat runtime returned an execution error instead of answering the current user message. ",
        "No graph updates were imported.",
      )
      .to_string(),
      assistant_message: None,
      used_graph_areas: Vec::new(),
      proposed_graph_patch: None,
    });
  }
  if is_codex_identity_answer(&assistant_message, current_user_message) {
    return Ok(RuntimeChatTurnResult {
      adapter_kind: adapter_kind.to_string(),
      status: "failed",
      failure_kind: Some(RuntimeFailureKind::InvalidResponse),
      message:
        "Chat runtime answered as Codex instead of answering the current user message. No graph updates were imported."
          .to_string(),
      assistant_message: None,
      used_graph_areas: Vec::new(),
      proposed_graph_patch: None,
    });
  }

  let used_graph_areas = value
    .get("used_graph_areas")
    .or_else(|| value.get("usedGraphAreas"))
    .and_then(Value::as_array)
    .cloned()
    .unwrap_or_default();
  let proposed_graph_patch = value
    .get("proposed_graph_patch")
    .or_else(|| value.get("proposedGraphPatch"))
    .or_else(|| value.get("graph_patch"))
    .or_else(|| value.get("graphPatch"))
    .filter(|patch| !patch.is_null())
    .cloned();

  Ok(RuntimeChatTurnResult {
    adapter_kind: adapter_kind.to_string(),
    status: "completed",
    failure_kind: None,
    message: "Chat runtime returned an assistant answer.".to_string(),
    assistant_message: Some(assistant_message),
    used_graph_areas,
    proposed_graph_patch,
  })
}

fn append_bounded_prompt_section(prompt: &mut String, file_name: &str, content: &str) {
  let header = format!("\n--- {file_name} ---\n");
  let minimum_section_bytes = header.len() + 1;
  let Some(available_content_bytes) =
    CHAT_PROMPT_MAX_BYTES.checked_sub(prompt.len().saturating_add(minimum_section_bytes))
  else {
    return;
  };

  prompt.push_str(&header);
  if content.len() <= available_content_bytes {
    prompt.push_str(content);
  } else {
    let mut bounded_context = content.to_string();
    let context_byte_limit = available_content_bytes.saturating_sub(CHAT_CONTEXT_TRUNCATION_NOTICE.len());
    truncate_utf8_to_byte_limit(&mut bounded_context, context_byte_limit);
    prompt.push_str(&bounded_context);
    if CHAT_CONTEXT_TRUNCATION_NOTICE.len() <= available_content_bytes {
      prompt.push_str(CHAT_CONTEXT_TRUNCATION_NOTICE);
    }
  }
  prompt.push('\n');
}

fn truncate_utf8_to_byte_limit(value: &mut String, max_bytes: usize) -> bool {
  if value.len() <= max_bytes {
    return false;
  }
  let mut boundary = max_bytes;
  while !value.is_char_boundary(boundary) {
    boundary -= 1;
  }
  value.truncate(boundary);
  true
}

fn is_codex_identity_answer(message: &str, current_user_message: &str) -> bool {
  let words = ascii_words(message);
  if words.len() < 3 || words.len() > 90 {
    return false;
  }
  let is_identity_answer = contains_first_person_runtime_intro(&words)
    || contains_named_runtime_intro(&words)
    || starts_with_runtime_role(&words);
  if asks_about_codex(current_user_message) && !is_identity_answer {
    return false;
  }
  is_identity_answer
}

fn is_runtime_error_answer(message: &str) -> bool {
  let message = message.trim().to_ascii_lowercase();
  message.starts_with("runtime command exited")
    || message.starts_with("runtime command failed")
    || message.starts_with("could not start runtime")
    || message.starts_with("error: runtime command")
    || message.starts_with("exit status")
    || message.contains("runtime command exited with status")
    || (message.contains("stderr") && message.contains("exit"))
}

fn contains_first_person_runtime_intro(words: &[String]) -> bool {
  words.iter().take(8).enumerate().any(|(index, word)| {
    if word == "im" {
      return contains_runtime_identity(&words[index + 1..words.len().min(index + 8)]);
    }
    if word == "i" && words.get(index + 1).is_some_and(|next| next == "am" || next == "m") {
      return contains_runtime_identity(&words[index + 2..words.len().min(index + 9)]);
    }
    false
  })
}

fn starts_with_runtime_role(words: &[String]) -> bool {
  if words.first().is_none_or(|word| word != "as") {
    return false;
  }
  contains_runtime_identity(&words[1..words.len().min(7)])
}

fn contains_named_runtime_intro(words: &[String]) -> bool {
  words.iter().take(8).enumerate().any(|(index, word)| {
    if word == "my"
      && words.get(index + 1).is_some_and(|next| next == "name")
      && words.get(index + 2).is_some_and(|next| next == "is")
    {
      return contains_runtime_identity(&words[index + 3..words.len().min(index + 9)]);
    }
    if word == "this" && words.get(index + 1).is_some_and(|next| next == "is") {
      return contains_runtime_identity(&words[index + 2..words.len().min(index + 8)]);
    }
    false
  })
}

fn contains_runtime_identity(words: &[String]) -> bool {
  words.iter().any(|word| word == "codex" || word == "chatgpt")
    || words.windows(2).any(|window| {
      matches!(
          window,
          [first, second]
              if ((first == "ai" || first == "language") && second == "model")
                  || (first == "coding" && second == "agent")
                  || (first == "ai" && second == "assistant")
      )
    })
}

fn asks_about_codex(value: &str) -> bool {
  let words = ascii_words(value);
  words.iter().any(|word| word == "codex")
}

fn ascii_words(value: &str) -> Vec<String> {
  let mut normalized = String::with_capacity(value.len());
  for ch in value.chars() {
    if ch.is_ascii_alphanumeric() {
      normalized.push(ch.to_ascii_lowercase());
    } else {
      normalized.push(' ');
    }
  }
  normalized.split_whitespace().map(str::to_string).collect()
}

fn extract_chat_turn_json(content: &str) -> Option<Value> {
  if let Ok(value) = serde_json::from_str::<Value>(content.trim()) {
    if is_chat_turn_like(&value) {
      return Some(value);
    }
  }

  let fenced = content
    .split("```")
    .find_map(|part| {
      let trimmed = part.trim().trim_start_matches("json").trim();
      serde_json::from_str::<Value>(trimmed).ok()
    })
    .filter(is_chat_turn_like);
  if fenced.is_some() {
    return fenced;
  }

  let start = content.find('{')?;
  let end = content.rfind('}')?;
  if end <= start || end - start > CHAT_RESPONSE_MAX_CHARS {
    return None;
  }
  serde_json::from_str::<Value>(&content[start..=end]).ok().filter(is_chat_turn_like)
}

fn is_chat_turn_like(value: &Value) -> bool {
  value.is_object() && (value.get("assistant_message").is_some() || value.get("assistantMessage").is_some())
}

#[cfg(test)]
mod tests {
  use super::*;
  use serde_json::json;

  #[test]
  fn chat_turn_prompt_keeps_current_message_above_context() {
    let prompt = chat_turn_prompt(&json!({
      "schema_version": 1,
      "mode": "graph_chat",
      "capture_graph_changes": true,
      "context_packet": {
        "user_message": "What is the best time to wake up?",
        "recent_graph_thread_messages": [{
          "role": "assistant",
          "content": "Focus on the ARC patch bottleneck."
        }]
      },
      "graph_patch_schema": {
        "large": "schema should not be sent to direct chat"
      }
    }));

    assert!(prompt.contains("CURRENT_USER_MESSAGE:\nWhat is the best time to wake up?"));
    assert!(prompt.contains("assistant_message must answer CURRENT_USER_MESSAGE"));
    assert!(prompt.contains("If the current message has no durable graph update"));
    assert!(prompt.contains("include a small proposed_graph_patch"));
    assert!(prompt.contains("Split multi-concept messages into separate proposed_nodes"));
    assert!(prompt.contains("Connect new and existing nodes with explicit proposed_edges"));
    assert!(prompt.contains("use mentions only for co-mentioned ideas"));
    assert!(prompt.contains("type, reason, and short bridge_text"));
    assert!(prompt.contains("Do not introduce yourself as Codex"));
    assert!(prompt.contains("answer as Soma's workspace chat runtime"));
    assert!(!prompt.contains("graph_patch_schema"));
    assert!(prompt.find("What is the best time to wake up?").unwrap() < prompt.find("context_packet.json").unwrap());
  }

  #[test]
  fn chat_turn_prompt_defaults_omitted_capture_off() {
    let prompt = chat_turn_prompt(&json!({
      "schema_version": 1,
      "mode": "graph_chat",
      "context_packet": {
        "user_message": "Answer without changing graph truth."
      }
    }));

    assert!(prompt.contains("Graph capture is off for this turn."));
    assert!(!prompt.contains("include a small proposed_graph_patch"));
  }

  #[test]
  fn chat_turn_prompt_truncates_multibyte_context_safely() {
    let prompt = chat_turn_prompt(&json!({
      "mode": "graph_chat",
      "context_packet": {
        "user_message": "Explain this page.",
        "reading_context": {
          "kind": "pdf",
          "page_text": "界".repeat(CHAT_PROMPT_MAX_BYTES)
        }
      }
    }));

    assert!(prompt.contains("[Context truncated."));
    assert!(prompt.is_char_boundary(prompt.len()));
    assert!(prompt.len() <= CHAT_PROMPT_MAX_BYTES);
  }

  #[test]
  fn chat_turn_prompt_preserves_the_complete_bounded_user_message_and_rules() {
    let suffix = "CURRENT_MESSAGE_END";
    let user_message =
      format!("{}{suffix}", "🧠".repeat(crate::repository::CHAT_MESSAGE_MAX_CHARACTERS - suffix.chars().count()));
    let prompt = chat_turn_prompt(&json!({
      "mode": "graph_chat",
      "capture_graph_changes": true,
      "context_packet": {
        "user_message": user_message,
        "reading_context": {
          "kind": "pdf",
          "page_text": "界".repeat(CHAT_PROMPT_MAX_BYTES)
        }
      }
    }));

    assert!(prompt.contains(&format!("CURRENT_USER_MESSAGE:\n{user_message}\n\nMODE:")));
    assert!(prompt.contains("- If the current message has no durable graph update"));
    assert!(prompt.contains("- Return JSON only. Do not wrap it in markdown."));
    assert!(prompt.find(suffix).unwrap() < prompt.find("- Return JSON only.").unwrap());
    assert!(prompt.contains("[Context truncated."));
    assert!(prompt.len() <= CHAT_PROMPT_MAX_BYTES);
  }

  #[test]
  fn utf8_truncation_moves_to_the_previous_character_boundary() {
    let mut value = "abc界".to_string();

    assert!(truncate_utf8_to_byte_limit(&mut value, 5));
    assert_eq!(value, "abc");
  }

  #[test]
  fn chat_turn_response_rejects_storage_lock_as_assistant_answer() {
    let result = parse_chat_turn_response(
      "codex_sdk_profile",
      r#"{
              "assistant_message": "Runtime command exited with status 1. Error: database is locked",
              "used_graph_areas": [{"id": "node_1", "title": "Node", "type": "concept"}],
              "proposed_graph_patch": {
                "schema_version": 1,
                "proposed_nodes": [{
                  "temp_id": "node_bad",
                  "type": "concept",
                  "title": "Bad Lock",
                  "compiled_body": "This must not be imported."
                }]
              }
            }"#,
      "Create a graph node.",
    )
    .unwrap();

    assert_eq!(result.status, "failed");
    assert!(result.assistant_message.is_none());
    assert!(result.proposed_graph_patch.is_none());
    assert_eq!(result.message, STORAGE_BUSY_MESSAGE);
  }

  #[test]
  fn chat_turn_response_rejects_raw_storage_lock_output() {
    let result = parse_chat_turn_response(
      "codex_sdk_profile",
      "Runtime command exited with status 1. Error: database is locked",
      "Create a graph node.",
    )
    .unwrap();

    assert_eq!(result.status, "failed");
    assert!(result.assistant_message.is_none());
    assert!(result.proposed_graph_patch.is_none());
    assert_eq!(result.message, STORAGE_BUSY_MESSAGE);
  }

  #[test]
  fn chat_turn_response_rejects_runtime_command_error_as_assistant_answer() {
    let result = parse_chat_turn_response(
      "codex_sdk_profile",
      r#"{
              "assistant_message": "Runtime command exited with status 1. stderr: failed before answering",
              "used_graph_areas": [],
              "proposed_graph_patch": {
                "schema_version": 1,
                "proposed_nodes": [{
                  "temp_id": "node_bad",
                  "type": "concept",
                  "title": "Bad Runtime",
                  "compiled_body": "This must not be imported."
                }]
              }
            }"#,
      "Create a graph node.",
    )
    .unwrap();

    assert_eq!(result.status, "failed");
    assert!(result.assistant_message.is_none());
    assert!(result.proposed_graph_patch.is_none());
    assert!(result.message.contains("execution error"));
  }

  #[test]
  fn chat_turn_response_rejects_codex_identity_answer() {
    let result = parse_chat_turn_response(
      "codex_sdk_profile",
      r#"{
              "assistant_message": "I am Codex, OpenAI's coding agent.",
              "used_graph_areas": [],
              "proposed_graph_patch": null
            }"#,
      "How do I make retrieval faster?",
    )
    .unwrap();

    assert_eq!(result.status, "failed");
    assert!(result.assistant_message.is_none());
    assert!(result.message.contains("answered as Codex"));
  }

  #[test]
  fn chat_turn_response_rejects_prefixed_codex_identity_answer() {
    let result = parse_chat_turn_response(
      "codex_sdk_profile",
      r#"{
              "assistant_message": "Hello, I am Codex, OpenAI's coding agent. How can I help?",
              "used_graph_areas": [],
              "proposed_graph_patch": null
            }"#,
      "Why is graph chat slow?",
    )
    .unwrap();

    assert_eq!(result.status, "failed");
    assert!(result.assistant_message.is_none());
    assert!(result.message.contains("answered as Codex"));
  }

  #[test]
  fn chat_turn_response_rejects_openai_codex_identity_variant() {
    let result = parse_chat_turn_response(
      "codex_sdk_profile",
      r#"{
              "assistant_message": "I'm OpenAI's Codex, a coding agent.",
              "used_graph_areas": [],
              "proposed_graph_patch": null
            }"#,
      "Why is graph chat slow?",
    )
    .unwrap();

    assert_eq!(result.status, "failed");
    assert!(result.assistant_message.is_none());
    assert!(result.message.contains("answered as Codex"));
  }

  #[test]
  fn chat_turn_response_rejects_named_codex_identity_variants() {
    for assistant_message in
      ["My name is Codex, and I can help with your workspace.", "This is Codex. I can help with that."]
    {
      let content = json!({
        "assistant_message": assistant_message,
        "used_graph_areas": [],
        "proposed_graph_patch": null
      })
      .to_string();
      let result = parse_chat_turn_response("codex_sdk_profile", &content, "Why is graph chat slow?").unwrap();

      assert_eq!(result.status, "failed");
      assert!(result.assistant_message.is_none());
      assert!(result.message.contains("answered as Codex"));
    }
  }

  #[test]
  fn chat_turn_response_rejects_chatgpt_or_model_identity_boilerplate() {
    for assistant_message in [
      "I am ChatGPT, an AI language model.",
      "I am an AI model trained to help.",
      "As an AI assistant, I can help with that.",
    ] {
      let content = json!({
        "assistant_message": assistant_message,
        "used_graph_areas": [],
        "proposed_graph_patch": null
      })
      .to_string();
      let result = parse_chat_turn_response("codex_sdk_profile", &content, "Explain the graph.").unwrap();

      assert_eq!(result.status, "failed");
      assert!(result.assistant_message.is_none());
    }
  }

  #[test]
  fn chat_turn_response_accepts_graph_patch_alias() {
    let result = parse_chat_turn_response(
      "codex_sdk_profile",
      r#"{
              "assistant_message": "I created a graph update for the investigation.",
              "usedGraphAreas": [],
              "graphPatch": {
                "schema_version": 1,
                "proposedNodes": [{
                  "temp_id": "node_alias",
                  "type": "question",
                  "title": "Alias Patch",
                  "compiled_body": "The runtime used a camelCase patch alias.",
                  "source_message_ids": ["message_1"]
                }]
              }
            }"#,
      "Investigate the alias parser.",
    )
    .unwrap();

    assert_eq!(result.status, "completed");
    assert_eq!(result.proposed_graph_patch.unwrap()["proposedNodes"][0]["title"], "Alias Patch");
  }

  #[test]
  fn chat_turn_response_allows_codex_topic_answers() {
    let content = json!({
      "assistant_message": concat!(
        "Codex is useful for code changes, but this workspace should keep direct chat ",
        "answer-focused.",
      ),
      "used_graph_areas": [],
      "proposed_graph_patch": null
    })
    .to_string();
    let result = parse_chat_turn_response("codex_sdk_profile", &content, "Should I use Codex for code edits?").unwrap();

    assert_eq!(result.status, "completed");
    assert!(result.assistant_message.as_deref().unwrap().contains("useful for code changes"));
  }
}
