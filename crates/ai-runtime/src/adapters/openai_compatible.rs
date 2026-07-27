use super::{handle_provider_response, read_provider_response_body};
use crate::credentials::{CredentialRef, CredentialResolver};
use crate::errors::AiRuntimeError;
use crate::ids::{ModelId, ProviderId};
use crate::message::{ContentPart, MessageRole};
use crate::request::AiRequest;
use crate::response::{AiResponse, FinishReason, TokenUsage};
use serde_json::{json, Map, Value};
use std::time::Duration;

const CHAT_COMPLETIONS_SUFFIX: &str = "chat/completions";
const MODELS_SUFFIX: &str = "models";
const DEFAULT_TIMEOUT_MS: u64 = 180_000;
const DEFAULT_MODEL_LIST_TIMEOUT_MS: u64 = 5_000;

#[derive(Debug, Clone)]
pub struct OpenAiCompatibleConfig {
  pub provider_id: ProviderId,
  pub base_url: String,
  pub credential: CredentialRef,
  pub require_api_key: bool,
}

pub struct OpenAiCompatibleProvider {
  config: OpenAiCompatibleConfig,
}

impl OpenAiCompatibleProvider {
  pub fn new(config: OpenAiCompatibleConfig) -> Self {
    Self { config }
  }

  fn endpoint_url(&self) -> Result<String, AiRuntimeError> {
    self.default_url_for(CHAT_COMPLETIONS_SUFFIX)
  }

  fn models_url(&self) -> Result<String, AiRuntimeError> {
    self.default_url_for(MODELS_SUFFIX)
  }

  fn default_url_for(&self, suffix: &str) -> Result<String, AiRuntimeError> {
    let base = self.base_url()?;
    let has_chat_suffix = has_chat_completions_suffix(base);
    if suffix == CHAT_COMPLETIONS_SUFFIX && has_chat_suffix {
      return Ok(base.to_string());
    }
    let base = strip_chat_completions_suffix(base);
    if !has_chat_suffix && needs_v1_prefix(base) {
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
        message: "OpenAI-compatible base URL is empty.".to_string(),
      });
    }
    Ok(base)
  }
}

impl OpenAiCompatibleProvider {
  pub fn complete(
    &self,
    request: AiRequest,
    credentials: &dyn CredentialResolver,
  ) -> Result<AiResponse, AiRuntimeError> {
    let secret = self.resolve_secret(credentials)?;
    let timeout = Duration::from_millis(request.timeout_ms.unwrap_or(DEFAULT_TIMEOUT_MS));
    let agent = ureq::AgentBuilder::new().timeout(timeout).build();
    let body = completion_body(&request);
    let url = self.endpoint_url()?;
    let mut call = agent.post(&url).set("Accept", "application/json").set("Content-Type", "application/json");
    if let Some(secret) = secret.as_deref().filter(|secret| !secret.trim().is_empty()) {
      call = call.set("Authorization", &format!("Bearer {secret}"));
    }

    let response = handle_provider_response(&self.config.provider_id, call.send_json(body))?;
    let raw = read_provider_response_body(&self.config.provider_id, response)?;
    let value: Value = serde_json::from_str(&raw).map_err(|error| AiRuntimeError::InvalidProviderResponse {
      provider: self.config.provider_id.clone(),
      message: format!("response body is not valid JSON: {error}"),
    })?;
    response_from_value(&self.config.provider_id, request.model, &value)
  }

  pub fn list_model_ids(&self, credentials: &dyn CredentialResolver) -> Result<Vec<String>, AiRuntimeError> {
    let secret = self.resolve_secret(credentials)?;
    let agent = ureq::AgentBuilder::new().timeout(Duration::from_millis(DEFAULT_MODEL_LIST_TIMEOUT_MS)).build();
    let mut call = agent.get(&self.models_url()?).set("Accept", "application/json");
    if let Some(secret) = secret.as_deref().filter(|secret| !secret.trim().is_empty()) {
      call = call.set("Authorization", &format!("Bearer {secret}"));
    }

    let response = handle_provider_response(&self.config.provider_id, call.call())?;
    let raw = read_provider_response_body(&self.config.provider_id, response)?;
    let value: Value = serde_json::from_str(&raw).map_err(|error| AiRuntimeError::InvalidProviderResponse {
      provider: self.config.provider_id.clone(),
      message: format!("response body is not valid JSON: {error}"),
    })?;
    Ok(model_ids_from_value(&value))
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
}

fn strip_chat_completions_suffix(base: &str) -> &str {
  base.strip_suffix("/chat/completions").unwrap_or(base)
}

fn has_chat_completions_suffix(base: &str) -> bool {
  base.ends_with("/chat/completions")
}

fn needs_v1_prefix(base: &str) -> bool {
  path_after_authority(base).map(str::trim).filter(|path| !path.is_empty() && *path != "/").is_none()
}

fn path_after_authority(url: &str) -> Option<&str> {
  let authority = url.find("://").map(|index| index + 3).unwrap_or(0);
  let rest = url.get(authority..)?;
  rest.find('/').map(|index| &rest[index..])
}

fn completion_body(request: &AiRequest) -> Value {
  let mut body = Map::new();
  body.insert("model".to_string(), json!(request.model.as_str()));
  body.insert("messages".to_string(), Value::Array(request.messages.iter().map(message_to_value).collect()));
  if let Some(temperature) = request.temperature {
    body.insert("temperature".to_string(), json!(temperature));
  }
  if let Some(max_tokens) = request.max_output_tokens {
    body.insert("max_tokens".to_string(), json!(max_tokens));
  }
  Value::Object(body)
}

fn message_to_value(message: &crate::message::AiMessage) -> Value {
  json!({
      "role": role_name(&message.role),
      "content": content_text(&message.content)
  })
}

fn role_name(role: &MessageRole) -> &'static str {
  match role {
    MessageRole::System => "system",
    MessageRole::User => "user",
    MessageRole::Assistant => "assistant",
    MessageRole::Tool => "tool",
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
  let choice = value.get("choices").and_then(Value::as_array).and_then(|choices| choices.first()).ok_or_else(|| {
    AiRuntimeError::InvalidProviderResponse {
      provider: provider.clone(),
      message: "response has no choices[0] item".to_string(),
    }
  })?;
  let content =
    choice.pointer("/message/content").or_else(|| choice.get("text")).and_then(Value::as_str).ok_or_else(|| {
      AiRuntimeError::InvalidProviderResponse {
        provider: provider.clone(),
        message: "response has no choices[0].message.content string".to_string(),
      }
    })?;
  Ok(AiResponse {
    content: content.to_string(),
    finish_reason: finish_reason(choice.get("finish_reason")),
    usage: token_usage(value.get("usage")),
    provider: provider.clone(),
    model,
  })
}

fn finish_reason(value: Option<&Value>) -> FinishReason {
  match value.and_then(Value::as_str).unwrap_or("") {
    "stop" => FinishReason::Stop,
    "length" => FinishReason::Length,
    "content_filter" => FinishReason::ContentFilter,
    "tool_calls" => FinishReason::ToolCalls,
    "" => FinishReason::Other("unknown".to_string()),
    other => FinishReason::Other(other.to_string()),
  }
}

fn token_usage(value: Option<&Value>) -> Option<TokenUsage> {
  let usage = value?;
  let input = usage.get("prompt_tokens").and_then(as_u32).unwrap_or_default();
  let output = usage.get("completion_tokens").and_then(as_u32).unwrap_or_default();
  let total = usage.get("total_tokens").and_then(as_u32).unwrap_or(input.saturating_add(output));
  Some(TokenUsage { input_tokens: input, output_tokens: output, total_tokens: total })
}

fn model_ids_from_value(value: &Value) -> Vec<String> {
  let mut ids = value
    .get("data")
    .or_else(|| value.get("models"))
    .and_then(Value::as_array)
    .into_iter()
    .flatten()
    .filter_map(|item| {
      item
        .get("id")
        .or_else(|| item.get("name"))
        .or_else(|| item.get("model"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|id| !id.is_empty())
        .map(str::to_string)
    })
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
  use crate::credentials::NoopCredentialResolver;
  use crate::ids::ModelId;
  use crate::message::AiMessage;
  use std::collections::HashMap;
  use std::io::{Read, Write};
  use std::net::{TcpListener, TcpStream};
  use std::thread;

  #[test]
  fn sends_expected_openai_compatible_json_body_for_text_completion() {
    let (base_url, server) = serve_once(200, completion_response("hello", true));
    let mut request = text_request(&base_url);
    request.messages = vec![AiMessage::system_text("system prompt"), AiMessage::user_text("user prompt")];
    request.temperature = Some(0.2);
    request.max_output_tokens = Some(128);

    provider(&base_url).complete(request, &NoopCredentialResolver).unwrap();
    let captured = server.join().unwrap();

    assert_eq!(captured.path, "/v1/chat/completions");
    assert_eq!(captured.body["model"], "fixture-model");
    assert_eq!(captured.body["messages"][0]["role"], "system");
    assert_eq!(captured.body["messages"][0]["content"], "system prompt");
    assert_eq!(captured.body["messages"][1]["role"], "user");
    assert_eq!(captured.body["messages"][1]["content"], "user prompt");
    let temperature = captured.body["temperature"].as_f64().unwrap();
    assert!((temperature - 0.2).abs() < 0.000_001);
    assert_eq!(captured.body["max_tokens"], 128);
    assert!(captured.body.get("response_format").is_none());
  }

  #[test]
  fn sends_authorization_header_when_credential_resolver_provides_key() {
    let (base_url, server) = serve_once(200, completion_response("authorized", false));
    let mut config = config(&base_url);
    config.credential = CredentialRef::ApiKey { provider: ProviderId::from("openai"), profile: "default".to_string() };
    config.require_api_key = true;

    OpenAiCompatibleProvider::new(config).complete(text_request(&base_url), &StaticCredential("secret-key")).unwrap();
    let captured = server.join().unwrap();

    assert_eq!(captured.headers.get("authorization").map(String::as_str), Some("Bearer secret-key"));
  }

  #[test]
  fn omits_authorization_header_when_no_credential_is_required() {
    let (base_url, server) = serve_once(200, completion_response("no key", false));

    provider(&base_url).complete(text_request(&base_url), &StaticCredential("unused")).unwrap();
    let captured = server.join().unwrap();

    assert!(!captured.headers.contains_key("authorization"));
  }

  #[test]
  fn parses_chat_completion_response_and_token_usage() {
    let (base_url, server) = serve_once(200, completion_response("answer", true));

    let response = provider(&base_url).complete(text_request(&base_url), &NoopCredentialResolver).unwrap();
    server.join().unwrap();

    assert_eq!(response.content, "answer");
    assert_eq!(response.finish_reason, FinishReason::Stop);
    assert_eq!(response.usage, Some(TokenUsage { input_tokens: 11, output_tokens: 7, total_tokens: 18 }));
  }

  #[test]
  fn lists_openai_compatible_models() {
    let (base_url, server) = serve_once(
      200,
      json!({
        "data": [
          { "id": "z-ai/glm-5.2" },
          { "id": "moonshotai/kimi-k2.6" }
        ]
      })
      .to_string(),
    );

    let models = provider(&base_url).list_model_ids(&NoopCredentialResolver).unwrap();
    let captured = server.join().unwrap();

    assert_eq!(captured.path, "/v1/models");
    assert_eq!(models, vec!["moonshotai/kimi-k2.6", "z-ai/glm-5.2"]);
  }

  #[test]
  fn classifies_model_catalog_transport_timeout() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let base_url = format!("http://{}", listener.local_addr().unwrap());
    let server = thread::spawn(move || {
      let (_stream, _) = listener.accept().unwrap();
      thread::sleep(Duration::from_millis(DEFAULT_MODEL_LIST_TIMEOUT_MS + 250));
    });

    let error = provider(&base_url).list_model_ids(&NoopCredentialResolver).unwrap_err();
    server.join().unwrap();

    assert!(
      matches!(
        &error,
        AiRuntimeError::Timeout { provider, .. } if *provider == ProviderId::from("openai_compatible")
      ),
      "unexpected timeout classification: {error:?}"
    );
  }

  #[test]
  fn rejects_oversized_completion_response_body() {
    let (base_url, server) = serve_once(200, oversized_response_body());

    let error = provider(&base_url).complete(text_request(&base_url), &NoopCredentialResolver).unwrap_err();
    server.join().unwrap();

    assert!(matches!(
      error,
      AiRuntimeError::ResponseBodyTooLarge { provider, limit_bytes }
        if provider == ProviderId::from("openai_compatible")
          && limit_bytes == crate::adapters::PROVIDER_RESPONSE_BODY_MAX_BYTES
    ));
  }

  #[test]
  fn rejects_oversized_model_catalog_response_body() {
    let (base_url, server) = serve_once(200, oversized_response_body());

    let error = provider(&base_url).list_model_ids(&NoopCredentialResolver).unwrap_err();
    server.join().unwrap();

    assert!(matches!(
      error,
      AiRuntimeError::ResponseBodyTooLarge { provider, limit_bytes }
        if provider == ProviderId::from("openai_compatible")
          && limit_bytes == crate::adapters::PROVIDER_RESPONSE_BODY_MAX_BYTES
    ));
  }

  #[test]
  fn lists_models_with_authorization_header() {
    let (base_url, server) = serve_once(
      200,
      json!({
        "models": [
          { "name": "llama3.3" },
          { "model": "gemma4" }
        ]
      })
      .to_string(),
    );
    let mut config = config(&base_url);
    config.credential =
      CredentialRef::ApiKey { provider: ProviderId::from("openrouter"), profile: "default".to_string() };
    config.require_api_key = true;

    let models = OpenAiCompatibleProvider::new(config).list_model_ids(&StaticCredential("router-key")).unwrap();
    let captured = server.join().unwrap();

    assert_eq!(captured.headers.get("authorization").map(String::as_str), Some("Bearer router-key"));
    assert_eq!(models, vec!["gemma4", "llama3.3"]);
  }

  #[test]
  fn returns_typed_error_for_non_2xx_response() {
    let (base_url, server) = serve_once(500, json!({ "error": "failed" }).to_string());

    let error = provider(&base_url).complete(text_request(&base_url), &NoopCredentialResolver).unwrap_err();
    server.join().unwrap();

    assert!(matches!(
        error,
        AiRuntimeError::HttpStatus { provider, status }
            if provider == ProviderId::from("openai_compatible") && status == 500
    ));
  }

  #[test]
  fn returns_typed_error_for_malformed_response_json() {
    let (base_url, server) = serve_once(200, "{not json".to_string());

    let error = provider(&base_url).complete(text_request(&base_url), &NoopCredentialResolver).unwrap_err();
    server.join().unwrap();

    assert!(matches!(
        error,
        AiRuntimeError::InvalidProviderResponse { provider, .. }
            if provider == ProviderId::from("openai_compatible")
    ));
  }

  #[test]
  fn handles_base_url_with_and_without_trailing_slash() {
    for with_trailing_slash in [false, true] {
      let (base_url, server) = serve_once(200, completion_response("ok", false));
      let base_url = if with_trailing_slash { format!("{base_url}/") } else { base_url };

      provider(&base_url).complete(text_request(&base_url), &NoopCredentialResolver).unwrap();
      let captured = server.join().unwrap();

      assert_eq!(captured.path, "/v1/chat/completions");
    }
  }

  #[test]
  fn appends_chat_completions_to_versioned_base_url() {
    let (base_url, server) = serve_once(200, completion_response("ok", false));
    let provider = OpenAiCompatibleProvider::new(config(&format!("{base_url}/v1")));

    provider.complete(text_request(&base_url), &NoopCredentialResolver).unwrap();
    let captured = server.join().unwrap();

    assert_eq!(captured.path, "/v1/chat/completions");
  }

  #[test]
  fn preserves_full_chat_completions_endpoint() {
    let (base_url, server) = serve_once(200, completion_response("ok", false));
    let provider = OpenAiCompatibleProvider::new(config(&format!("{base_url}/v1/chat/completions")));

    provider.complete(text_request(&base_url), &NoopCredentialResolver).unwrap();
    let captured = server.join().unwrap();

    assert_eq!(captured.path, "/v1/chat/completions");
  }

  #[test]
  fn preserves_full_chat_completions_endpoint_without_version_path() {
    let (base_url, server) = serve_once(200, completion_response("ok", false));
    let provider = OpenAiCompatibleProvider::new(config(&format!("{base_url}/chat/completions")));

    provider.complete(text_request(&base_url), &NoopCredentialResolver).unwrap();
    let captured = server.join().unwrap();

    assert_eq!(captured.path, "/chat/completions");
  }

  #[test]
  fn lists_models_from_versioned_base_url() {
    let (base_url, server) = serve_once(
      200,
      json!({
        "data": [
          { "id": "openai/gpt-test" }
        ]
      })
      .to_string(),
    );
    let provider = OpenAiCompatibleProvider::new(config(&format!("{base_url}/api/v1")));

    let models = provider.list_model_ids(&NoopCredentialResolver).unwrap();
    let captured = server.join().unwrap();

    assert_eq!(captured.path, "/api/v1/models");
    assert_eq!(models, vec!["openai/gpt-test"]);
  }

  #[test]
  fn lists_models_next_to_full_chat_completions_endpoint_without_version_path() {
    let (base_url, server) = serve_once(
      200,
      json!({
        "data": [
          { "id": "deepseek-chat" }
        ]
      })
      .to_string(),
    );
    let provider = OpenAiCompatibleProvider::new(config(&format!("{base_url}/chat/completions")));

    let models = provider.list_model_ids(&NoopCredentialResolver).unwrap();
    let captured = server.join().unwrap();

    assert_eq!(captured.path, "/models");
    assert_eq!(models, vec!["deepseek-chat"]);
  }

  fn provider(base_url: &str) -> OpenAiCompatibleProvider {
    OpenAiCompatibleProvider::new(config(base_url))
  }

  fn config(base_url: &str) -> OpenAiCompatibleConfig {
    OpenAiCompatibleConfig {
      provider_id: ProviderId::from("openai_compatible"),
      base_url: base_url.to_string(),
      credential: CredentialRef::None,
      require_api_key: false,
    }
  }

  fn text_request(_base_url: &str) -> AiRequest {
    AiRequest::new(ModelId::from("fixture-model"), vec![AiMessage::user_text("hello")])
  }

  fn completion_response(content: &str, include_usage: bool) -> String {
    let mut response = json!({
        "choices": [{
            "message": { "content": content },
            "finish_reason": "stop"
        }]
    });
    if include_usage {
      response["usage"] = json!({
          "prompt_tokens": 11,
          "completion_tokens": 7,
          "total_tokens": 18
      });
    }
    response.to_string()
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

  #[derive(Debug)]
  struct CapturedRequest {
    path: String,
    headers: HashMap<String, String>,
    body: Value,
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
      let headers = String::from_utf8_lossy(&bytes[..header_end]);
      let content_length = headers
        .lines()
        .find_map(|line| {
          let (name, value) = line.split_once(':')?;
          name.eq_ignore_ascii_case("content-length").then(|| value.trim().parse::<usize>().ok()).flatten()
        })
        .unwrap_or(0);
      if bytes.len() >= header_end + 4 + content_length {
        let body = &bytes[header_end + 4..header_end + 4 + content_length];
        return captured_request(&headers, body);
      }
    }
    captured_request("", &[])
  }

  fn captured_request(headers: &str, body: &[u8]) -> CapturedRequest {
    let mut lines = headers.lines();
    let path = lines.next().and_then(|line| line.split_whitespace().nth(1)).unwrap_or("").to_string();
    let headers = lines
      .filter_map(|line| {
        let (name, value) = line.split_once(':')?;
        Some((name.to_ascii_lowercase(), value.trim().to_string()))
      })
      .collect();
    CapturedRequest { path, headers, body: serde_json::from_slice(body).unwrap_or(Value::Null) }
  }

  fn find_header_end(bytes: &[u8]) -> Option<usize> {
    bytes.windows(4).position(|window| window == b"\r\n\r\n")
  }

  struct StaticCredential(&'static str);

  impl CredentialResolver for StaticCredential {
    fn resolve(&self, _credential: &CredentialRef) -> Result<Option<String>, AiRuntimeError> {
      Ok(Some(self.0.to_string()))
    }
  }
}
