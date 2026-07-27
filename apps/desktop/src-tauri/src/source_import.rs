use std::collections::HashSet;
use std::fs;
use std::io::Read;
use std::path::Path;

use rusqlite::{params, Connection};
use serde_json::{json, Map, Value};
use time::format_description::well_known::Rfc3339;
use time::OffsetDateTime;
use uuid::Uuid;

use crate::database::{open_existing_database, with_write_transaction};
use crate::error::{CommandError, CommandResult};
use crate::workspace::{WorkspacePaths, RAW_IMPORT_DIR};

const CHUNK_MAX_CHARS: usize = 1600;
// Bound text and JSON allocations while still allowing substantial conversation exports.
const SOURCE_IMPORT_MAX_BYTES: u64 = 64 * 1024 * 1024;

#[derive(Debug)]
struct ParsedConversation {
  provider: String,
  title: String,
  created_at: Option<String>,
  messages: Vec<ParsedMessage>,
}

#[derive(Debug)]
struct ParsedMessage {
  role: String,
  content: String,
  created_at: Option<String>,
}

pub(crate) struct JobChunkSelection {
  pub(crate) chunks: Vec<Value>,
  pub(crate) total_count: i64,
}

pub fn import_source_file(paths: &WorkspacePaths, source_path: impl AsRef<Path>) -> CommandResult<Value> {
  import_source_file_before_archive(paths, source_path, |_| {})
}

fn import_source_file_before_archive(
  paths: &WorkspacePaths,
  source_path: impl AsRef<Path>,
  before_archive: impl FnOnce(&Path),
) -> CommandResult<Value> {
  let source_path = source_path.as_ref();
  let absolute_source_path = source_path.canonicalize().unwrap_or_else(|_| source_path.to_path_buf());
  let extension = absolute_source_path
    .extension()
    .and_then(|value| value.to_str())
    .map(|value| format!(".{}", value.to_lowercase()))
    .unwrap_or_default();
  let source_type = source_type_for_extension(&extension)?;
  let fallback_title = absolute_source_path.file_name().and_then(|value| value.to_str()).unwrap_or("Imported source");
  let source_id = new_id();
  let imported_at = now_string()?;
  before_archive(&absolute_source_path);
  let raw_path = copy_raw_source(&paths.workspace_dir, &source_id, &absolute_source_path)?;
  let import_result = read_source_text(&raw_path)
    .and_then(|content| parse_source(&content, &extension, fallback_title))
    .and_then(|conversations| {
      open_existing_database(&paths.database_path).and_then(|conn| {
        with_write_transaction(&conn, |conn| {
          insert_imported_source(
            conn,
            &source_id,
            source_type,
            conversations,
            &absolute_source_path,
            &raw_path,
            &imported_at,
          )
        })
      })
    });

  match import_result {
    Ok(value) => Ok(value),
    Err(mut error) => {
      if let Err(cleanup_error) = fs::remove_file(&raw_path) {
        error.message = format!("{} Raw source cleanup also failed: {cleanup_error}", error.message);
      }
      Err(error)
    }
  }
}

#[cfg(test)]
fn search_chunks(paths: &WorkspacePaths, query: &str, limit: i64) -> CommandResult<Value> {
  let fts_query = build_fts_query(query)?;
  let conn = open_existing_database(&paths.database_path)?;
  let mut stmt = conn.prepare(
    r#"
    SELECT
      chunks.id,
      chunks.content,
      chunks.chunk_index,
      chunks.token_count,
      messages.id,
      messages.role,
      messages.order_index,
      conversations.id,
      conversations.title,
      sources.id,
      sources.title,
      sources.original_path,
      sources.raw_path
    FROM chunks_fts
    JOIN chunks ON chunks_fts.chunk_id = chunks.id
    JOIN messages ON chunks.message_id = messages.id
    JOIN conversations ON messages.conversation_id = conversations.id
    JOIN sources ON conversations.source_id = sources.id
    WHERE chunks_fts MATCH ?1
    ORDER BY rank
    LIMIT ?2
    "#,
  )?;
  let rows = stmt.query_map(params![fts_query, limit], |row| {
    Ok(json!({
      "chunk_id": row.get::<_, String>(0)?,
      "content": row.get::<_, String>(1)?,
      "chunk_index": row.get::<_, i64>(2)?,
      "token_count": row.get::<_, i64>(3)?,
      "message_id": row.get::<_, String>(4)?,
      "role": row.get::<_, String>(5)?,
      "order_index": row.get::<_, i64>(6)?,
      "conversation_id": row.get::<_, String>(7)?,
      "conversation_title": row.get::<_, String>(8)?,
      "source_id": row.get::<_, String>(9)?,
      "source_title": row.get::<_, String>(10)?,
      "original_path": row.get::<_, String>(11)?,
      "raw_path": row.get::<_, String>(12)?
    }))
  })?;
  Ok(Value::Array(rows.collect::<Result<Vec<_>, _>>()?))
}

pub fn workspace_stats(paths: &WorkspacePaths) -> CommandResult<Value> {
  let conn = open_existing_database(&paths.database_path)?;
  Ok(json!({
    "sources": count_rows(&conn, "sources")?,
    "conversations": count_rows(&conn, "conversations")?,
    "messages": count_rows(&conn, "messages")?,
    "chunks": count_rows(&conn, "chunks")?,
    "ftsRows": count_rows(&conn, "chunks_fts")?
  }))
}

pub(crate) fn select_chunks_for_job(conn: &Connection, limit: i64) -> CommandResult<JobChunkSelection> {
  let mut stmt = conn.prepare(
    r#"
    SELECT
      chunks.id,
      chunks.content,
      chunks.chunk_index,
      chunks.token_count,
      messages.id,
      messages.role,
      messages.order_index,
      conversations.id,
      conversations.title,
      sources.id,
      sources.title,
      COUNT(*) OVER () AS total_chunk_count
    FROM chunks
    JOIN messages ON chunks.message_id = messages.id
    JOIN conversations ON messages.conversation_id = conversations.id
    JOIN sources ON conversations.source_id = sources.id
    ORDER BY
      sources.imported_at,
      sources.id,
      conversations.created_at,
      conversations.id,
      messages.order_index,
      chunks.chunk_index
    LIMIT ?1
    "#,
  )?;
  let rows = stmt.query_map(params![limit], |row| {
    Ok((
      json!({
        "chunk_id": row.get::<_, String>(0)?,
        "content": row.get::<_, String>(1)?,
        "chunk_index": row.get::<_, i64>(2)?,
        "token_count": row.get::<_, i64>(3)?,
        "message_id": row.get::<_, String>(4)?,
        "role": row.get::<_, String>(5)?,
        "order_index": row.get::<_, i64>(6)?,
        "conversation_id": row.get::<_, String>(7)?,
        "conversation_title": row.get::<_, String>(8)?,
        "source_id": row.get::<_, String>(9)?,
        "source_title": row.get::<_, String>(10)?
      }),
      row.get::<_, i64>(11)?,
    ))
  })?;
  let rows = rows.collect::<Result<Vec<_>, _>>()?;
  Ok(JobChunkSelection {
    total_count: rows.first().map(|row| row.1).unwrap_or(0),
    chunks: rows.into_iter().map(|row| row.0).collect(),
  })
}

fn insert_imported_source(
  conn: &Connection,
  source_id: &str,
  source_type: &str,
  conversations: Vec<ParsedConversation>,
  original_path: &Path,
  raw_path: &Path,
  imported_at: &str,
) -> CommandResult<Value> {
  let title = conversations.first().map(|conversation| conversation.title.as_str()).unwrap_or("Imported source");
  conn.execute(
    concat!(
      "INSERT INTO sources (id, source_type, title, original_path, raw_path, imported_at) ",
      "VALUES (?1, ?2, ?3, ?4, ?5, ?6)"
    ),
    params![source_id, source_type, title, original_path.to_string_lossy(), raw_path.to_string_lossy(), imported_at],
  )?;

  let mut inserted_conversations = Vec::new();
  let mut message_count = 0;
  let mut chunk_count = 0;
  for conversation in conversations {
    let conversation_id = new_id();
    let provider = conversation.provider;
    let title = conversation.title;
    let created_at = conversation.created_at;
    let messages = conversation.messages;
    inserted_conversations.push(json!({
      "id": conversation_id.clone(),
      "title": title.clone(),
      "messageCount": messages.len()
    }));
    conn.execute(
      "INSERT INTO conversations (id, source_id, provider, title, created_at) VALUES (?1, ?2, ?3, ?4, ?5)",
      params![&conversation_id, source_id, &provider, &title, created_at.as_deref()],
    )?;

    for (message_index, message) in messages.into_iter().enumerate() {
      let message_id = new_id();
      let role = message.role;
      let content = message.content;
      let created_at = message.created_at;
      message_count += 1;
      conn.execute(
        concat!(
          "INSERT INTO messages (id, conversation_id, role, content, order_index, created_at) ",
          "VALUES (?1, ?2, ?3, ?4, ?5, ?6)"
        ),
        params![&message_id, &conversation_id, &role, &content, message_index as i64, created_at.as_deref()],
      )?;
      for (chunk_index, chunk) in chunk_message(&content).into_iter().enumerate() {
        let chunk_id = new_id();
        chunk_count += 1;
        conn.execute(
          "INSERT INTO chunks (id, message_id, content, chunk_index, token_count) VALUES (?1, ?2, ?3, ?4, ?5)",
          params![&chunk_id, &message_id, &chunk, chunk_index as i64, count_tokens(&chunk) as i64],
        )?;
        conn.execute("INSERT INTO chunks_fts (content, chunk_id) VALUES (?1, ?2)", params![&chunk, &chunk_id])?;
      }
    }
  }

  Ok(json!({
    "sourceId": source_id,
    "rawPath": raw_path.to_string_lossy(),
    "conversations": inserted_conversations,
    "messageCount": message_count,
    "chunkCount": chunk_count
  }))
}

fn copy_raw_source(workspace_dir: &Path, source_id: &str, source_path: &Path) -> CommandResult<std::path::PathBuf> {
  ensure_source_within_import_limit(source_path)?;
  let safe_name = source_path
    .file_name()
    .and_then(|value| value.to_str())
    .unwrap_or("source.txt")
    .chars()
    .map(|ch| if ch.is_ascii_alphanumeric() || matches!(ch, '.' | '_' | '-') { ch } else { '_' })
    .collect::<String>();
  let raw_path = workspace_dir.join(RAW_IMPORT_DIR).join(format!("{source_id}-{safe_name}"));
  let mut source = fs::File::open(source_path)?.take(SOURCE_IMPORT_MAX_BYTES + 1);
  let mut raw_file = fs::OpenOptions::new().write(true).create_new(true).open(&raw_path)?;
  let copy_result = match std::io::copy(&mut source, &mut raw_file) {
    Ok(copied_bytes) if copied_bytes > SOURCE_IMPORT_MAX_BYTES => {
      Err(CommandError::validation("Source file exceeds the 64 MiB import limit."))
    }
    Ok(_) => Ok(()),
    Err(error) => Err(error.into()),
  };
  if let Err(mut error) = copy_result {
    drop(raw_file);
    if let Err(cleanup_error) = fs::remove_file(&raw_path) {
      error.message = format!("{} Partial raw-copy cleanup also failed: {cleanup_error}", error.message);
    }
    return Err(error);
  }
  Ok(raw_path)
}

fn read_source_text(source_path: &Path) -> CommandResult<String> {
  ensure_source_within_import_limit(source_path)?;
  let mut source = fs::File::open(source_path)?.take(SOURCE_IMPORT_MAX_BYTES + 1);
  let mut content = String::new();
  source.read_to_string(&mut content)?;
  if content.len() as u64 > SOURCE_IMPORT_MAX_BYTES {
    return Err(CommandError::validation("Source file exceeds the 64 MiB import limit."));
  }
  Ok(content)
}

fn ensure_source_within_import_limit(source_path: &Path) -> CommandResult<()> {
  if fs::metadata(source_path)?.len() > SOURCE_IMPORT_MAX_BYTES {
    return Err(CommandError::validation("Source file exceeds the 64 MiB import limit."));
  }
  Ok(())
}

fn parse_source(content: &str, extension: &str, fallback_title: &str) -> CommandResult<Vec<ParsedConversation>> {
  match extension {
    ".json" => parse_json_source(content, fallback_title),
    ".md" | ".markdown" => Ok(vec![parse_marked_text_conversation(content, fallback_title, true)?]),
    ".txt" => Ok(vec![parse_marked_text_conversation(content, fallback_title, false)?]),
    _ => Err(CommandError::validation(format!("Unsupported source extension: {extension}"))),
  }
}

fn source_type_for_extension(extension: &str) -> CommandResult<&'static str> {
  match extension {
    ".json" => Ok("json"),
    ".md" | ".markdown" => Ok("markdown"),
    ".txt" => Ok("text"),
    _ => Err(CommandError::validation(format!("Unsupported source extension: {extension}"))),
  }
}

fn parse_marked_text_conversation(
  content: &str,
  fallback_title: &str,
  use_markdown_title: bool,
) -> CommandResult<ParsedConversation> {
  let title = if use_markdown_title {
    extract_markdown_title(content).unwrap_or_else(|| fallback_title.to_string())
  } else {
    fallback_title.to_string()
  };
  let mut messages = Vec::new();
  let mut current_role: Option<String> = None;
  let mut current_lines: Vec<String> = Vec::new();

  for line in content.lines() {
    if let Some((role, initial_content)) = parse_role_marker(line)? {
      flush_message(&mut messages, &mut current_role, &mut current_lines);
      current_role = Some(role);
      current_lines = if initial_content.is_empty() { Vec::new() } else { vec![initial_content] };
      continue;
    }
    if current_role.is_some() {
      current_lines.push(line.to_string());
    }
  }
  flush_message(&mut messages, &mut current_role, &mut current_lines);
  if messages.is_empty() && !content.trim().is_empty() {
    messages.push(ParsedMessage { role: "user".to_string(), content: content.trim().to_string(), created_at: None });
  }

  Ok(ParsedConversation { provider: "manual".to_string(), title, created_at: None, messages })
}

fn parse_json_source(content: &str, fallback_title: &str) -> CommandResult<Vec<ParsedConversation>> {
  let parsed: Value =
    serde_json::from_str(content).map_err(|error| CommandError::validation(format!("Invalid JSON source: {error}")))?;
  if let Some(items) = parsed.as_array() {
    if items.iter().all(is_chatgpt_conversation) {
      return items
        .iter()
        .enumerate()
        .map(|(index, conversation)| chatgpt_conversation(conversation, &format!("{fallback_title} {}", index + 1)))
        .collect();
    }
  }
  if is_chatgpt_conversation(&parsed) {
    return Ok(vec![chatgpt_conversation(&parsed, fallback_title)?]);
  }
  if parsed.is_array() {
    return Ok(vec![json_conversation(&json!({ "title": fallback_title, "messages": parsed }), fallback_title)?]);
  }
  if let Some(conversations) = parsed.get("conversations").and_then(Value::as_array) {
    return conversations
      .iter()
      .enumerate()
      .map(|(index, conversation)| {
        if is_chatgpt_conversation(conversation) {
          chatgpt_conversation(conversation, &format!("{fallback_title} {}", index + 1))
        } else {
          json_conversation(conversation, &format!("{fallback_title} {}", index + 1))
        }
      })
      .collect();
  }
  if parsed.get("messages").and_then(Value::as_array).is_some() {
    let title = parsed.get("title").and_then(Value::as_str).unwrap_or(fallback_title);
    return Ok(vec![json_conversation(&parsed, title)?]);
  }
  Err(CommandError::validation(
    "JSON source must be an array of messages, an object with messages, or an object with conversations.",
  ))
}

fn is_chatgpt_conversation(value: &Value) -> bool {
  value.get("mapping").is_some_and(Value::is_object)
}

fn chatgpt_conversation(value: &Value, fallback_title: &str) -> CommandResult<ParsedConversation> {
  let mapping = value
    .get("mapping")
    .and_then(Value::as_object)
    .ok_or_else(|| CommandError::validation("ChatGPT export conversation is missing mapping."))?;
  let mut messages = Vec::new();

  for node in chatgpt_active_path(value, mapping)? {
    if let Some(message) = node.get("message") {
      if let Some(parsed) = chatgpt_message(message)? {
        messages.push(parsed);
      }
    }
  }

  if messages.is_empty() {
    return Err(CommandError::validation("ChatGPT export conversation has no importable text messages."));
  }

  Ok(ParsedConversation {
    provider: "chatgpt".to_string(),
    title: value.get("title").and_then(Value::as_str).unwrap_or(fallback_title).to_string(),
    created_at: unix_seconds_to_iso(value.get("create_time")),
    messages,
  })
}

fn chatgpt_active_path<'a>(conversation: &Value, mapping: &'a Map<String, Value>) -> CommandResult<Vec<&'a Value>> {
  let current_node_id = conversation
    .get("current_node")
    .and_then(Value::as_str)
    .filter(|id| !id.trim().is_empty())
    .map(str::to_string)
    .or_else(|| chatgpt_fallback_leaf_id(mapping))
    .ok_or_else(|| CommandError::validation("ChatGPT export conversation has no active message path."))?;
  let mut path = Vec::new();
  let mut seen = HashSet::new();
  let mut node_id = current_node_id;

  loop {
    if !seen.insert(node_id.clone()) {
      return Err(CommandError::validation("ChatGPT export conversation contains a cycle in its active path."));
    }
    let node = mapping.get(&node_id).ok_or_else(|| {
      CommandError::validation(format!("ChatGPT export active path references missing node: {node_id}."))
    })?;
    path.push(node);
    match node.get("parent") {
      None | Some(Value::Null) => break,
      Some(Value::String(parent_id)) if !parent_id.trim().is_empty() => node_id = parent_id.clone(),
      _ => return Err(CommandError::validation("ChatGPT export active path contains an invalid parent reference.")),
    }
  }

  path.reverse();
  Ok(path)
}

fn chatgpt_fallback_leaf_id(mapping: &Map<String, Value>) -> Option<String> {
  let parent_ids =
    mapping.values().filter_map(|node| node.get("parent").and_then(Value::as_str)).collect::<HashSet<_>>();
  mapping
    .iter()
    .filter(|(id, _)| !parent_ids.contains(id.as_str()))
    .max_by(|(left_id, left), (right_id, right)| {
      chatgpt_node_time(left).total_cmp(&chatgpt_node_time(right)).then_with(|| left_id.cmp(right_id))
    })
    .map(|(id, _)| id.clone())
}

fn chatgpt_node_time(node: &Value) -> f64 {
  node
    .get("message")
    .and_then(|message| message.get("create_time"))
    .and_then(Value::as_f64)
    .unwrap_or(f64::NEG_INFINITY)
}

fn chatgpt_message(value: &Value) -> CommandResult<Option<ParsedMessage>> {
  let role = match value.get("author").and_then(|author| author.get("role")).and_then(Value::as_str) {
    Some(role) => match normalize_role(role) {
      Ok(role) => role,
      Err(_) => return Ok(None),
    },
    None => return Ok(None),
  };

  let content = chatgpt_message_content(value.get("content")).trim().to_string();
  if content.is_empty() {
    return Ok(None);
  }

  Ok(Some(ParsedMessage { role, content, created_at: unix_seconds_to_iso(value.get("create_time")) }))
}

fn chatgpt_message_content(value: Option<&Value>) -> String {
  let Some(value) = value else {
    return String::new();
  };
  if let Some(parts) = value.get("parts").and_then(Value::as_array) {
    let mut content = String::new();
    for part in parts.iter().filter_map(chatgpt_part_text) {
      if !content.is_empty() {
        content.push_str("\n\n");
      }
      content.push_str(&part);
    }
    return content;
  }
  value.get("text").or_else(|| value.get("result")).and_then(Value::as_str).unwrap_or("").to_string()
}

fn chatgpt_part_text(value: &Value) -> Option<String> {
  if let Some(text) = value.as_str() {
    return Some(text.to_string());
  }
  value.get("text").or_else(|| value.get("content")).and_then(Value::as_str).map(String::from)
}

fn json_conversation(value: &Value, fallback_title: &str) -> CommandResult<ParsedConversation> {
  let messages = value
    .get("messages")
    .and_then(Value::as_array)
    .ok_or_else(|| CommandError::validation("JSON conversation is missing a messages array."))?;
  Ok(ParsedConversation {
    provider: value.get("provider").and_then(Value::as_str).unwrap_or("manual").to_string(),
    title: value.get("title").and_then(Value::as_str).unwrap_or(fallback_title).to_string(),
    created_at: value.get("created_at").or_else(|| value.get("createdAt")).and_then(Value::as_str).map(String::from),
    messages: messages.iter().enumerate().map(json_message).collect::<CommandResult<Vec<_>>>()?,
  })
}

fn json_message((index, value): (usize, &Value)) -> CommandResult<ParsedMessage> {
  let content = value
    .get("content")
    .or_else(|| value.get("text"))
    .and_then(Value::as_str)
    .ok_or_else(|| CommandError::validation(format!("JSON message {index} is missing non-empty content.")))?
    .trim()
    .to_string();
  if content.is_empty() {
    return Err(CommandError::validation(format!("JSON message {index} is missing non-empty content.")));
  }
  Ok(ParsedMessage {
    role: normalize_role(value.get("role").and_then(Value::as_str).unwrap_or("user"))?,
    content,
    created_at: value.get("created_at").or_else(|| value.get("createdAt")).and_then(Value::as_str).map(String::from),
  })
}

fn parse_role_marker(line: &str) -> CommandResult<Option<(String, String)>> {
  let trimmed = line.trim();
  let without_hash = trimmed.trim_start_matches('#').trim();
  if trimmed.starts_with('#') {
    let role = without_hash.trim_end_matches(':').trim();
    if is_role_label(role) {
      return Ok(Some((normalize_role(role)?, String::new())));
    }
  }
  if let Some((role, content)) = trimmed.split_once(':') {
    if is_role_label(role.trim()) {
      return Ok(Some((normalize_role(role.trim())?, content.trim().to_string())));
    }
  }
  Ok(None)
}

fn flush_message(
  messages: &mut Vec<ParsedMessage>,
  current_role: &mut Option<String>,
  current_lines: &mut Vec<String>,
) {
  let text = current_lines.join("\n").trim().to_string();
  if let Some(role) = current_role.take() {
    if !text.is_empty() {
      messages.push(ParsedMessage { role, content: text, created_at: None });
    }
  }
  current_lines.clear();
}

fn normalize_role(role: &str) -> CommandResult<String> {
  match role.to_lowercase().as_str() {
    "human" => Ok("user".to_string()),
    "ai" => Ok("assistant".to_string()),
    "user" | "assistant" | "system" | "tool" => Ok(role.to_lowercase()),
    _ => Err(CommandError::validation(format!("Unsupported message role: {role}"))),
  }
}

fn unix_seconds_to_iso(value: Option<&Value>) -> Option<String> {
  let seconds = value.and_then(Value::as_f64)?;
  OffsetDateTime::from_unix_timestamp(seconds as i64).ok().and_then(|time| time.format(&Rfc3339).ok())
}

fn is_role_label(value: &str) -> bool {
  matches!(value.to_lowercase().as_str(), "user" | "assistant" | "system" | "tool" | "human" | "ai")
}

fn extract_markdown_title(content: &str) -> Option<String> {
  content
    .lines()
    .find_map(|line| line.strip_prefix("# ").map(str::trim).filter(|title| !title.is_empty()).map(String::from))
}

fn chunk_message(content: &str) -> Vec<String> {
  let mut chunks = Vec::new();
  let mut current = String::new();
  let trimmed = content.trim();
  let mut found_paragraph = false;

  for paragraph in trimmed.split("\n\n").map(str::trim).filter(|part| !part.is_empty()) {
    found_paragraph = true;
    append_chunk_paragraph(&mut chunks, &mut current, paragraph);
  }
  if !found_paragraph && !trimmed.is_empty() {
    append_chunk_paragraph(&mut chunks, &mut current, trimmed);
  }
  if !current.is_empty() {
    chunks.push(current);
  }
  chunks
}

fn append_chunk_paragraph(chunks: &mut Vec<String>, current: &mut String, paragraph: &str) {
  let paragraph_chars = paragraph.chars().count();
  if paragraph_chars > CHUNK_MAX_CHARS {
    if !current.is_empty() {
      chunks.push(std::mem::take(current));
    }
    chunks.extend(split_long_text(paragraph));
    return;
  }

  let separator_len = if current.is_empty() { 0 } else { 2 };
  if current.chars().count() + separator_len + paragraph_chars > CHUNK_MAX_CHARS && !current.is_empty() {
    chunks.push(std::mem::take(current));
  }
  if !current.is_empty() {
    current.push_str("\n\n");
  }
  current.push_str(paragraph);
}

fn split_long_text(text: &str) -> Vec<String> {
  let mut chunks = Vec::new();
  let mut remaining = text.trim();
  while let Some(hard_boundary) = char_boundary_after(remaining, CHUNK_MAX_CHARS) {
    let soft_boundary = char_boundary_after(remaining, CHUNK_MAX_CHARS * 6 / 10).unwrap_or_default();
    let boundary = remaining[..hard_boundary].rfind(' ').unwrap_or(hard_boundary);
    let split_at = if boundary > soft_boundary { boundary } else { hard_boundary };
    chunks.push(remaining[..split_at].trim().to_string());
    remaining = remaining[split_at..].trim();
  }
  if !remaining.is_empty() {
    chunks.push(remaining.to_string());
  }
  chunks
}

fn char_boundary_after(value: &str, char_count: usize) -> Option<usize> {
  value.char_indices().nth(char_count).map(|(index, _)| index)
}

#[cfg(test)]
fn build_fts_query(query: &str) -> CommandResult<String> {
  let terms = query
    .to_lowercase()
    .split(|ch: char| !ch.is_ascii_alphanumeric() && ch != '_')
    .filter(|term| !term.is_empty())
    .map(|term| format!("\"{}\"", term.replace('"', "\"\"")))
    .collect::<Vec<_>>();
  if terms.is_empty() {
    return Err(CommandError::validation("Search query must contain at least one alphanumeric term."));
  }
  Ok(terms.join(" "))
}

fn count_rows(conn: &Connection, table: &str) -> CommandResult<i64> {
  conn.query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| row.get(0)).map_err(Into::into)
}

fn count_tokens(text: &str) -> usize {
  text.split_whitespace().count()
}

fn now_string() -> CommandResult<String> {
  Ok(OffsetDateTime::now_utc().format(&Rfc3339)?)
}

fn new_id() -> String {
  Uuid::new_v4().to_string()
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::workspace::create_workspace_dir;

  #[test]
  fn imports_markdown_text_and_json_into_searchable_chunks() {
    let root = std::env::temp_dir().join(format!("soma-import-test-{}", new_id()));
    let paths = create_workspace_dir(&root).unwrap();
    let md = root.join("sample.md");
    let txt = root.join("sample.txt");
    let json_file = root.join("sample.json");
    fs::write(&md, "# Import Notes\n\nUser: Semantic memory matters.\n\nAssistant: The graph keeps evidence.").unwrap();
    fs::write(&txt, "Human: Connectedness slider reduces overload.\n\nAI: It keeps dense graphs readable.").unwrap();
    fs::write(
      &json_file,
      concat!(
        r#"{"title":"JSON Chat","messages":[{"role":"user","#,
        r#""content":"Job folders preserve provenance."},{"role":"assistant","#,
        r#""content":"Output patches stay reviewable."}]}"#
      ),
    )
    .unwrap();

    import_source_file(&paths, &md).unwrap();
    import_source_file(&paths, &txt).unwrap();
    import_source_file(&paths, &json_file).unwrap();

    let stats = workspace_stats(&paths).unwrap();
    assert_eq!(stats["sources"], 3);
    assert_eq!(stats["conversations"], 3);
    assert_eq!(stats["messages"], 6);
    assert_eq!(stats["chunks"], 6);
    assert_eq!(stats["ftsRows"], 6);

    assert_eq!(search_chunks(&paths, "evidence", 10).unwrap().as_array().unwrap().len(), 1);
    assert_eq!(search_chunks(&paths, "connectedness", 10).unwrap().as_array().unwrap().len(), 1);
    assert_eq!(search_chunks(&paths, "provenance", 10).unwrap().as_array().unwrap().len(), 1);
  }

  #[test]
  fn imports_crlf_text_without_rewriting_raw_source() {
    let root = std::env::temp_dir().join(format!("soma-crlf-import-test-{}", new_id()));
    let paths = create_workspace_dir(&root).unwrap();
    let txt = root.join("windows-chat.txt");
    let raw_content = b"Human: First line\r\nsecond line\r\n\r\nAI: Answer line\r\n";
    fs::write(&txt, raw_content).unwrap();

    let imported = import_source_file(&paths, &txt).unwrap();
    let raw_path = imported["rawPath"].as_str().unwrap();
    let raw_copy = fs::read(raw_path).unwrap();
    let stats = workspace_stats(&paths).unwrap();

    assert_eq!(raw_copy, raw_content);
    assert_eq!(stats["messages"], 2);
    assert_eq!(search_chunks(&paths, "second", 10).unwrap().as_array().unwrap().len(), 1);
  }

  #[test]
  fn parses_the_exact_raw_copy_when_the_source_changes_before_archival() {
    let root = std::env::temp_dir().join(format!("soma-source-provenance-test-{}", new_id()));
    let paths = create_workspace_dir(&root).unwrap();
    let source = root.join("changing-source.txt");
    fs::write(&source, "Human: Original source bytes.").unwrap();

    let imported = import_source_file_before_archive(&paths, &source, |path| {
      fs::write(path, "Human: Archived source bytes.").unwrap();
    })
    .unwrap();
    let raw_path = imported["rawPath"].as_str().unwrap();

    assert_eq!(fs::read_to_string(raw_path).unwrap(), "Human: Archived source bytes.");
    assert_eq!(search_chunks(&paths, "archived", 10).unwrap().as_array().unwrap().len(), 1);
    assert!(search_chunks(&paths, "original", 10).unwrap().as_array().unwrap().is_empty());
  }

  #[test]
  fn rejects_oversized_source_before_copying_or_importing() {
    let root = std::env::temp_dir().join(format!("soma-oversized-import-test-{}", new_id()));
    let paths = create_workspace_dir(&root).unwrap();
    let source = root.join("oversized.txt");
    fs::File::create(&source).unwrap().set_len(SOURCE_IMPORT_MAX_BYTES + 1).unwrap();

    let error = import_source_file(&paths, &source).unwrap_err();

    assert_eq!(error.code, "Soma_VALIDATION_ERROR");
    assert!(error.message.contains("64 MiB"));
    assert_eq!(workspace_stats(&paths).unwrap()["sources"], 0);
    assert_eq!(fs::read_dir(paths.workspace_dir.join(RAW_IMPORT_DIR)).unwrap().count(), 0);
  }

  #[test]
  fn removes_only_the_new_raw_copy_when_database_import_fails() {
    let root = std::env::temp_dir().join(format!("soma-failed-import-test-{}", new_id()));
    let paths = create_workspace_dir(&root).unwrap();
    let source = root.join("conversation.txt");
    fs::write(&source, "Human: Keep raw imports transactional.").unwrap();
    let raw_import_dir = paths.workspace_dir.join(RAW_IMPORT_DIR);
    let existing_raw = raw_import_dir.join("existing-source.txt");
    fs::write(&existing_raw, "preexisting raw data").unwrap();
    let conn = open_existing_database(&paths.database_path).unwrap();
    conn
      .execute_batch(
        r#"
        CREATE TRIGGER reject_source_import
        BEFORE INSERT ON sources
        BEGIN
          SELECT RAISE(ABORT, 'forced source import failure');
        END;
        "#,
      )
      .unwrap();
    drop(conn);

    let error = import_source_file(&paths, &source).unwrap_err();
    let remaining_raw_files =
      fs::read_dir(&raw_import_dir).unwrap().map(|entry| entry.unwrap().path()).collect::<Vec<_>>();

    assert_eq!(error.code, "Soma_STORAGE_ERROR");
    assert_eq!(remaining_raw_files, vec![existing_raw.clone()]);
    assert_eq!(fs::read_to_string(existing_raw).unwrap(), "preexisting raw data");
    assert_eq!(workspace_stats(&paths).unwrap()["sources"], 0);
  }

  #[test]
  fn chunk_message_splits_long_text_without_oversized_chunks() {
    let text = format!("{}\n\n{}", "alpha ".repeat(420), "beta ".repeat(420));

    let chunks = chunk_message(&text);

    assert!(chunks.len() > 2);
    assert!(chunks.iter().all(|chunk| chunk.chars().count() <= CHUNK_MAX_CHARS));
    assert!(chunks.first().unwrap().contains("alpha"));
    assert!(chunks.last().unwrap().contains("beta"));
  }

  #[test]
  fn chunk_message_splits_multibyte_text_on_utf8_boundaries() {
    let text = "界".repeat(CHUNK_MAX_CHARS + 400);

    let chunks = chunk_message(&text);

    assert_eq!(chunks.len(), 2);
    assert!(chunks.iter().all(|chunk| chunk.chars().count() <= CHUNK_MAX_CHARS));
    assert_eq!(chunks.concat(), text);
  }

  #[test]
  fn imports_chatgpt_mapping_export_into_searchable_chunks() {
    let root = std::env::temp_dir().join(format!("soma-chatgpt-import-test-{}", new_id()));
    let paths = create_workspace_dir(&root).unwrap();
    let chatgpt_file = root.join("conversations.json");
    let export = json!([{
        "title": "CKC Architecture Conversation",
        "create_time": 1782500000.0,
        "mapping": {
            "root": {
                "id": "root",
                "message": null,
                "parent": null,
                "children": ["system"]
            },
            "system": {
                "id": "system",
                "parent": "root",
                "children": ["user1"],
                "message": {
                    "id": "message_system",
                    "author": { "role": "system" },
                    "create_time": 1782500001.0,
                    "content": {
                        "content_type": "text",
                        "parts": ["You are ChatGPT."]
                    }
                }
            },
            "user1": {
                "id": "user1",
                "parent": "system",
                "children": ["assistant1"],
                "message": {
                    "id": "message_user1",
                    "author": { "role": "user" },
                    "create_time": 1782500010.0,
                    "content": {
                        "content_type": "text",
                        "parts": ["The imported graph should keep source evidence and readable bridge text."]
                    }
                }
            },
            "assistant1": {
                "id": "assistant1",
                "parent": "user1",
                "children": [],
                "message": {
                    "id": "message_assistant1",
                    "author": { "role": "assistant" },
                    "create_time": 1782500020.0,
                    "content": {
                        "content_type": "text",
                        "parts": ["Extraction jobs should produce reviewed graph patches before graph truth changes."]
                    }
                }
            }
        }
    }]);
    fs::write(&chatgpt_file, serde_json::to_string_pretty(&export).unwrap()).unwrap();

    let imported = import_source_file(&paths, &chatgpt_file).unwrap();
    let stats = workspace_stats(&paths).unwrap();

    assert_eq!(imported["conversations"][0]["title"], "CKC Architecture Conversation");
    assert_eq!(stats["sources"], 1);
    assert_eq!(stats["conversations"], 1);
    assert_eq!(stats["messages"], 3);
    assert_eq!(stats["chunks"], 3);
    assert_eq!(stats["ftsRows"], 3);
    assert_eq!(search_chunks(&paths, "bridge", 10).unwrap().as_array().unwrap().len(), 1);
  }

  #[test]
  fn chatgpt_import_keeps_only_the_active_conversation_branch() {
    let root = std::env::temp_dir().join(format!("soma-chatgpt-branch-test-{}", new_id()));
    let paths = create_workspace_dir(&root).unwrap();
    let source = root.join("branched-conversation.json");
    let message = |role: &str, content: &str, created_at: f64| {
      json!({
        "author": { "role": role },
        "create_time": created_at,
        "content": { "content_type": "text", "parts": [content] }
      })
    };
    let export = json!({
      "title": "Branched conversation",
      "current_node": "assistant_active",
      "mapping": {
        "root": {
          "parent": null,
          "children": ["user"],
          "message": null
        },
        "user": {
          "parent": "root",
          "children": ["assistant_abandoned", "assistant_active"],
          "message": message("user", "Which design is grounded?", 1.0)
        },
        "assistant_abandoned": {
          "parent": "user",
          "children": [],
          "message": message("assistant", "Abandoned branch sentinel.", 2.0)
        },
        "assistant_active": {
          "parent": "user",
          "children": [],
          "message": message("assistant", "Active branch sentinel.", 3.0)
        }
      }
    });
    fs::write(&source, serde_json::to_vec(&export).unwrap()).unwrap();

    import_source_file(&paths, &source).unwrap();

    assert_eq!(workspace_stats(&paths).unwrap()["messages"], 2);
    assert_eq!(search_chunks(&paths, "active", 10).unwrap().as_array().unwrap().len(), 1);
    assert!(search_chunks(&paths, "abandoned", 10).unwrap().as_array().unwrap().is_empty());
  }
}
