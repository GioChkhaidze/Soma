use super::{handle_provider_response, read_provider_response_body};
use crate::credentials::{CredentialRef, CredentialResolver};
use crate::errors::AiRuntimeError;
use crate::ids::{ModelId, ProviderId};
use crate::message::{ContentPart, MessageRole};
use crate::request::AiRequest;
use crate::response::{AiResponse, FinishReason, TokenUsage};
use serde_json::{json, Map, Value};
use std::time::Duration;

const MESSAGES_SUFFIX: &str = "messages";
const MODELS_SUFFIX: &str = "models";
const DEFAULT_TIMEOUT_MS: u64 = 180_000;
const DEFAULT_MODEL_LIST_TIMEOUT_MS: u64 = 5_000;
const DEFAULT_MAX_OUTPUT_TOKENS: u32 = 4096;

#[derive(Debug, Clone)]
pub struct AnthropicMessagesConfig {
  pub provider_id: ProviderId,
  pub base_url: String,
  pub credential: CredentialRef,
  pub require_api_key: bool,
  pub anthropic_version: String,
}

pub struct AnthropicMessagesProvider {
  config: AnthropicMessagesConfig,
}

impl AnthropicMessagesProvider {
  pub fn new(config: AnthropicMessagesConfig) -> Self {
    Self { config }
  }

  pub fn complete(
    &self,
    request: AiRequest,
    credentials: &dyn CredentialResolver,
  ) -> Result<AiResponse, AiRuntimeError> {
    let secret = self.resolve_secret(credentials)?;
    let timeout = Duration::from_millis(request.timeout_ms.unwrap_or(DEFAULT_TIMEOUT_MS));
    let agent = ureq::AgentBuilder::new().timeout(timeout).build();
    let model = request.model.clone();
    let body = completion_body(&request);
    let response = self.authorized_call(agent.post(&self.messages_url()?), secret.as_deref()).send_json(body);

    let raw = read_provider_response_body(
      &self.config.provider_id,
      handle_provider_response(&self.config.provider_id, response)?,
    )?;
    let value: Value = serde_json::from_str(&raw).map_err(|error| invalid_response(&self.config.provider_id, error))?;
    response_from_value(&self.config.provider_id, model, &value)
  }

  pub fn list_model_ids(&self, credentials: &dyn CredentialResolver) -> Result<Vec<String>, AiRuntimeError> {
    let secret = self.resolve_secret(credentials)?;
    let agent = ureq::AgentBuilder::new().timeout(Duration::from_millis(DEFAULT_MODEL_LIST_TIMEOUT_MS)).build();
    let response = self.authorized_call(agent.get(&self.models_url()?), secret.as_deref()).call();

    let raw = read_provider_response_body(
      &self.config.provider_id,
      handle_provider_response(&self.config.provider_id, response)?,
    )?;
    let value: Value = serde_json::from_str(&raw).map_err(|error| invalid_response(&self.config.provider_id, error))?;
    Ok(model_ids_from_value(&value))
  }

  fn authorized_call(&self, call: ureq::Request, secret: Option<&str>) -> ureq::Request {
    let mut call = call
      .set("Accept", "application/json")
      .set("Content-Type", "application/json")
      .set("anthropic-version", self.anthropic_version());
    if let Some(secret) = secret.filter(|secret| !secret.trim().is_empty()) {
      call = call.set("x-api-key", secret);
    }
    call
  }

  fn resolve_secret(&self, credentials: &dyn CredentialResolver) -> Result<Option<String>, AiRuntimeError> {
    if matches!(self.config.credential, CredentialRef::None) {
      if self.config.require_api_key {
        return Err(AiRuntimeError::MissingCredential { credential: self.config.credential.clone() });
      }
      return Ok(None);
    }

    let secret = credentials.resolve(&self.config.credential).map_err(|error| {
      AiRuntimeError::CredentialResolution { credential: self.config.credential.clone(), message: error.to_string() }
    })?;
    if self.config.require_api_key && secret.as_deref().unwrap_or("").trim().is_empty() {
      return Err(AiRuntimeError::MissingCredential { credential: self.config.credential.clone() });
    }
    Ok(secret)
  }

  fn messages_url(&self) -> Result<String, AiRuntimeError> {
    self.default_url_for(MESSAGES_SUFFIX)
  }

  fn models_url(&self) -> Result<String, AiRuntimeError> {
    self.default_url_for(MODELS_SUFFIX)
  }

  fn default_url_for(&self, suffix: &str) -> Result<String, AiRuntimeError> {
    let base = self.base_url()?;
    if suffix == MESSAGES_SUFFIX && base.ends_with("/messages") {
      return Ok(base.to_string());
    }
    let base = base.strip_suffix("/messages").unwrap_or(base);
    if needs_v1_prefix(base) {
      Ok(format!("{base}/v1/{suffix}"))
    } else {
      Ok(format!("{base}/{suffix}"))
    }
  }

  fn base_url(&self) -> Result<&str, AiRuntimeError> {
    let base = self.config.base_url.trim().trim_end_matches('/');
    if base.is_empty() {
      return Err(AiRuntimeError::ProviderExecution {
        provider: self.config.provider_id.clone(),
        message: "Anthropic Messages base URL is empty.".to_string(),
      });
    }
    Ok(base)
  }

  fn anthropic_version(&self) -> &str {
    let version = self.config.anthropic_version.trim();
    if version.is_empty() {
      "2023-06-01"
    } else {
      version
    }
  }
}

fn needs_v1_prefix(base: &str) -> bool {
  path_after_authority(base).map(str::trim).filter(|path| !path.is_empty() && *path != "/").is_none()
}

fn path_after_authority(url: &str) -> Option<&str> {
  let authority = url.find("://").map(|index| index + 3).unwrap_or(0);
  let rest = url.get(authority..)?;
  rest.find('/').map(|index| &rest[index..])
}

fn invalid_response(provider: &ProviderId, error: serde_json::Error) -> AiRuntimeError {
  AiRuntimeError::InvalidProviderResponse {
    provider: provider.clone(),
    message: format!("response body is not valid JSON: {error}"),
  }
}

fn completion_body(request: &AiRequest) -> Value {
  let (system, messages) = prompt_parts(&request.messages);
  let mut body = Map::new();
  body.insert("model".to_string(), json!(request.model.as_str()));
  body.insert("max_tokens".to_string(), json!(request.max_output_tokens.unwrap_or(DEFAULT_MAX_OUTPUT_TOKENS)));
  if !system.is_empty() {
    body.insert("system".to_string(), json!(system));
  }
  if let Some(temperature) = request.temperature {
    body.insert("temperature".to_string(), json!(temperature));
  }
  body.insert("messages".to_string(), Value::Array(messages));
  Value::Object(body)
}

fn prompt_parts(messages: &[crate::message::AiMessage]) -> (String, Vec<Value>) {
  let mut system = Vec::new();
  let mut turns = Vec::new();
  for message in messages {
    match message.role {
      MessageRole::System => system.push(content_text(&message.content)),
      MessageRole::User | MessageRole::Assistant | MessageRole::Tool => {
        turns.push(json!({
            "role": anthropic_role(&message.role),
            "content": content_text(&message.content)
        }));
      }
    }
  }
  (system.join("\n\n"), turns)
}

fn anthropic_role(role: &MessageRole) -> &'static str {
  match role {
    MessageRole::Assistant => "assistant",
    _ => "user",
  }
}

fn content_text(parts: &[ContentPart]) -> String {
  parts
    .iter()
    .map(|part| match part {
      ContentPart::Text(text) => text.clone(),
      ContentPart::Json(value) => serde_json::to_string(value).unwrap_or_else(|_| value.to_string()),
    })
    .collect::<Vec<_>>()
    .join("\n")
}

fn response_from_value(provider: &ProviderId, model: ModelId, value: &Value) -> Result<AiResponse, AiRuntimeError> {
  let content = value
    .get("content")
    .and_then(Value::as_array)
    .into_iter()
    .flatten()
    .filter_map(|part| {
      (part.get("type").and_then(Value::as_str) == Some("text"))
        .then(|| part.get("text").and_then(Value::as_str))
        .flatten()
    })
    .collect::<Vec<_>>()
    .join("");
  if content.is_empty() {
    return Err(AiRuntimeError::InvalidProviderResponse {
      provider: provider.clone(),
      message: "response content has no text parts".to_string(),
    });
  }
  Ok(AiResponse {
    content,
    finish_reason: finish_reason(value.get("stop_reason")),
    usage: token_usage(value.get("usage")),
    provider: provider.clone(),
    model,
  })
}

fn finish_reason(value: Option<&Value>) -> FinishReason {
  match value.and_then(Value::as_str).unwrap_or("") {
    "end_turn" | "stop_sequence" => FinishReason::Stop,
    "max_tokens" => FinishReason::Length,
    "tool_use" => FinishReason::ToolCalls,
    "refusal" => FinishReason::ContentFilter,
    "" => FinishReason::Other("unknown".to_string()),
    other => FinishReason::Other(other.to_string()),
  }
}

fn token_usage(value: Option<&Value>) -> Option<TokenUsage> {
  let usage = value?;
  let input = usage.get("input_tokens").and_then(as_u32).unwrap_or_default();
  let output = usage.get("output_tokens").and_then(as_u32).unwrap_or_default();
  Some(TokenUsage { input_tokens: input, output_tokens: output, total_tokens: input.saturating_add(output) })
}

fn model_ids_from_value(value: &Value) -> Vec<String> {
  let mut ids = value
    .get("data")
    .or_else(|| value.get("models"))
    .and_then(Value::as_array)
    .into_iter()
    .flatten()
    .filter_map(|item| item.get("id").and_then(Value::as_str))
    .map(str::trim)
    .filter(|id| !id.is_empty())
    .map(str::to_string)
    .collect::<Vec<_>>();
  ids.sort();
  ids.dedup();
  ids
}

fn as_u32(value: &Value) -> Option<u32> {
  value.as_u64().and_then(|value| u32::try_from(value).ok())
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::ids::ModelId;
  use crate::message::AiMessage;
  use std::collections::HashMap;
  use std::io::{Read, Write};
  use std::net::{TcpListener, TcpStream};
  use std::thread;

  #[test]
  fn sends_anthropic_messages_request_and_parses_response() {
    let (base_url, server) = serve_once(
      200,
      json!({
        "content": [{ "type": "text", "text": "hello" }],
        "stop_reason": "end_turn",
        "usage": { "input_tokens": 12, "output_tokens": 6 }
      })
      .to_string(),
    );
    let mut request = AiRequest::new(
      ModelId::from("claude-sonnet-5"),
      vec![
        AiMessage::system_text("system prompt"),
        AiMessage::user_text("user prompt"),
        AiMessage::assistant_text("assistant prompt"),
      ],
    );
    request.temperature = Some(0.2);
    request.max_output_tokens = Some(512);

    let response = provider(&base_url).complete(request, &StaticCredential("anthropic-key")).unwrap();
    let captured = server.join().unwrap();

    assert_eq!(captured.path, "/v1/messages");
    assert_eq!(captured.headers.get("x-api-key").map(String::as_str), Some("anthropic-key"));
    assert_eq!(captured.headers.get("anthropic-version").map(String::as_str), Some("2023-06-01"));
    assert_eq!(captured.body["model"], "claude-sonnet-5");
    assert_eq!(captured.body["max_tokens"], 512);
    assert_eq!(captured.body["system"], "system prompt");
    assert_eq!(captured.body["messages"][0]["role"], "user");
    assert_eq!(captured.body["messages"][1]["role"], "assistant");
    let temperature = captured.body["temperature"].as_f64().unwrap();
    assert!((temperature - 0.2).abs() < 0.000_001);
    assert_eq!(response.content, "hello");
    assert_eq!(response.finish_reason, FinishReason::Stop);
    assert_eq!(response.usage, Some(TokenUsage { input_tokens: 12, output_tokens: 6, total_tokens: 18 }));
  }

  #[test]
  fn lists_anthropic_models() {
    let (base_url, server) = serve_once(
      200,
      json!({
        "data": [
          { "id": "claude-sonnet-5" },
          { "id": "claude-opus-4-8" }
        ]
      })
      .to_string(),
    );

    let models = provider(&base_url).list_model_ids(&StaticCredential("anthropic-key")).unwrap();
    let captured = server.join().unwrap();

    assert_eq!(captured.path, "/v1/models");
    assert_eq!(models, vec!["claude-opus-4-8", "claude-sonnet-5"]);
  }

  #[test]
  fn rejects_oversized_completion_response_body() {
    let (base_url, server) = serve_once(200, oversized_response_body());
    let request = AiRequest::new(ModelId::from("claude-sonnet-5"), vec![AiMessage::user_text("hello")]);

    let error = provider(&base_url).complete(request, &StaticCredential("anthropic-key")).unwrap_err();
    server.join().unwrap();

    assert!(matches!(
      error,
      AiRuntimeError::ResponseBodyTooLarge { provider, limit_bytes }
        if provider == ProviderId::from("claude")
          && limit_bytes == crate::adapters::PROVIDER_RESPONSE_BODY_MAX_BYTES
    ));
  }

  #[test]
  fn rejects_oversized_model_catalog_response_body() {
    let (base_url, server) = serve_once(200, oversized_response_body());

    let error = provider(&base_url).list_model_ids(&StaticCredential("anthropic-key")).unwrap_err();
    server.join().unwrap();

    assert!(matches!(
      error,
      AiRuntimeError::ResponseBodyTooLarge { provider, limit_bytes }
        if provider == ProviderId::from("claude")
          && limit_bytes == crate::adapters::PROVIDER_RESPONSE_BODY_MAX_BYTES
    ));
  }

  fn provider(base_url: &str) -> AnthropicMessagesProvider {
    AnthropicMessagesProvider::new(AnthropicMessagesConfig {
      provider_id: ProviderId::from("claude"),
      base_url: base_url.to_string(),
      credential: CredentialRef::ApiKey { provider: ProviderId::from("claude"), profile: "default".to_string() },
      require_api_key: true,
      anthropic_version: "2023-06-01".to_string(),
    })
  }

  fn oversized_response_body() -> String {
    json!({
      "padding": "x".repeat(crate::adapters::PROVIDER_RESPONSE_BODY_MAX_BYTES as usize)
    })
    .to_string()
  }

  fn serve_once(status: u16, response_body: String) -> (String, thread::JoinHandle<CapturedRequest>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let base_url = format!("http://{}", listener.local_addr().unwrap());
    let handle = thread::spawn(move || {
      let (mut stream, _) = listener.accept().unwrap();
      let captured = read_request(&mut stream);
      let status_text = if status == 200 { "OK" } else { "ERROR" };
      let response = format!(
        concat!(
          "HTTP/1.1 {status} {status_text}\r\n",
          "Content-Type: application/json\r\n",
          "Content-Length: {}\r\n",
          "Connection: close\r\n\r\n",
          "{}"
        ),
        response_body.len(),
        response_body,
        status = status,
        status_text = status_text
      );
      stream.write_all(response.as_bytes()).unwrap();
      captured
    });
    (base_url, handle)
  }

  fn read_request(stream: &mut TcpStream) -> CapturedRequest {
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
      let headers = String::from_utf8_lossy(&bytes[..header_end]).to_string();
      let content_length = headers
        .lines()
        .find_map(|line| {
          let (name, value) = line.split_once(':')?;
          name.eq_ignore_ascii_case("content-length").then(|| value.trim().parse::<usize>().ok()).flatten()
        })
        .unwrap_or(0);
      if bytes.len() >= header_end + 4 + content_length {
        let body = &bytes[header_end + 4..header_end + 4 + content_length];
        return CapturedRequest {
          path: headers.lines().next().and_then(|line| line.split_whitespace().nth(1)).unwrap_or("").to_string(),
          headers: parse_headers(&headers),
          body: serde_json::from_slice(body).unwrap_or(Value::Null),
        };
      }
    }
    CapturedRequest { path: String::new(), headers: HashMap::new(), body: Value::Null }
  }

  fn parse_headers(headers: &str) -> HashMap<String, String> {
    headers
      .lines()
      .skip(1)
      .filter_map(|line| {
        let (name, value) = line.split_once(':')?;
        Some((name.to_ascii_lowercase(), value.trim().to_string()))
      })
      .collect()
  }

  fn find_header_end(bytes: &[u8]) -> Option<usize> {
    bytes.windows(4).position(|window| window == b"\r\n\r\n")
  }

  struct CapturedRequest {
    path: String,
    headers: HashMap<String, String>,
    body: Value,
  }

  struct StaticCredential(&'static str);

  impl CredentialResolver for StaticCredential {
    fn resolve(&self, _credential: &CredentialRef) -> Result<Option<String>, AiRuntimeError> {
      Ok(Some(self.0.to_string()))
    }
  }
}
