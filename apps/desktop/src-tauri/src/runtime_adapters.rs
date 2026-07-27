use std::fs;
use std::path::{Path, PathBuf};

use serde_json::{json, Value};
#[cfg(test)]
use soma_ai_runtime::NoopCredentialResolver;
#[cfg(test)]
use soma_ai_runtime::{AgentTaskRequest, AgentTaskStatus, CliAgentConfig, CliAgentRuntime, CliPromptMode};
use soma_ai_runtime::{
  AiMessage, AiRequest, AiRuntimeError, AnthropicMessagesConfig, AnthropicMessagesProvider, CredentialRef,
  CredentialResolver, ModelId, OpenAiCompatibleConfig, OpenAiCompatibleProvider, ProviderId,
};

use crate::brain_provider_registry::{brain_provider, BrainProviderAdapter, BrainProviderSpec, DEFAULT_PROVIDER_ID};
use crate::brain_settings::BrainSettings;
use crate::chat_runtime::{
  chat_turn_prompt, current_chat_user_message, parse_chat_turn_response, RuntimeChatTurnResult,
};
use crate::contracts::empty_graph_patch;
use crate::error::{CommandError, CommandResult, RuntimeFailureKind};
use crate::secrets::AppDataCredentialStore;

#[path = "runtime_profile.rs"]
mod runtime_profile;

pub use runtime_profile::{authorize_codex_brain_status, codex_brain_status};
use runtime_profile::{run_profile_chat_turn, run_profile_command, ProfileCommand};

const JOB_PROMPT_MAX_BYTES: usize = 90_000;
const ADAPTER_OUTPUT_MAX_BYTES: usize = 180_000;
const HOSTED_CHUNKS_SECTION: &str = "\n--- chunks.json (included source chunks) ---\n";
const DIRECT_CHAT_SYSTEM_PROMPT: &str = concat!(
  "You are Soma's direct chat runtime. ",
  "Return only valid JSON with assistant_message, used_graph_areas, and proposed_graph_patch."
);

#[cfg(test)]
pub(crate) static RUNTIME_ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[derive(Debug, Clone)]
pub struct StoredCredentialResolver {
  store: AppDataCredentialStore,
}

impl StoredCredentialResolver {
  pub fn new(store: AppDataCredentialStore) -> Self {
    Self { store }
  }
}

impl CredentialResolver for StoredCredentialResolver {
  fn resolve(&self, credential: &CredentialRef) -> Result<Option<String>, AiRuntimeError> {
    let CredentialRef::ApiKey { provider, .. } = credential else {
      return Ok(None);
    };
    self
      .store
      .read_api_key(provider.as_str())
      .map_err(|error| AiRuntimeError::CredentialResolution { credential: credential.clone(), message: error.message })
  }
}

#[cfg(test)]
pub fn default_runtime_descriptor() -> Value {
  runtime_descriptor(&BrainSettings::default())
}

pub fn runtime_descriptor(settings: &BrainSettings) -> Value {
  let mut descriptor = json!({
    "schema_version": 1,
    "providerId": settings.provider_id,
    "model": settings.model,
    "endpoint": settings.endpoint,
    "credentialConfigured": settings.credential_configured,
    "adapter": adapter_for(settings)
  });
  if settings.provider_id == "codex_sdk" {
    descriptor["authProfile"] = json!(settings.auth_profile);
  }
  descriptor
}

fn adapter_for(settings: &BrainSettings) -> Value {
  let provider = brain_provider(&settings.provider_id).unwrap_or_else(default_provider);
  match provider.adapter {
    BrainProviderAdapter::LocalOpenAiCompatible => {
      let endpoint = effective_endpoint(provider, &settings.endpoint);
      json!({
      "kind": "local_offline_endpoint",
      "status": if endpoint.is_none() { "needs_endpoint" } else { "configured" },
      "transport": "openai_compatible_http",
      "providerId": settings.provider_id,
      "endpoint": endpoint.unwrap_or_else(|| settings.endpoint.trim().to_string()),
      "model": settings.model
        })
    }
    BrainProviderAdapter::CodexSdk => json!({
      "kind": "codex_sdk_profile",
      "status": "job_folder_ready",
      "profile": profile_or_default(&settings.auth_profile),
      "model": settings.model
    }),
    BrainProviderAdapter::ClaudeCode => json!({
      "kind": "claude_code_profile",
      "status": "job_folder_ready",
      "model": settings.model
    }),
    BrainProviderAdapter::AnthropicMessages => {
      let endpoint = effective_endpoint(provider, &settings.endpoint);
      json!({
      "kind": "anthropic_messages_provider",
      "status": api_provider_status(settings, endpoint.as_deref()),
      "transport": "anthropic_messages_http",
      "providerId": settings.provider_id,
      "model": settings.model,
      "endpoint": endpoint.unwrap_or_else(|| settings.endpoint.trim().to_string()),
      "credentialConfigured": settings.credential_configured,
      "requireApiKey": true,
      "anthropicVersion": "2023-06-01"
        })
    }
    BrainProviderAdapter::OpenAiCompatibleApi => {
      let endpoint = effective_endpoint(provider, &settings.endpoint);
      json!({
      "kind": "api_provider",
      "status": api_provider_status(settings, endpoint.as_deref()),
      "transport": "openai_compatible_http",
      "providerId": settings.provider_id,
      "model": settings.model,
      "endpoint": endpoint.unwrap_or_else(|| settings.endpoint.trim().to_string()),
      "credentialConfigured": settings.credential_configured,
      "requireApiKey": true
        })
    }
    BrainProviderAdapter::Managed => json!({
      "kind": "managed_provider",
      "status": "planned",
      "providerId": settings.provider_id
    }),
  }
}

fn default_provider() -> &'static BrainProviderSpec {
  brain_provider(DEFAULT_PROVIDER_ID).expect("default brain provider is registered")
}

fn effective_endpoint(provider: &BrainProviderSpec, endpoint: &str) -> Option<String> {
  let endpoint = endpoint.trim();
  if !endpoint.is_empty() {
    return Some(endpoint.to_string());
  }
  provider.default_endpoint.map(str::to_string)
}

fn profile_or_default(value: &str) -> &str {
  if value.trim().is_empty() {
    "default"
  } else {
    value.trim()
  }
}

fn api_provider_status(settings: &BrainSettings, endpoint: Option<&str>) -> &'static str {
  if endpoint.unwrap_or("").trim().is_empty() {
    return "needs_endpoint";
  }
  if settings.model.trim().is_empty() {
    return "needs_model";
  }
  if !settings.credential_configured {
    return "needs_api_key";
  }
  "configured"
}

#[derive(Debug)]
pub struct RuntimeRunResult {
  pub adapter_kind: String,
  pub status: &'static str,
  pub failure_kind: Option<RuntimeFailureKind>,
  pub message: String,
  pub wrote_output_patch: bool,
}

#[cfg(test)]
pub fn run_compile_job(job_dir: &Path, runtime: &Value) -> CommandResult<RuntimeRunResult> {
  run_compile_job_with_credentials(job_dir, runtime, &NoopCredentialResolver)
}

pub fn run_compile_job_with_credentials(
  job_dir: &Path,
  runtime: &Value,
  credentials: &dyn CredentialResolver,
) -> CommandResult<RuntimeRunResult> {
  let adapter = runtime.get("adapter").unwrap_or(&Value::Null);
  let adapter_kind = adapter.get("kind").and_then(Value::as_str).unwrap_or("unknown").to_string();

  match adapter_kind.as_str() {
    "local_offline_endpoint" | "api_provider" => run_openai_compatible_job(job_dir, runtime, adapter, credentials),
    "anthropic_messages_provider" => run_anthropic_messages_job(job_dir, runtime, adapter, credentials),
    "codex_sdk_profile" => run_profile_command(job_dir, runtime, adapter, ProfileCommand::Codex),
    "claude_code_profile" => run_profile_command(job_dir, runtime, adapter, ProfileCommand::Claude),
    "managed_provider" => Ok(RuntimeRunResult {
      adapter_kind,
      status: "unsupported",
      failure_kind: Some(RuntimeFailureKind::Unsupported),
      message: "Managed Soma runtime is not connected yet.".to_string(),
      wrote_output_patch: false,
    }),
    _ => Ok(RuntimeRunResult {
      adapter_kind,
      status: "unsupported",
      failure_kind: Some(RuntimeFailureKind::Unsupported),
      message: "This runtime adapter is not supported for execution.".to_string(),
      wrote_output_patch: false,
    }),
  }
}

pub fn run_chat_turn_with_credentials(
  runtime: &Value,
  request: &Value,
  credentials: &dyn CredentialResolver,
) -> CommandResult<RuntimeChatTurnResult> {
  let adapter = runtime.get("adapter").unwrap_or(&Value::Null);
  let adapter_kind = adapter_kind(adapter);

  match adapter_kind.as_str() {
    "local_offline_endpoint" | "api_provider" => {
      run_openai_compatible_chat_turn(runtime, adapter, request, credentials)
    }
    "anthropic_messages_provider" => run_anthropic_messages_chat_turn(runtime, adapter, request, credentials),
    "codex_sdk_profile" => run_profile_chat_turn(runtime, adapter, request, ProfileCommand::Codex),
    "claude_code_profile" => run_profile_chat_turn(runtime, adapter, request, ProfileCommand::Claude),
    "managed_provider" => Ok(RuntimeChatTurnResult {
      adapter_kind,
      status: "unsupported",
      failure_kind: Some(RuntimeFailureKind::Unsupported),
      message: "Managed Soma runtime is not connected yet.".to_string(),
      assistant_message: None,
      used_graph_areas: Vec::new(),
      proposed_graph_patch: None,
    }),
    _ => Ok(RuntimeChatTurnResult {
      adapter_kind,
      status: "unsupported",
      failure_kind: Some(RuntimeFailureKind::Unsupported),
      message: "This runtime adapter is not supported for chat turns.".to_string(),
      assistant_message: None,
      used_graph_areas: Vec::new(),
      proposed_graph_patch: None,
    }),
  }
}

fn run_openai_compatible_job(
  job_dir: &Path,
  runtime: &Value,
  adapter: &Value,
  credentials: &dyn CredentialResolver,
) -> CommandResult<RuntimeRunResult> {
  let adapter_kind = adapter_kind(adapter);
  let endpoint = openai_compatible_endpoint(runtime, adapter);
  if endpoint.is_empty() {
    return Ok(RuntimeRunResult {
      adapter_kind,
      status: "failed",
      failure_kind: Some(RuntimeFailureKind::Configuration),
      message: "Runtime needs an OpenAI-compatible HTTP endpoint.".to_string(),
      wrote_output_patch: false,
    });
  }

  let HostedJobPrompt { text: prompt, included_chunk_count, total_chunk_count } = hosted_job_prompt(job_dir)?;
  let content = run_openai_compatible_text_completion(
    runtime,
    adapter,
    vec![
      AiMessage::system_text("You are Soma's graph compiler. Return only valid JSON matching the GraphPatch schema."),
      AiMessage::user_text(prompt),
    ],
    Some(0.1),
    credentials,
  )?;
  write_extracted_patch(job_dir, &content).map(|wrote| RuntimeRunResult {
    adapter_kind,
    status: if wrote { "completed" } else { "failed" },
    failure_kind: if wrote { None } else { Some(RuntimeFailureKind::InvalidResponse) },
    message: if wrote {
      format!(
        "OpenAI-compatible runtime wrote output_patch.json using {included_chunk_count} \
         of {total_chunk_count} job chunks."
      )
    } else {
      format!(
        "OpenAI-compatible response did not contain a valid GraphPatch JSON object. \
         The runtime received {included_chunk_count} of {total_chunk_count} job chunks."
      )
    },
    wrote_output_patch: wrote,
  })
}

fn run_openai_compatible_chat_turn(
  runtime: &Value,
  adapter: &Value,
  request: &Value,
  credentials: &dyn CredentialResolver,
) -> CommandResult<RuntimeChatTurnResult> {
  let adapter_kind = adapter_kind(adapter);
  let endpoint = openai_compatible_endpoint(runtime, adapter);
  if endpoint.is_empty() {
    return Ok(RuntimeChatTurnResult {
      adapter_kind,
      status: "failed",
      failure_kind: Some(RuntimeFailureKind::Configuration),
      message:
        "Runtime needs an http:// or https:// OpenAI-compatible endpoint in Brain Settings before chat can answer."
          .to_string(),
      assistant_message: None,
      used_graph_areas: Vec::new(),
      proposed_graph_patch: None,
    });
  }

  let content = run_openai_compatible_text_completion(
    runtime,
    adapter,
    vec![AiMessage::system_text(DIRECT_CHAT_SYSTEM_PROMPT), AiMessage::user_text(chat_turn_prompt(request))],
    Some(0.2),
    credentials,
  )?;
  parse_chat_turn_response(&adapter_kind, &content, current_chat_user_message(request))
}

fn run_openai_compatible_text_completion(
  runtime: &Value,
  adapter: &Value,
  messages: Vec<AiMessage>,
  temperature: Option<f32>,
  credentials: &dyn CredentialResolver,
) -> CommandResult<String> {
  let model = runtime.get("model").and_then(Value::as_str).unwrap_or("").trim();
  let provider = openai_compatible_provider(runtime, adapter)?;
  let mut request = AiRequest::new(ModelId::from(model), messages);
  request.temperature = temperature;
  request.timeout_ms = Some(180_000);

  let response = provider.complete(request, credentials).map_err(ai_runtime_error)?;
  Ok(response.content)
}

fn run_anthropic_messages_job(
  job_dir: &Path,
  runtime: &Value,
  adapter: &Value,
  credentials: &dyn CredentialResolver,
) -> CommandResult<RuntimeRunResult> {
  let adapter_kind = adapter_kind(adapter);
  let endpoint = anthropic_messages_endpoint(runtime, adapter);
  if endpoint.is_empty() {
    return Ok(RuntimeRunResult {
      adapter_kind,
      status: "failed",
      failure_kind: Some(RuntimeFailureKind::Configuration),
      message: "Runtime needs an Anthropic Messages HTTP endpoint.".to_string(),
      wrote_output_patch: false,
    });
  }

  let HostedJobPrompt { text: prompt, included_chunk_count, total_chunk_count } = hosted_job_prompt(job_dir)?;
  let content = run_anthropic_messages_text_completion(
    runtime,
    adapter,
    vec![
      AiMessage::system_text("You are Soma's graph compiler. Return only valid JSON matching the GraphPatch schema."),
      AiMessage::user_text(prompt),
    ],
    credentials,
  )?;
  write_extracted_patch(job_dir, &content).map(|wrote| RuntimeRunResult {
    adapter_kind,
    status: if wrote { "completed" } else { "failed" },
    failure_kind: if wrote { None } else { Some(RuntimeFailureKind::InvalidResponse) },
    message: if wrote {
      format!(
        "Anthropic Messages runtime wrote output_patch.json using {included_chunk_count} \
         of {total_chunk_count} job chunks."
      )
    } else {
      format!(
        "Anthropic response did not contain a valid GraphPatch JSON object. \
         The runtime received {included_chunk_count} of {total_chunk_count} job chunks."
      )
    },
    wrote_output_patch: wrote,
  })
}

fn run_anthropic_messages_chat_turn(
  runtime: &Value,
  adapter: &Value,
  request: &Value,
  credentials: &dyn CredentialResolver,
) -> CommandResult<RuntimeChatTurnResult> {
  let adapter_kind = adapter_kind(adapter);
  let endpoint = anthropic_messages_endpoint(runtime, adapter);
  if endpoint.is_empty() {
    return Ok(RuntimeChatTurnResult {
      adapter_kind,
      status: "failed",
      failure_kind: Some(RuntimeFailureKind::Configuration),
      message: "Runtime needs an Anthropic Messages HTTP endpoint in Brain Settings before chat can answer."
        .to_string(),
      assistant_message: None,
      used_graph_areas: Vec::new(),
      proposed_graph_patch: None,
    });
  }

  let content = run_anthropic_messages_text_completion(
    runtime,
    adapter,
    vec![AiMessage::system_text(DIRECT_CHAT_SYSTEM_PROMPT), AiMessage::user_text(chat_turn_prompt(request))],
    credentials,
  )?;
  parse_chat_turn_response(&adapter_kind, &content, current_chat_user_message(request))
}

fn run_anthropic_messages_text_completion(
  runtime: &Value,
  adapter: &Value,
  messages: Vec<AiMessage>,
  credentials: &dyn CredentialResolver,
) -> CommandResult<String> {
  let model = runtime.get("model").and_then(Value::as_str).unwrap_or("").trim();
  let provider = anthropic_messages_provider(runtime, adapter)?;
  let mut request = AiRequest::new(ModelId::from(model), messages);
  request.timeout_ms = Some(180_000);

  let response = provider.complete(request, credentials).map_err(ai_runtime_error)?;
  Ok(response.content)
}

pub fn list_runtime_models(runtime: &Value, credentials: &dyn CredentialResolver) -> CommandResult<Value> {
  let adapter = runtime.get("adapter").unwrap_or(&Value::Null);
  let adapter_kind = adapter_kind(adapter);
  let provider_id = runtime.get("providerId").and_then(Value::as_str).unwrap_or("unknown");
  let models = match adapter_kind.as_str() {
    "local_offline_endpoint" | "api_provider" => {
      let provider = match openai_compatible_provider(runtime, adapter) {
        Ok(provider) => provider,
        Err(error) => {
          return Ok(json!({
            "providerId": provider_id,
            "status": "failed",
            "message": error.message,
            "models": []
          }));
        }
      };
      provider.list_model_ids(credentials)
    }
    "anthropic_messages_provider" => {
      let provider = match anthropic_messages_provider(runtime, adapter) {
        Ok(provider) => provider,
        Err(error) => {
          return Ok(json!({
            "providerId": provider_id,
            "status": "failed",
            "message": error.message,
            "models": []
          }));
        }
      };
      provider.list_model_ids(credentials)
    }
    _ => {
      return Ok(json!({
        "providerId": provider_id,
        "status": "unsupported",
        "message": "This runtime does not expose a model catalog.",
        "models": []
      }));
    }
  };
  match models {
    Ok(models) => Ok(json!({
      "providerId": provider_id,
      "status": "ready",
      "message": format!("Loaded {} model{}.", models.len(), if models.len() == 1 { "" } else { "s" }),
      "models": models
    })),
    Err(error) => Ok(json!({
      "providerId": provider_id,
      "status": "failed",
      "message": error.to_string(),
      "models": []
    })),
  }
}

fn openai_compatible_provider(runtime: &Value, adapter: &Value) -> CommandResult<OpenAiCompatibleProvider> {
  let endpoint = openai_compatible_endpoint(runtime, adapter);
  validate_http_endpoint(&endpoint)?;
  let provider_id = ProviderId::from(runtime.get("providerId").and_then(Value::as_str).unwrap_or("local_llm"));
  let require_api_key =
    adapter.get("requireApiKey").and_then(Value::as_bool).unwrap_or_else(|| adapter_kind(adapter) == "api_provider");
  let credential = if require_api_key {
    CredentialRef::ApiKey {
      provider: provider_id.clone(),
      profile: runtime.get("authProfile").and_then(Value::as_str).unwrap_or("default").trim().to_string(),
    }
  } else {
    CredentialRef::None
  };
  Ok(OpenAiCompatibleProvider::new(OpenAiCompatibleConfig {
    provider_id,
    base_url: endpoint,
    credential,
    require_api_key,
  }))
}

fn anthropic_messages_provider(runtime: &Value, adapter: &Value) -> CommandResult<AnthropicMessagesProvider> {
  let endpoint = anthropic_messages_endpoint(runtime, adapter);
  validate_http_endpoint(&endpoint)?;
  let provider_id = ProviderId::from(runtime.get("providerId").and_then(Value::as_str).unwrap_or("claude"));
  let require_api_key = adapter.get("requireApiKey").and_then(Value::as_bool).unwrap_or(true);
  let credential = if require_api_key {
    CredentialRef::ApiKey {
      provider: provider_id.clone(),
      profile: runtime.get("authProfile").and_then(Value::as_str).unwrap_or("default").trim().to_string(),
    }
  } else {
    CredentialRef::None
  };
  Ok(AnthropicMessagesProvider::new(AnthropicMessagesConfig {
    provider_id,
    base_url: endpoint,
    credential,
    require_api_key,
    anthropic_version: adapter.get("anthropicVersion").and_then(Value::as_str).unwrap_or("2023-06-01").to_string(),
  }))
}

fn openai_compatible_endpoint(runtime: &Value, adapter: &Value) -> String {
  adapter
    .get("endpoint")
    .or_else(|| runtime.get("endpoint"))
    .and_then(Value::as_str)
    .map(str::trim)
    .filter(|value| !value.is_empty())
    .map(str::to_string)
    .or_else(|| {
      runtime
        .get("providerId")
        .and_then(Value::as_str)
        .and_then(brain_provider)
        .and_then(|provider| provider.default_endpoint)
        .map(str::to_string)
    })
    .unwrap_or_default()
}

fn anthropic_messages_endpoint(runtime: &Value, adapter: &Value) -> String {
  adapter
    .get("endpoint")
    .or_else(|| runtime.get("endpoint"))
    .and_then(Value::as_str)
    .map(str::trim)
    .filter(|value| !value.is_empty())
    .map(str::to_string)
    .or_else(|| brain_provider("claude").and_then(|provider| provider.default_endpoint).map(str::to_string))
    .unwrap_or_default()
}

fn validate_http_endpoint(endpoint: &str) -> CommandResult<()> {
  let endpoint = endpoint.trim().trim_end_matches('/');
  if endpoint.is_empty() {
    return Err(CommandError::runtime(RuntimeFailureKind::Configuration, "Runtime endpoint is empty."));
  }
  let Some(scheme_end) = endpoint.find("://") else {
    return Err(CommandError::runtime(
      RuntimeFailureKind::Configuration,
      "Runtime endpoint must use http:// or https://.",
    ));
  };
  let scheme = endpoint[..scheme_end].to_ascii_lowercase();
  if scheme != "http" && scheme != "https" {
    return Err(CommandError::runtime(
      RuntimeFailureKind::Configuration,
      "Runtime endpoint must use http:// or https://.",
    ));
  }
  let authority_start = scheme_end + 3;
  let authority_and_path = &endpoint[authority_start..];
  if authority_and_path.is_empty() || authority_and_path.starts_with('/') {
    return Err(CommandError::runtime(RuntimeFailureKind::Configuration, "Runtime endpoint host is empty."));
  }
  Ok(())
}

pub(super) fn ai_runtime_failure_kind(error: &AiRuntimeError) -> RuntimeFailureKind {
  match error {
    AiRuntimeError::MissingCredential { .. } | AiRuntimeError::CredentialResolution { .. } => {
      RuntimeFailureKind::Credential
    }
    AiRuntimeError::HttpStatus { status: 401 | 403, .. } => RuntimeFailureKind::Credential,
    AiRuntimeError::HttpStatus { status: 408 | 504, .. } | AiRuntimeError::Timeout { .. } => {
      RuntimeFailureKind::Timeout
    }
    AiRuntimeError::HttpStatus { status: 429, .. } => RuntimeFailureKind::Busy,
    AiRuntimeError::InvalidAgentConfig { .. } => RuntimeFailureKind::Configuration,
    AiRuntimeError::ResponseBodyTooLarge { .. } | AiRuntimeError::InvalidProviderResponse { .. } => {
      RuntimeFailureKind::InvalidResponse
    }
    AiRuntimeError::HttpStatus { .. } | AiRuntimeError::ProviderExecution { .. } => RuntimeFailureKind::Unavailable,
  }
}

fn ai_runtime_error(error: AiRuntimeError) -> CommandError {
  let kind = ai_runtime_failure_kind(&error);
  CommandError::runtime(kind, error.to_string())
}

#[cfg(test)]
fn write_runtime_test_job(job_dir: &Path) {
  fs::create_dir_all(job_dir).unwrap();
  fs::write(job_dir.join("instructions.md"), "Write output_patch.json from the provided job context.").unwrap();
  fs::write(job_dir.join("runtime.json"), r#"{"providerId":"test"}"#).unwrap();
  fs::write(
    job_dir.join("chunks.json"),
    r#"{"schema_version":1,"chunks":[{"chunk_id":"runtime_test_chunk","content":"Runtime test source."}]}"#,
  )
  .unwrap();
  fs::write(job_dir.join("current_graph_snapshot.json"), r#"{"schema_version":1,"nodes":[],"edges":[]}"#).unwrap();
  fs::write(job_dir.join("graph_patch.schema.json"), r#"{"type":"object"}"#).unwrap();
}

#[derive(Debug)]
struct HostedJobPrompt {
  text: String,
  included_chunk_count: usize,
  total_chunk_count: usize,
}

fn hosted_job_prompt(job_dir: &Path) -> CommandResult<HostedJobPrompt> {
  let instructions = fs::read_to_string(required_job_file(job_dir, "instructions.md")?)?;
  let runtime = compact_job_json(job_dir, "runtime.json")?;
  let current_graph = compact_job_json(job_dir, "current_graph_snapshot.json")?;
  let patch_schema = compact_job_json(job_dir, "graph_patch.schema.json")?;
  let chunks_document = read_job_json(job_dir, "chunks.json")?;
  let chunks = chunks_document
    .get("chunks")
    .and_then(Value::as_array)
    .ok_or_else(|| CommandError::validation("chunks.json must contain a chunks array."))?;
  let total_chunk_count = chunks.len();

  let mut prompt = String::from(
    "Complete this Soma graph compile request. All context available to you is embedded below. \
     You cannot access the local job folder. Treat filenames in the instructions as names of matching embedded \
     sections in this message. Return the output_patch.json content in your response; do not try to read or write \
     local files.\n",
  );
  append_prompt_section(&mut prompt, "instructions.md", &instructions);
  append_prompt_section(&mut prompt, "runtime.json", &runtime);
  append_prompt_section(&mut prompt, "current_graph_snapshot.json", &current_graph);
  append_prompt_section(&mut prompt, "graph_patch.schema.json", &patch_schema);
  prompt.push_str(HOSTED_CHUNKS_SECTION);
  prompt.push_str("[\n");

  let empty_tail = hosted_job_prompt_tail(0, total_chunk_count);
  if prompt.len() + empty_tail.len() > JOB_PROMPT_MAX_BYTES {
    return Err(CommandError::validation(format!(
      "Hosted compile instructions, runtime, current graph, and schema exceed the {JOB_PROMPT_MAX_BYTES}-byte \
       request limit."
    )));
  }

  let mut included_chunk_count = 0;
  for chunk in chunks {
    let encoded = serde_json::to_string(chunk).map_err(|error| CommandError::storage(error.to_string()))?;
    let separator = if included_chunk_count == 0 { "" } else { ",\n" };
    let next_tail = hosted_job_prompt_tail(included_chunk_count + 1, total_chunk_count);
    if prompt.len() + separator.len() + encoded.len() + next_tail.len() > JOB_PROMPT_MAX_BYTES {
      break;
    }
    prompt.push_str(separator);
    prompt.push_str(&encoded);
    included_chunk_count += 1;
  }

  if total_chunk_count > 0 && included_chunk_count == 0 {
    return Err(CommandError::validation(format!(
      "Hosted compile context leaves no room for one complete source chunk within the \
       {JOB_PROMPT_MAX_BYTES}-byte request limit."
    )));
  }
  prompt.push_str(&hosted_job_prompt_tail(included_chunk_count, total_chunk_count));
  debug_assert!(prompt.len() <= JOB_PROMPT_MAX_BYTES);
  Ok(HostedJobPrompt { text: prompt, included_chunk_count, total_chunk_count })
}

fn required_job_file(job_dir: &Path, file_name: &str) -> CommandResult<PathBuf> {
  let path = job_dir.join(file_name);
  path
    .exists()
    .then_some(path)
    .ok_or_else(|| CommandError::validation(format!("Compile job is missing required {file_name}.")))
}

fn read_job_json(job_dir: &Path, file_name: &str) -> CommandResult<Value> {
  let content = fs::read_to_string(required_job_file(job_dir, file_name)?)?;
  serde_json::from_str(&content)
    .map_err(|error| CommandError::validation(format!("{file_name} is invalid JSON: {error}")))
}

fn compact_job_json(job_dir: &Path, file_name: &str) -> CommandResult<String> {
  serde_json::to_string(&read_job_json(job_dir, file_name)?).map_err(|error| CommandError::storage(error.to_string()))
}

fn hosted_job_prompt_tail(included_chunk_count: usize, total_chunk_count: usize) -> String {
  format!(
    "\n]\n\nSource chunk coverage in this request: {included_chunk_count} of {total_chunk_count} selected job chunks.\n\
     Return one JSON object matching the embedded graph_patch.schema.json section. \
     Do not wrap it in markdown or try to write a local file.\n"
  )
}

fn profile_job_prompt() -> String {
  [
    "You are Soma's graph compiler.",
    "The current working directory is a Soma compile job folder.",
    "Read instructions.md, runtime.json, input JSON files, current_graph_snapshot.json, and graph_patch.schema.json.",
    "Write only output_patch.json as a valid Soma GraphPatch JSON object.",
    "Treat all graph changes as proposed and untrusted; do not mutate source files or trusted graph state.",
  ]
  .join(" ")
}

fn append_prompt_section(prompt: &mut String, file_name: &str, content: &str) {
  prompt.push_str("\n--- ");
  prompt.push_str(file_name);
  prompt.push_str(" ---\n");
  prompt.push_str(content);
  prompt.push('\n');
}

fn write_extracted_patch(job_dir: &Path, content: &str) -> CommandResult<bool> {
  let Some(patch) = extract_graph_patch(content) else {
    return Ok(false);
  };
  fs::write(
    job_dir.join("output_patch.json"),
    format!("{}\n", serde_json::to_string_pretty(&patch).map_err(|error| CommandError::storage(error.to_string()))?),
  )?;
  Ok(true)
}

fn extract_graph_patch(content: &str) -> Option<Value> {
  if let Ok(value) = serde_json::from_str::<Value>(content.trim()) {
    if is_graph_patch_like(&value) {
      return Some(value);
    }
  }

  let fenced = content
    .split("```")
    .find_map(|part| {
      let trimmed = part.trim().trim_start_matches("json").trim();
      serde_json::from_str::<Value>(trimmed).ok()
    })
    .filter(is_graph_patch_like);
  if fenced.is_some() {
    return fenced;
  }

  let start = content.find('{')?;
  let end = content.rfind('}')?;
  if end <= start || end - start > ADAPTER_OUTPUT_MAX_BYTES {
    return None;
  }
  serde_json::from_str::<Value>(&content[start..=end]).ok().filter(is_graph_patch_like)
}

fn is_graph_patch_like(value: &Value) -> bool {
  value.get("schema_version").and_then(Value::as_i64) == Some(1)
    && value.get("proposed_nodes").is_some()
    && value.get("proposed_edges").is_some()
}

fn output_patch_has_proposals(job_dir: &Path) -> bool {
  let output_path = job_dir.join("output_patch.json");
  let patch = fs::read_to_string(output_path)
    .ok()
    .and_then(|content| serde_json::from_str::<Value>(&content).ok())
    .unwrap_or_else(empty_graph_patch);
  [
    "proposed_nodes",
    "proposed_edges",
    "proposed_node_body_updates",
    "proposed_edge_bridge_updates",
    "proposed_message_evidence_attachments",
    "proposed_paths",
    "ambiguities",
    "merge_candidates",
    "warnings",
  ]
  .iter()
  .filter_map(|field| patch.get(field).and_then(Value::as_array))
  .map(Vec::len)
  .sum::<usize>()
    > 0
}

fn adapter_kind(adapter: &Value) -> String {
  adapter.get("kind").and_then(Value::as_str).unwrap_or("unknown").to_string()
}

fn command_failure_message(code: Option<i32>, stdout: &str, stderr: &str) -> String {
  let mut message = match code {
    Some(code) => format!("Runtime command exited with status {code}."),
    None => "Runtime command was terminated.".to_string(),
  };
  let excerpt = if !stderr.trim().is_empty() { stderr.trim() } else { stdout.trim() };
  if !excerpt.is_empty() {
    message.push(' ');
    message.push_str(&excerpt.chars().take(400).collect::<String>());
  }
  message
}

#[cfg(test)]
#[path = "runtime_adapters_tests.rs"]
mod tests;
