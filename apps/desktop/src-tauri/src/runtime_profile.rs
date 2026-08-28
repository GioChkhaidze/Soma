use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Mutex, MutexGuard, OnceLock, TryLockError};
use std::time::Duration;

use serde_json::{json, Value};
use soma_ai_runtime::{
  AgentTaskCancellation, AgentTaskRequest, AgentTaskResult, AgentTaskStatus, AiRuntimeError, CliAgentConfig,
  CliAgentRuntime, CliPromptMode, ProviderId,
};
use uuid::Uuid;

use super::{
  adapter_kind, ai_runtime_failure_kind, command_failure_message, hosted_job_prompt, output_patch_has_proposals,
  profile_job_prompt, write_extracted_patch, RuntimeRunResult, ADAPTER_OUTPUT_MAX_BYTES,
};
use crate::chat_runtime::{
  chat_turn_prompt, current_chat_user_message, parse_chat_turn_response, RuntimeChatTurnResult,
};
use crate::error::{CommandError, CommandResult, RuntimeFailureKind};

pub(super) const CODEX_RUNTIME_BUSY_MESSAGE: &str =
  "Codex is busy with another Soma request. Wait for it to finish, then try again.";
pub(super) const CODEX_STORAGE_BUSY_MESSAGE: &str =
  "Codex profile storage is busy. Wait for the current Codex run to finish, then try again.";

const PROFILE_RUNTIME_TIMEOUT: Duration = Duration::from_secs(240);
const CHAT_RUNTIME_TIMEOUT: Duration = Duration::from_secs(120);
const CODEX_PROBE_TIMEOUT_MS: u64 = 2_000;
const CODEX_STORAGE_BUSY_RETRY_DELAYS: [Duration; 2] = [Duration::from_millis(250), Duration::from_millis(750)];

pub(super) static CODEX_CLI_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

pub fn codex_brain_status() -> Value {
  let _guard = match codex_cli_guard() {
    Ok(guard) => guard,
    Err(error) => {
      return json!({
        "providerId": "codex_sdk",
        "status": "failed",
        "message": error.message,
        "launcher": "",
        "version": null
      });
    }
  };
  codex_brain_status_unlocked()
}

fn codex_brain_status_unlocked() -> Value {
  let Some((program, args)) = codex_version_command_spec() else {
    return json!({
      "providerId": "codex_sdk",
      "status": "failed",
      "message": "Codex launcher is empty.",
      "launcher": "",
      "version": null
    });
  };

  match run_codex_probe(&program, args.clone(), CODEX_PROBE_TIMEOUT_MS) {
    Ok(output) => {
      let stdout = output.stdout.trim().to_string();
      let stderr = output.stderr.trim().to_string();
      if output.status == AgentTaskStatus::Completed {
        let version = if stdout.is_empty() { stderr } else { stdout };
        match codex_login_status_unlocked() {
          Ok(login_status) => json!({
            "providerId": "codex_sdk",
            "status": "ready",
            "message": if login_status.is_empty() {
              "Codex is ready.".to_string()
            } else {
              format!("Codex is ready. {login_status}")
            },
            "launcher": launcher_label(&program, &args),
            "version": version
          }),
          Err(message) => json!({
            "providerId": "codex_sdk",
            "status": "failed",
            "message": format!("Codex is installed but not authorized. {message}"),
            "launcher": launcher_label(&program, &args),
            "version": version
          }),
        }
      } else {
        json!({
          "providerId": "codex_sdk",
          "status": "failed",
          "message": codex_probe_failure_message("Codex readiness check", &output, CODEX_PROBE_TIMEOUT_MS),
          "launcher": launcher_label(&program, &args),
          "version": null
        })
      }
    }
    Err(error) => json!({
      "providerId": "codex_sdk",
      "status": "failed",
      "message": format!("Could not start Codex runtime launcher `{program}`: {error}"),
      "launcher": launcher_label(&program, &args),
      "version": null
    }),
  }
}

pub fn authorize_codex_brain_status() -> Value {
  let status = codex_brain_status();
  if status.get("status").and_then(Value::as_str) == Some("ready") {
    return status;
  }
  if status.get("version").is_none_or(Value::is_null) {
    return status;
  }

  match launch_codex_login() {
    Ok(()) => json!({
      "providerId": "codex_sdk",
      "status": "pending",
      "message": "Codex authorization opened. Complete sign-in, then click Enable Codex.",
      "launcher": codex_auth_launcher_label(),
      "version": null
    }),
    Err(error) => json!({
      "providerId": "codex_sdk",
      "status": "failed",
      "message": format!("Could not open Codex authorization: {error}"),
      "launcher": codex_auth_launcher_label(),
      "version": null
    }),
  }
}

#[cfg(test)]
pub(super) fn run_profile_chat_turn(
  runtime: &Value,
  adapter: &Value,
  request: &Value,
  command_kind: ProfileCommand,
) -> CommandResult<RuntimeChatTurnResult> {
  run_profile_chat_turn_with_cancellation(runtime, adapter, request, command_kind, AgentTaskCancellation::new())
}

pub(super) fn run_profile_chat_turn_with_cancellation(
  runtime: &Value,
  adapter: &Value,
  request: &Value,
  command_kind: ProfileCommand,
  cancellation: AgentTaskCancellation,
) -> CommandResult<RuntimeChatTurnResult> {
  let adapter_kind = adapter_kind(adapter);
  let mut runtime = runtime.clone();
  if matches!(command_kind, ProfileCommand::Codex) {
    let captures_graph = request.get("capture_graph_changes").and_then(Value::as_bool).unwrap_or(false);
    let effort_key = if captures_graph { "reasoningEffort" } else { "chatReasoningEffort" };
    let fallback = if captures_graph { "xhigh" } else { "medium" };
    let effort =
      runtime.get(effort_key).and_then(Value::as_str).filter(|value| !value.is_empty()).unwrap_or(fallback).to_string();
    runtime["reasoningEffort"] = json!(effort);
  }

  let Some(config) = profile_agent_config(&runtime, adapter, command_kind) else {
    return Ok(RuntimeChatTurnResult {
      adapter_kind,
      status: "failed",
      failure_kind: Some(RuntimeFailureKind::Configuration),
      message: "Runtime command is empty.".to_string(),
      assistant_message: None,
      used_graph_areas: Vec::new(),
      proposed_graph_patch: None,
    });
  };

  let temp_dir = RuntimeTempDir::create(std::env::temp_dir().join(format!("soma-chat-turn-{}", Uuid::new_v4())))?;
  let prompt = chat_turn_prompt(request);
  let program = config.program.clone();
  let output = run_profile_agent_task_with_storage_retry(
    config,
    AgentTaskRequest::new(temp_dir.path(), prompt, CHAT_RUNTIME_TIMEOUT.as_millis() as u64)
      .with_cancellation(cancellation),
    command_kind,
  );
  let final_message = fs::read_to_string(temp_dir.path().join("codex_final_message.txt")).unwrap_or_default();

  let output = match output {
    Ok(output) => output,
    Err(error) => {
      let failure_kind = profile_launch_failure_kind(command_kind, &error);
      return Ok(RuntimeChatTurnResult {
        adapter_kind,
        status: "failed",
        failure_kind: Some(failure_kind),
        message: profile_launch_failure_message(command_kind, &program, error),
        assistant_message: None,
        used_graph_areas: Vec::new(),
        proposed_graph_patch: None,
      });
    }
  };

  let stdout = output.stdout;
  let stderr = output.stderr;
  let stdout_truncated = output.stdout_truncated;
  if output.status == AgentTaskStatus::Cancelled {
    return Ok(RuntimeChatTurnResult {
      adapter_kind,
      status: "cancelled",
      failure_kind: None,
      message: "Stopped by you.".to_string(),
      assistant_message: None,
      used_graph_areas: Vec::new(),
      proposed_graph_patch: None,
    });
  }

  if output.status == AgentTaskStatus::TimedOut {
    return Ok(RuntimeChatTurnResult {
      adapter_kind,
      status: "failed",
      failure_kind: Some(RuntimeFailureKind::Timeout),
      message: format!(
        "Chat runtime timed out after {} seconds. {}",
        CHAT_RUNTIME_TIMEOUT.as_secs(),
        profile_command_failure_message(command_kind, output.exit_code, &stdout, &stderr)
      ),
      assistant_message: None,
      used_graph_areas: Vec::new(),
      proposed_graph_patch: None,
    });
  }
  if output.status != AgentTaskStatus::Completed {
    let message = profile_command_failure_message(command_kind, output.exit_code, &stdout, &stderr);
    return Ok(RuntimeChatTurnResult {
      adapter_kind,
      status: "failed",
      failure_kind: Some(profile_command_failure_kind(&message)),
      message,
      assistant_message: None,
      used_graph_areas: Vec::new(),
      proposed_graph_patch: None,
    });
  }
  if final_message.trim().is_empty() && stdout_truncated {
    let error = profile_response_too_large_error(command_kind);
    return Ok(RuntimeChatTurnResult {
      adapter_kind,
      status: "failed",
      failure_kind: Some(ai_runtime_failure_kind(&error)),
      message: error.to_string(),
      assistant_message: None,
      used_graph_areas: Vec::new(),
      proposed_graph_patch: None,
    });
  }

  let content = if final_message.trim().is_empty() { stdout } else { final_message };
  parse_chat_turn_response(&adapter_kind, &content, current_chat_user_message(request))
}

#[derive(Copy, Clone)]
pub(super) enum ProfileCommand {
  Codex,
  Claude,
}

fn profile_label(command_kind: ProfileCommand) -> &'static str {
  match command_kind {
    ProfileCommand::Codex => "Codex",
    ProfileCommand::Claude => "Claude Code",
  }
}

fn profile_response_too_large_error(command_kind: ProfileCommand) -> AiRuntimeError {
  let provider = match command_kind {
    ProfileCommand::Codex => ProviderId::from("codex_sdk"),
    ProfileCommand::Claude => ProviderId::from("claude_code"),
  };
  AiRuntimeError::ResponseBodyTooLarge { provider, limit_bytes: ADAPTER_OUTPUT_MAX_BYTES as u64 }
}

fn agent_error_message(program: &str, error: AiRuntimeError) -> String {
  match error {
    AiRuntimeError::ProviderExecution { message, .. } => message,
    error => format!("Could not start runtime command `{program}`: {error}"),
  }
}

pub(super) fn profile_launch_failure_message(
  command_kind: ProfileCommand,
  program: &str,
  error: AiRuntimeError,
) -> String {
  let detail = profile_failure_detail(command_kind, &agent_error_message(program, error));
  if is_profile_busy_message(&detail) {
    return detail;
  }
  let label = profile_label(command_kind);
  format!(
    "{label} runtime could not start. Open Brain Settings and enable a working {label} runtime. Details: {detail}"
  )
}

pub(super) fn profile_command_failure_message(
  command_kind: ProfileCommand,
  code: Option<i32>,
  stdout: &str,
  stderr: &str,
) -> String {
  profile_failure_detail(command_kind, &command_failure_message(code, stdout, stderr))
}

fn profile_failure_detail(command_kind: ProfileCommand, message: &str) -> String {
  let message = message.trim();
  if matches!(command_kind, ProfileCommand::Codex) {
    if is_codex_storage_locked_message(message) {
      return CODEX_STORAGE_BUSY_MESSAGE.to_string();
    }
    if message.contains(CODEX_RUNTIME_BUSY_MESSAGE) {
      return CODEX_RUNTIME_BUSY_MESSAGE.to_string();
    }
  }
  message.to_string()
}

fn is_profile_busy_message(message: &str) -> bool {
  message == CODEX_RUNTIME_BUSY_MESSAGE || message == CODEX_STORAGE_BUSY_MESSAGE
}

fn profile_launch_failure_kind(command_kind: ProfileCommand, error: &AiRuntimeError) -> RuntimeFailureKind {
  let detail = profile_failure_detail(command_kind, &agent_error_detail(error));
  if is_profile_busy_message(&detail) {
    RuntimeFailureKind::Busy
  } else {
    ai_runtime_failure_kind(error)
  }
}

fn profile_command_failure_kind(message: &str) -> RuntimeFailureKind {
  if is_profile_busy_message(message) {
    RuntimeFailureKind::Busy
  } else {
    RuntimeFailureKind::Execution
  }
}

fn is_codex_storage_locked_message(message: &str) -> bool {
  let message = message.to_ascii_lowercase();
  message.contains("sqlite_busy")
    || message.contains("sqlite_locked")
    || message.contains("database is locked")
    || message.contains("database table is locked")
    || message.contains("database schema is locked")
    || message.contains("write lock was poisoned")
    || (message.contains("sqlite") && message.contains("locked"))
}

pub(super) fn run_profile_command(
  job_dir: &Path,
  runtime: &Value,
  adapter: &Value,
  command_kind: ProfileCommand,
) -> CommandResult<RuntimeRunResult> {
  let prompt = match command_kind {
    ProfileCommand::Codex => profile_job_prompt(),
    ProfileCommand::Claude => hosted_job_prompt(job_dir)?.text,
  };
  let Some(config) = profile_agent_config(runtime, adapter, command_kind) else {
    return Ok(RuntimeRunResult {
      adapter_kind: adapter_kind(adapter),
      status: "failed",
      failure_kind: Some(RuntimeFailureKind::Configuration),
      message: "Runtime command is empty.".to_string(),
      wrote_output_patch: false,
    });
  };

  let program = config.program.clone();
  let output = match run_profile_agent_task_with_storage_retry(
    config,
    AgentTaskRequest::new(job_dir, prompt, PROFILE_RUNTIME_TIMEOUT.as_millis() as u64),
    command_kind,
  ) {
    Ok(output) => output,
    Err(error) => {
      let failure_kind = profile_launch_failure_kind(command_kind, &error);
      return Ok(RuntimeRunResult {
        adapter_kind: adapter_kind(adapter),
        status: "failed",
        failure_kind: Some(failure_kind),
        message: profile_launch_failure_message(command_kind, &program, error),
        wrote_output_patch: false,
      });
    }
  };

  let stdout = output.stdout;
  let stderr = output.stderr;
  let stdout_truncated = output.stdout_truncated;
  let final_message = fs::read_to_string(job_dir.join("codex_final_message.txt")).unwrap_or_default();
  let wrote_stdout_patch =
    (!stdout_truncated && write_extracted_patch(job_dir, &stdout)?) || write_extracted_patch(job_dir, &final_message)?;
  let output_patch_ready = output_patch_has_proposals(job_dir);
  if output.status == AgentTaskStatus::TimedOut {
    return Ok(RuntimeRunResult {
      adapter_kind: adapter_kind(adapter),
      status: "failed",
      failure_kind: Some(RuntimeFailureKind::Timeout),
      message: format!(
        "Runtime command timed out after {} seconds. {}",
        PROFILE_RUNTIME_TIMEOUT.as_secs(),
        profile_command_failure_message(command_kind, output.exit_code, &stdout, &stderr)
      ),
      wrote_output_patch: wrote_stdout_patch || output_patch_ready,
    });
  }
  if output.status == AgentTaskStatus::Completed && (wrote_stdout_patch || output_patch_ready) {
    return Ok(RuntimeRunResult {
      adapter_kind: adapter_kind(adapter),
      status: "completed",
      failure_kind: None,
      message: "Runtime command completed and output_patch.json is ready.".to_string(),
      wrote_output_patch: wrote_stdout_patch || output_patch_ready,
    });
  }
  if output.status == AgentTaskStatus::Completed && stdout_truncated {
    let error = profile_response_too_large_error(command_kind);
    return Ok(RuntimeRunResult {
      adapter_kind: adapter_kind(adapter),
      status: "failed",
      failure_kind: Some(ai_runtime_failure_kind(&error)),
      message: error.to_string(),
      wrote_output_patch: false,
    });
  }

  let message = profile_command_failure_message(command_kind, output.exit_code, &stdout, &stderr);
  Ok(RuntimeRunResult {
    adapter_kind: adapter_kind(adapter),
    status: "failed",
    failure_kind: Some(if output.status == AgentTaskStatus::Completed {
      RuntimeFailureKind::InvalidResponse
    } else {
      profile_command_failure_kind(&message)
    }),
    message,
    wrote_output_patch: wrote_stdout_patch,
  })
}

pub(super) fn profile_agent_config(
  runtime: &Value,
  adapter: &Value,
  command_kind: ProfileCommand,
) -> Option<CliAgentConfig> {
  let (program, mut args) = profile_command_spec(runtime, adapter, command_kind)?;
  let prompt_mode = match command_kind {
    ProfileCommand::Codex => {
      args.push("-".to_string());
      CliPromptMode::Stdin
    }
    ProfileCommand::Claude => {
      args.extend([
        "--tools".to_string(),
        String::new(),
        "--strict-mcp-config".to_string(),
        "--no-session-persistence".to_string(),
      ]);
      CliPromptMode::Stdin
    }
  };
  let provider_id = match command_kind {
    ProfileCommand::Codex => ProviderId::from("codex_sdk"),
    ProfileCommand::Claude => ProviderId::from("claude_code"),
  };

  Some(CliAgentConfig {
    provider_id,
    program,
    args,
    prompt_mode,
    path_prepend: profile_path_prepend(),
    env: Vec::new(),
    max_output_bytes: ADAPTER_OUTPUT_MAX_BYTES,
  })
}

pub(super) fn run_profile_agent_task(
  mut config: CliAgentConfig,
  request: AgentTaskRequest,
  command_kind: ProfileCommand,
) -> Result<AgentTaskResult, AiRuntimeError> {
  if matches!(command_kind, ProfileCommand::Codex) {
    let _sqlite_home = configure_codex_runtime_env(&mut config).map_err(|error| AiRuntimeError::ProviderExecution {
      provider: config.provider_id.clone(),
      message: error.message,
    })?;
    let provider = config.provider_id.clone();
    let _guard =
      codex_cli_guard().map_err(|error| AiRuntimeError::ProviderExecution { provider, message: error.message })?;
    return CliAgentRuntime::new(config).run_task(request);
  }
  CliAgentRuntime::new(config).run_task(request)
}

fn run_profile_agent_task_with_storage_retry(
  config: CliAgentConfig,
  request: AgentTaskRequest,
  command_kind: ProfileCommand,
) -> Result<AgentTaskResult, AiRuntimeError> {
  let mut result = run_profile_agent_task(config.clone(), request.clone(), command_kind);
  for delay in CODEX_STORAGE_BUSY_RETRY_DELAYS {
    if !profile_agent_task_storage_busy(command_kind, &result) {
      return result;
    }
    std::thread::sleep(delay);
    result = run_profile_agent_task(config.clone(), request.clone(), command_kind);
  }
  result
}

pub(super) fn profile_agent_task_storage_busy(
  command_kind: ProfileCommand,
  result: &Result<AgentTaskResult, AiRuntimeError>,
) -> bool {
  if !matches!(command_kind, ProfileCommand::Codex) {
    return false;
  }
  match result {
    Ok(output) => {
      let message = command_failure_message(output.exit_code, &output.stdout, &output.stderr);
      is_codex_storage_locked_message(&message)
    }
    Err(error) => is_codex_storage_locked_message(&agent_error_detail(error)),
  }
}

fn agent_error_detail(error: &AiRuntimeError) -> String {
  match error {
    AiRuntimeError::ProviderExecution { message, .. } => message.clone(),
    error => error.to_string(),
  }
}

pub(super) fn codex_cli_guard() -> CommandResult<MutexGuard<'static, ()>> {
  match CODEX_CLI_LOCK.get_or_init(|| Mutex::new(())).try_lock() {
    Ok(guard) => Ok(guard),
    Err(TryLockError::WouldBlock) => Err(CommandError::storage(CODEX_RUNTIME_BUSY_MESSAGE)),
    Err(TryLockError::Poisoned(_)) => Err(CommandError::storage("Codex runtime lock was poisoned.")),
  }
}

pub(super) fn configure_codex_runtime_env(config: &mut CliAgentConfig) -> CommandResult<RuntimeTempDir> {
  let sqlite_home = codex_sqlite_home()?;
  upsert_env(&mut config.env, "CODEX_SQLITE_HOME", sqlite_home.path().to_string_lossy().as_ref());
  upsert_env(&mut config.env, "CODEX_NON_INTERACTIVE", "1");
  Ok(sqlite_home)
}

fn codex_sqlite_home() -> std::io::Result<RuntimeTempDir> {
  let path = std::env::temp_dir().join(format!("soma-codex-sqlite-{}-{}", std::process::id(), Uuid::new_v4()));
  RuntimeTempDir::create(path)
}

#[must_use = "the guard must stay alive while the runtime uses its temporary directory"]
pub(super) struct RuntimeTempDir {
  path: PathBuf,
}

impl RuntimeTempDir {
  pub(super) fn create(path: PathBuf) -> std::io::Result<Self> {
    fs::create_dir_all(&path)?;
    Ok(Self { path })
  }

  pub(super) fn path(&self) -> &Path {
    &self.path
  }
}

impl Drop for RuntimeTempDir {
  fn drop(&mut self) {
    let _ = fs::remove_dir_all(&self.path);
  }
}

fn upsert_env(env_vars: &mut Vec<(String, String)>, name: &str, value: &str) {
  if let Some((_, existing)) = env_vars.iter_mut().find(|(key, _)| key == name) {
    *existing = value.to_string();
    return;
  }
  env_vars.push((name.to_string(), value.to_string()));
}

pub(super) fn profile_command_spec(
  runtime: &Value,
  adapter: &Value,
  command_kind: ProfileCommand,
) -> Option<(String, Vec<String>)> {
  let env_name = match command_kind {
    ProfileCommand::Codex => "SOMA_CODEX_COMMAND",
    ProfileCommand::Claude => "SOMA_CLAUDE_COMMAND",
  };
  if let Ok(value) = std::env::var(env_name) {
    return split_command(&value);
  }

  let model = runtime.get("model").and_then(Value::as_str).unwrap_or("").trim();
  let reasoning_effort = runtime.get("reasoningEffort").and_then(Value::as_str).unwrap_or("").trim();
  let profile = adapter.get("profile").and_then(Value::as_str).unwrap_or("default");

  match command_kind {
    ProfileCommand::Codex => {
      let mut args = vec![
        "exec".to_string(),
        "--skip-git-repo-check".to_string(),
        "--ephemeral".to_string(),
        "--sandbox".to_string(),
        "workspace-write".to_string(),
        "--color".to_string(),
        "never".to_string(),
        "--output-last-message".to_string(),
        "codex_final_message.txt".to_string(),
      ];
      if profile != "default" {
        args.push("--profile".to_string());
        args.push(profile.to_string());
      }
      if !model.is_empty() {
        args.push("--model".to_string());
        args.push(model.to_string());
      }
      if matches!(reasoning_effort, "none" | "low" | "medium" | "high" | "xhigh" | "max") {
        args.push("--config".to_string());
        args.push(format!("model_reasoning_effort=\"{reasoning_effort}\""));
      }
      Some(codex_launcher_command(args))
    }
    ProfileCommand::Claude => {
      let mut args = Vec::new();
      if !model.is_empty() {
        args.push("--model".to_string());
        args.push(model.to_string());
      }
      args.push("-p".to_string());
      Some(("claude".to_string(), args))
    }
  }
}

fn codex_version_command_spec() -> Option<(String, Vec<String>)> {
  if let Ok(value) = std::env::var("SOMA_CODEX_COMMAND") {
    let (program, mut args) = split_command(&value)?;
    args.push("--version".to_string());
    return Some((program, args));
  }
  Some(codex_launcher_command(vec!["--version".to_string()]))
}

pub(super) fn codex_login_status_command_spec() -> Option<(String, Vec<String>)> {
  if let Ok(value) = std::env::var("SOMA_CODEX_COMMAND") {
    let (program, mut args) = split_command(&value)?;
    args.push("login".to_string());
    args.push("status".to_string());
    return Some((program, args));
  }
  Some(codex_launcher_command(vec!["login".to_string(), "status".to_string()]))
}

fn codex_login_status_unlocked() -> Result<String, String> {
  let Some((program, args)) = codex_login_status_command_spec() else {
    return Err("Codex launcher is empty.".to_string());
  };
  let output = run_codex_probe(&program, args, CODEX_PROBE_TIMEOUT_MS)
    .map_err(|error| format!("Could not check Codex login status: {error}"))?;

  let stdout = output.stdout.trim().to_string();
  let stderr = output.stderr.trim().to_string();
  if output.status == AgentTaskStatus::Completed {
    Ok(if stdout.is_empty() { stderr } else { stdout })
  } else {
    Err(codex_probe_failure_message("Codex login status check", &output, CODEX_PROBE_TIMEOUT_MS))
  }
}

pub(super) fn run_codex_probe(program: &str, args: Vec<String>, timeout_ms: u64) -> Result<AgentTaskResult, String> {
  let mut config = CliAgentConfig::new(ProviderId::from("codex_sdk"), program.to_string(), args, CliPromptMode::Stdin);
  config.path_prepend = profile_path_prepend();
  config.max_output_bytes = ADAPTER_OUTPUT_MAX_BYTES;
  let _sqlite_home = configure_codex_runtime_env(&mut config)
    .map_err(|error| format!("Could not prepare isolated Codex SQLite state: {}", error.message))?;
  // A timed-out Windows process tree can retain its cwd after the launcher exits.
  let output =
    CliAgentRuntime::new(config).run_task(AgentTaskRequest::new(env::temp_dir(), "", timeout_ms)).map_err(|error| {
      let message = agent_error_message(program, error);
      if is_codex_storage_locked_message(&message) {
        CODEX_STORAGE_BUSY_MESSAGE.to_string()
      } else {
        message
      }
    })?;
  if output.stdout_truncated || output.stderr_truncated {
    return Err(profile_response_too_large_error(ProfileCommand::Codex).to_string());
  }
  Ok(output)
}

pub(super) fn codex_probe_failure_message(label: &str, output: &AgentTaskResult, timeout_ms: u64) -> String {
  let stdout = output.stdout.trim();
  let stderr = output.stderr.trim();
  let command_message = command_failure_message(output.exit_code, stdout, stderr);
  if is_codex_storage_locked_message(&command_message) {
    return CODEX_STORAGE_BUSY_MESSAGE.to_string();
  }
  if output.status == AgentTaskStatus::TimedOut {
    format!("{label} timed out after {timeout_ms} ms. {command_message}")
  } else {
    command_message
  }
}

#[cfg(windows)]
fn launch_codex_login() -> std::io::Result<()> {
  let (program, args) = codex_launcher_command(vec!["login".to_string(), "--device-auth".to_string()]);
  let login_command = format!("start \"Soma Codex Login\" cmd.exe /K {}", windows_shell_command(&program, &args));
  let mut command = Command::new("cmd.exe");
  command.args(["/D", "/C", login_command.as_str()]);
  add_windows_execution_alias_path(&mut command);
  command.spawn().map(|_| ())
}

#[cfg(target_os = "macos")]
fn launch_codex_login() -> std::io::Result<()> {
  let (program, args) = codex_launcher_command(vec!["login".to_string(), "--device-auth".to_string()]);
  let login_command = posix_shell_command(&program, &args);
  let script = format!("tell application \"Terminal\" to do script {}", apple_script_string(&login_command));
  Command::new("osascript")
    .args(["-e", "tell application \"Terminal\" to activate", "-e", script.as_str()])
    .spawn()
    .map(|_| ())
}

#[cfg(target_os = "linux")]
fn launch_codex_login() -> std::io::Result<()> {
  let (program, args) = codex_launcher_command(vec!["login".to_string(), "--device-auth".to_string()]);
  let terminals: [(&str, &[&str]); 5] = [
    ("xdg-terminal-exec", &[]),
    ("x-terminal-emulator", &["-e"]),
    ("gnome-terminal", &["--"]),
    ("konsole", &["-e"]),
    ("xterm", &["-e"]),
  ];
  let mut last_error = None;

  for (terminal, terminal_args) in terminals {
    let mut command = Command::new(terminal);
    command.args(terminal_args).arg(&program).args(&args);
    match command.spawn() {
      Ok(_) => return Ok(()),
      Err(error) => last_error = Some(error),
    }
  }

  Err(last_error.unwrap_or_else(|| std::io::Error::other("No supported terminal launcher was found.")))
}

#[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
fn launch_codex_login() -> std::io::Result<()> {
  let (program, args) = codex_launcher_command(vec!["login".to_string(), "--device-auth".to_string()]);
  Command::new(program).args(args).spawn().map(|_| ())
}

#[cfg(windows)]
fn codex_auth_launcher_label() -> &'static str {
  "Windows Codex launcher"
}

#[cfg(target_os = "macos")]
fn codex_auth_launcher_label() -> &'static str {
  "macOS Terminal"
}

#[cfg(target_os = "linux")]
fn codex_auth_launcher_label() -> &'static str {
  "Linux terminal"
}

#[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
fn codex_auth_launcher_label() -> &'static str {
  "system terminal"
}

pub(super) fn codex_launcher_command(args: Vec<String>) -> (String, Vec<String>) {
  #[cfg(windows)]
  {
    if let Some(program) = discover_user_codex_executable() {
      return (program.to_string_lossy().to_string(), args);
    }
    let mut shell_args = vec!["/D".to_string(), "/C".to_string(), "codex".to_string()];
    shell_args.extend(args);
    ("cmd.exe".to_string(), shell_args)
  }

  #[cfg(not(windows))]
  {
    ("codex".to_string(), args)
  }
}

#[cfg(windows)]
fn profile_path_prepend() -> Vec<PathBuf> {
  let mut paths = user_codex_bin_dirs();
  if let Some(local_app_data) = env::var_os("LOCALAPPDATA") {
    let windows_apps = PathBuf::from(local_app_data).join("Microsoft").join("WindowsApps");
    if !paths.iter().any(|path| path == &windows_apps) {
      paths.push(windows_apps);
    }
  }
  paths
}

#[cfg(not(windows))]
fn profile_path_prepend() -> Vec<PathBuf> {
  Vec::new()
}

#[cfg(target_os = "macos")]
fn posix_shell_command(program: &str, args: &[String]) -> String {
  std::iter::once(program).chain(args.iter().map(String::as_str)).map(posix_shell_word).collect::<Vec<_>>().join(" ")
}

#[cfg(target_os = "macos")]
fn posix_shell_word(value: &str) -> String {
  format!("'{}'", value.replace('\'', "'\"'\"'"))
}

#[cfg(target_os = "macos")]
fn apple_script_string(value: &str) -> String {
  format!("\"{}\"", value.replace('\\', "\\\\").replace('"', "\\\""))
}

#[cfg(windows)]
fn add_windows_execution_alias_path(command: &mut Command) {
  let mut paths: Vec<PathBuf> = env::var_os("PATH").map(|value| env::split_paths(&value).collect()).unwrap_or_default();

  for codex_bin_dir in user_codex_bin_dirs() {
    paths.retain(|path| path != &codex_bin_dir);
    paths.insert(0, codex_bin_dir);
  }

  if let Some(local_app_data) = env::var_os("LOCALAPPDATA") {
    let windows_apps = PathBuf::from(local_app_data).join("Microsoft").join("WindowsApps");
    let already_present = paths.iter().any(|path| path == &windows_apps);
    if !already_present {
      paths.push(windows_apps);
    }
  }

  if let Ok(joined) = env::join_paths(paths) {
    command.env("PATH", joined);
  }
}

#[cfg(not(windows))]
fn add_windows_execution_alias_path(_command: &mut Command) {}

#[cfg(windows)]
fn discover_user_codex_executable() -> Option<PathBuf> {
  user_codex_bin_dirs().into_iter().map(|path| path.join("codex.exe")).find(|path| path.is_file())
}

#[cfg(windows)]
fn user_codex_bin_dirs() -> Vec<PathBuf> {
  let Some(local_app_data) = env::var_os("LOCALAPPDATA") else {
    return Vec::new();
  };
  let bin_root = PathBuf::from(local_app_data).join("OpenAI").join("Codex").join("bin");
  let mut paths: Vec<PathBuf> = fs::read_dir(bin_root)
    .ok()
    .into_iter()
    .flatten()
    .filter_map(Result::ok)
    .map(|entry| entry.path())
    .filter(|path| path.join("codex.exe").is_file())
    .collect();
  paths.sort_by(|left, right| right.cmp(left));
  paths
}

fn launcher_label(program: &str, args: &[String]) -> String {
  if program.eq_ignore_ascii_case("cmd.exe") && args.iter().any(|arg| arg == "codex") {
    "Windows Codex launcher".to_string()
  } else if program.to_ascii_lowercase().ends_with("codex.exe") {
    "Codex executable".to_string()
  } else {
    program.to_string()
  }
}

#[cfg(windows)]
pub(super) fn windows_shell_command(program: &str, args: &[String]) -> String {
  std::iter::once(program).chain(args.iter().map(String::as_str)).map(windows_shell_arg).collect::<Vec<_>>().join(" ")
}

#[cfg(windows)]
fn windows_shell_arg(value: &str) -> String {
  if value.is_empty() {
    return "\"\"".to_string();
  }
  if value.chars().any(|char| char.is_whitespace() || matches!(char, '"' | '&' | '|' | '<' | '>' | '^')) {
    return format!("\"{}\"", value.replace('"', "\"\""));
  }
  value.to_string()
}

pub(super) fn split_command(value: &str) -> Option<(String, Vec<String>)> {
  let mut parts = Vec::new();
  let mut current = String::new();
  let mut quote = None;

  for ch in value.chars() {
    match (quote, ch) {
      (Some(active), ch) if ch == active => quote = None,
      (Some(_), ch) => current.push(ch),
      (None, '"' | '\'') => quote = Some(ch),
      (None, ch) if ch.is_whitespace() => {
        if !current.is_empty() {
          parts.push(std::mem::take(&mut current));
        }
      }
      (None, ch) => current.push(ch),
    }
  }

  if !current.is_empty() {
    parts.push(current);
  }

  let program = parts.first()?.to_string();
  Some((program, parts.into_iter().skip(1).collect()))
}
