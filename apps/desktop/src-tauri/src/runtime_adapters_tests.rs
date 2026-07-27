#[cfg(windows)]
use super::runtime_profile::windows_shell_command;
use super::runtime_profile::{
  codex_cli_guard, codex_launcher_command, codex_login_status_command_spec, codex_probe_failure_message,
  configure_codex_runtime_env, profile_agent_config, profile_agent_task_storage_busy, profile_command_failure_message,
  profile_command_spec, profile_launch_failure_message, run_codex_probe, run_profile_agent_task, run_profile_chat_turn,
  run_profile_command, split_command, ProfileCommand, RuntimeTempDir, CODEX_CLI_LOCK, CODEX_RUNTIME_BUSY_MESSAGE,
  CODEX_STORAGE_BUSY_MESSAGE,
};
use super::*;
use crate::database::open_existing_database;
use crate::jobs::create_graph_extraction_job_with_runtime;
use crate::source_import::import_source_file;
use crate::workspace::create_workspace_dir;
use soma_ai_runtime::AgentTaskResult;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::Mutex;
use std::thread;
use std::time::{Duration, Instant};

#[test]
fn ai_runtime_errors_map_to_stable_command_failure_contracts() {
  let provider = ProviderId::from("fixture");
  let cases = [
    (
      AiRuntimeError::MissingCredential {
        credential: CredentialRef::ApiKey { provider: provider.clone(), profile: "default".to_string() },
      },
      RuntimeFailureKind::Credential,
      "SOMA_RUNTIME_CREDENTIAL",
    ),
    (
      AiRuntimeError::Timeout { provider: provider.clone(), message: "request timed out".to_string() },
      RuntimeFailureKind::Timeout,
      "SOMA_RUNTIME_TIMEOUT",
    ),
    (
      AiRuntimeError::InvalidProviderResponse {
        provider: provider.clone(),
        message: "response was not JSON".to_string(),
      },
      RuntimeFailureKind::InvalidResponse,
      "SOMA_RUNTIME_INVALID_RESPONSE",
    ),
    (
      AiRuntimeError::ResponseBodyTooLarge { provider: provider.clone(), limit_bytes: 180_000 },
      RuntimeFailureKind::InvalidResponse,
      "SOMA_RUNTIME_INVALID_RESPONSE",
    ),
    (
      AiRuntimeError::ProviderExecution { provider, message: "connection refused".to_string() },
      RuntimeFailureKind::Unavailable,
      "SOMA_RUNTIME_UNAVAILABLE",
    ),
  ];

  for (runtime_error, expected_kind, expected_code) in cases {
    let error = ai_runtime_error(runtime_error);
    assert_eq!(error.kind, Some(expected_kind));
    assert_eq!(error.code, expected_code);
  }
}

#[test]
fn codex_profile_descriptor_uses_profile_without_credentials() {
  let settings = BrainSettings {
    provider_id: "codex_sdk".to_string(),
    model: "gpt-5.4".to_string(),
    endpoint: String::new(),
    auth_profile: "work".to_string(),
    credential_configured: false,
    updated_at: None,
  };

  let descriptor = runtime_descriptor(&settings);
  assert_eq!(descriptor["adapter"]["kind"], "codex_sdk_profile");
  assert_eq!(descriptor["adapter"]["profile"], "work");
  assert_eq!(descriptor["authProfile"], "work");
  assert!(descriptor.get("apiKey").is_none());
}

#[test]
fn claude_code_descriptor_ignores_legacy_auth_profile() {
  let settings = BrainSettings {
    provider_id: "claude_code".to_string(),
    model: "sonnet".to_string(),
    endpoint: String::new(),
    auth_profile: "legacy-profile".to_string(),
    credential_configured: false,
    updated_at: None,
  };

  let descriptor = runtime_descriptor(&settings);

  assert_eq!(descriptor["adapter"]["kind"], "claude_code_profile");
  assert_eq!(descriptor["adapter"]["model"], "sonnet");
  assert!(descriptor.get("authProfile").is_none());
  assert!(descriptor["adapter"].get("profile").is_none());
}

#[test]
fn api_provider_descriptor_is_redacted_and_configured() {
  let settings = BrainSettings {
    provider_id: "openai".to_string(),
    model: "gpt-test".to_string(),
    endpoint: "https://api.example.test".to_string(),
    auth_profile: String::new(),
    credential_configured: true,
    updated_at: None,
  };

  let descriptor = runtime_descriptor(&settings);
  assert_eq!(descriptor["adapter"]["kind"], "api_provider");
  assert_eq!(descriptor["adapter"]["status"], "configured");
  assert_eq!(descriptor["adapter"]["transport"], "openai_compatible_http");
  assert_eq!(descriptor["credentialConfigured"], true);
  assert!(descriptor.get("authProfile").is_none());
  assert!(descriptor.get("apiKey").is_none());
}

#[test]
fn openrouter_descriptor_uses_default_compatible_endpoint() {
  let settings = BrainSettings {
    provider_id: "openrouter".to_string(),
    model: "openai/gpt-5.2".to_string(),
    endpoint: String::new(),
    auth_profile: String::new(),
    credential_configured: true,
    updated_at: None,
  };

  let descriptor = runtime_descriptor(&settings);

  assert_eq!(descriptor["adapter"]["kind"], "api_provider");
  assert_eq!(descriptor["adapter"]["endpoint"], "https://openrouter.ai/api/v1");
  assert_eq!(descriptor["adapter"]["status"], "configured");
}

#[test]
fn codex_runtime_messages_hide_sqlite_lock_detail() {
  assert_eq!(
    profile_command_failure_message(ProfileCommand::Codex, Some(1), "", "Error: database is locked"),
    CODEX_STORAGE_BUSY_MESSAGE
  );
  assert_eq!(
    profile_launch_failure_message(
      ProfileCommand::Codex,
      "codex",
      AiRuntimeError::ProviderExecution {
        provider: ProviderId::from("codex_sdk"),
        message: "sqlite database is locked".to_string(),
      },
    ),
    CODEX_STORAGE_BUSY_MESSAGE
  );
  assert_eq!(
    codex_probe_failure_message(
      "Codex readiness check",
      &AgentTaskResult {
        status: AgentTaskStatus::Failed,
        stdout: "".to_string(),
        stdout_truncated: false,
        stderr: "SQLITE_LOCKED: database schema is locked".to_string(),
        stderr_truncated: false,
        exit_code: Some(1),
      },
      2_000,
    ),
    CODEX_STORAGE_BUSY_MESSAGE
  );
  assert!(profile_command_failure_message(ProfileCommand::Claude, Some(1), "", "Error: database is locked")
    .contains("database is locked"));
}

#[test]
fn codex_runtime_retries_only_storage_lock_failures() {
  let locked = Ok(AgentTaskResult {
    status: AgentTaskStatus::Failed,
    stdout: "".to_string(),
    stdout_truncated: false,
    stderr: "Error: database is locked".to_string(),
    stderr_truncated: false,
    exit_code: Some(1),
  });
  let normal_failure = Ok(AgentTaskResult {
    status: AgentTaskStatus::Failed,
    stdout: "".to_string(),
    stdout_truncated: false,
    stderr: "Error: model not found".to_string(),
    stderr_truncated: false,
    exit_code: Some(1),
  });
  let launch_locked = Err(AiRuntimeError::ProviderExecution {
    provider: ProviderId::from("codex_sdk"),
    message: "SQLITE_BUSY: database is locked".to_string(),
  });

  assert!(profile_agent_task_storage_busy(ProfileCommand::Codex, &locked));
  assert!(profile_agent_task_storage_busy(ProfileCommand::Codex, &launch_locked));
  assert!(!profile_agent_task_storage_busy(ProfileCommand::Codex, &normal_failure));
  assert!(!profile_agent_task_storage_busy(ProfileCommand::Claude, &locked));
}

#[test]
fn codex_cli_guard_fails_fast_when_another_request_is_running() {
  let _env_guard = RUNTIME_ENV_LOCK.lock().unwrap();
  let held_guard = CODEX_CLI_LOCK.get_or_init(|| Mutex::new(())).lock().unwrap();
  let started = Instant::now();
  let error = match codex_cli_guard() {
    Ok(_) => panic!("Codex guard should not wait behind an active request."),
    Err(error) => error,
  };

  assert_eq!(error.message, CODEX_RUNTIME_BUSY_MESSAGE);
  assert!(started.elapsed() < Duration::from_millis(100));
  drop(held_guard);
}

#[test]
fn claude_descriptor_uses_anthropic_messages_endpoint() {
  let settings = BrainSettings {
    provider_id: "claude".to_string(),
    model: "claude-sonnet-5".to_string(),
    endpoint: String::new(),
    auth_profile: String::new(),
    credential_configured: true,
    updated_at: None,
  };

  let descriptor = runtime_descriptor(&settings);

  assert_eq!(descriptor["adapter"]["kind"], "anthropic_messages_provider");
  assert_eq!(descriptor["adapter"]["transport"], "anthropic_messages_http");
  assert_eq!(descriptor["adapter"]["endpoint"], "https://api.anthropic.com/v1");
  assert_eq!(descriptor["adapter"]["status"], "configured");
}

#[test]
fn ollama_descriptor_uses_registry_endpoint() {
  let settings = BrainSettings {
    provider_id: "ollama".to_string(),
    model: "llama3.3".to_string(),
    endpoint: String::new(),
    auth_profile: String::new(),
    credential_configured: false,
    updated_at: None,
  };

  let descriptor = runtime_descriptor(&settings);

  assert_eq!(descriptor["adapter"]["kind"], "local_offline_endpoint");
  assert_eq!(descriptor["adapter"]["endpoint"], "http://localhost:11434/v1");
  assert_eq!(descriptor["adapter"]["status"], "configured");
}

#[test]
fn deepseek_descriptor_uses_full_chat_endpoint_default() {
  let settings = BrainSettings {
    provider_id: "deepseek".to_string(),
    model: "deepseek-chat".to_string(),
    endpoint: String::new(),
    auth_profile: String::new(),
    credential_configured: true,
    updated_at: None,
  };

  let descriptor = runtime_descriptor(&settings);

  assert_eq!(descriptor["adapter"]["kind"], "api_provider");
  assert_eq!(descriptor["adapter"]["endpoint"], "https://api.deepseek.com/chat/completions");
  assert_eq!(descriptor["adapter"]["status"], "configured");
}

#[test]
fn vercel_gateway_descriptor_uses_standard_v1_base_url() {
  let settings = BrainSettings {
    provider_id: "vercel_ai_gateway".to_string(),
    model: "xai/grok-test".to_string(),
    endpoint: String::new(),
    auth_profile: String::new(),
    credential_configured: true,
    updated_at: None,
  };

  let descriptor = runtime_descriptor(&settings);

  assert_eq!(descriptor["adapter"]["kind"], "api_provider");
  assert_eq!(descriptor["adapter"]["endpoint"], "https://ai-gateway.vercel.sh/v1");
  assert_eq!(descriptor["adapter"]["status"], "configured");
}

#[test]
fn zai_descriptor_uses_current_compatible_endpoint() {
  let settings = BrainSettings {
    provider_id: "zai".to_string(),
    model: "glm-5.2".to_string(),
    endpoint: String::new(),
    auth_profile: String::new(),
    credential_configured: true,
    updated_at: None,
  };

  let descriptor = runtime_descriptor(&settings);

  assert_eq!(descriptor["adapter"]["kind"], "api_provider");
  assert_eq!(descriptor["adapter"]["endpoint"], "https://api.z.ai/api/paas/v4");
  assert_eq!(descriptor["adapter"]["status"], "configured");
}

#[test]
fn split_command_preserves_quoted_program_and_arguments() {
  let (program, args) =
    split_command(r#""C:\Program Files\Soma Runtime\codex.exe" exec --profile "work profile""#).unwrap();

  assert_eq!(program, r#"C:\Program Files\Soma Runtime\codex.exe"#);
  assert_eq!(args, vec!["exec", "--profile", "work profile"]);
}

#[test]
fn codex_launcher_uses_windows_alias_shell_on_windows() {
  let _guard = RUNTIME_ENV_LOCK.lock().unwrap();
  let previous_local_app_data = std::env::var_os("LOCALAPPDATA");
  let root = std::env::temp_dir().join(format!("soma-empty-codex-localappdata-{}", uuid::Uuid::new_v4()));
  fs::create_dir_all(&root).unwrap();
  std::env::set_var("LOCALAPPDATA", &root);
  let (program, args) = codex_launcher_command(vec!["--version".to_string()]);
  #[cfg(windows)]
  {
    assert_eq!(program, "cmd.exe");
    assert_eq!(args, vec!["/D", "/C", "codex", "--version"]);
  }
  #[cfg(not(windows))]
  {
    assert_eq!(program, "codex");
    assert_eq!(args, vec!["--version"]);
  }
  restore_env_var("LOCALAPPDATA", previous_local_app_data);
  let _ = fs::remove_dir_all(root);
}

#[test]
fn codex_login_status_command_uses_codex_launcher() {
  let _guard = RUNTIME_ENV_LOCK.lock().unwrap();
  let previous_local_app_data = std::env::var_os("LOCALAPPDATA");
  let root = std::env::temp_dir().join(format!("soma-codex-login-status-test-{}", uuid::Uuid::new_v4()));
  fs::create_dir_all(&root).unwrap();
  std::env::set_var("LOCALAPPDATA", &root);

  let (program, args) = codex_login_status_command_spec().unwrap();

  #[cfg(windows)]
  {
    assert_eq!(program, "cmd.exe");
    assert_eq!(args, vec!["/D", "/C", "codex", "login", "status"]);
  }
  #[cfg(not(windows))]
  {
    assert_eq!(program, "codex");
    assert_eq!(args, vec!["login", "status"]);
  }
  restore_env_var("LOCALAPPDATA", previous_local_app_data);
  let _ = fs::remove_dir_all(root);
}

#[test]
fn codex_brain_status_times_out_when_launcher_hangs() {
  let _guard = RUNTIME_ENV_LOCK.lock().unwrap();
  let previous_command = std::env::var_os("SOMA_CODEX_COMMAND");
  let previous_codex_home = std::env::var_os("CODEX_HOME");
  let source_home = std::env::temp_dir().join(format!("soma-codex-timeout-home-test-{}", uuid::Uuid::new_v4()));
  fs::create_dir_all(&source_home).unwrap();
  std::env::set_var("CODEX_HOME", &source_home);

  #[cfg(windows)]
  {
    let sleep_command = source_home.join("sleep-codex.bat");
    fs::write(&sleep_command, "@echo off\r\nping -n 8 127.0.0.1 > nul\r\n").unwrap();
    std::env::set_var("SOMA_CODEX_COMMAND", format!("cmd.exe /D /C {}", sleep_command.to_string_lossy()));
  }
  #[cfg(not(windows))]
  {
    let sleep_command = source_home.join("sleep-codex.sh");
    fs::write(&sleep_command, "sleep 8\n").unwrap();
    std::env::set_var("SOMA_CODEX_COMMAND", format!("sh {}", sleep_command.to_string_lossy()));
  }

  let started = Instant::now();
  let status = codex_brain_status();

  assert_eq!(status["status"], "failed");
  assert!(status["message"].as_str().unwrap_or("").contains("timed out"));
  assert!(started.elapsed() < Duration::from_secs(4));
  restore_env_var("SOMA_CODEX_COMMAND", previous_command);
  restore_env_var("CODEX_HOME", previous_codex_home);
  let _ = fs::remove_dir_all(source_home);
}

#[test]
#[cfg(windows)]
fn codex_launcher_prefers_user_local_codex_executable_on_windows() {
  let _guard = RUNTIME_ENV_LOCK.lock().unwrap();
  let previous_local_app_data = std::env::var_os("LOCALAPPDATA");
  let root = std::env::temp_dir().join(format!("soma-codex-localappdata-{}", uuid::Uuid::new_v4()));
  let codex_dir = root.join("OpenAI").join("Codex").join("bin").join("test-version");
  fs::create_dir_all(&codex_dir).unwrap();
  fs::write(codex_dir.join("codex.exe"), "").unwrap();
  std::env::set_var("LOCALAPPDATA", &root);

  let (program, args) = codex_launcher_command(vec!["--version".to_string()]);

  assert_eq!(program, codex_dir.join("codex.exe").to_string_lossy().to_string());
  assert_eq!(args, vec!["--version"]);
  restore_env_var("LOCALAPPDATA", previous_local_app_data);
  let _ = fs::remove_dir_all(root);
}

#[test]
#[cfg(windows)]
fn codex_login_shell_command_quotes_discovered_executable_on_windows() {
  let program = r#"X:\fixtures\codex\test version\codex.exe"#;
  let args = vec!["login".to_string(), "--device-auth".to_string()];

  assert_eq!(
    windows_shell_command(program, &args),
    r#""X:\fixtures\codex\test version\codex.exe" login --device-auth"#
  );
}

#[test]
fn codex_profile_command_is_writable_and_captures_final_message() {
  let _guard = RUNTIME_ENV_LOCK.lock().unwrap();
  let previous_command = std::env::var_os("SOMA_CODEX_COMMAND");
  std::env::remove_var("SOMA_CODEX_COMMAND");
  let runtime = json!({
    "providerId": "codex_sdk",
    "model": "gpt-test",
    "adapter": {
      "kind": "codex_sdk_profile",
      "profile": "default"
    }
  });
  let (program, args) = profile_command_spec(&runtime, &runtime["adapter"], ProfileCommand::Codex).unwrap();

  #[cfg(windows)]
  assert!(program.eq_ignore_ascii_case("cmd.exe") || program.to_ascii_lowercase().ends_with("codex.exe"));
  #[cfg(not(windows))]
  assert_eq!(program, "codex");
  assert!(args.windows(2).any(|pair| pair[0] == "--sandbox" && pair[1] == "workspace-write"));
  assert!(args.iter().any(|arg| arg == "--ephemeral"));
  assert!(args.windows(2).any(|pair| pair[0] == "--output-last-message" && pair[1] == "codex_final_message.txt"));
  assert!(args.windows(2).any(|pair| pair[0] == "--model" && pair[1] == "gpt-test"));
  restore_env_var("SOMA_CODEX_COMMAND", previous_command);
}

#[test]
fn profile_agent_config_carries_prompt_cap_and_runtime_path() {
  let _guard = RUNTIME_ENV_LOCK.lock().unwrap();
  let previous_command = std::env::var_os("SOMA_CODEX_COMMAND");
  let previous_local_app_data = std::env::var_os("LOCALAPPDATA");
  std::env::remove_var("SOMA_CODEX_COMMAND");
  let root = std::env::temp_dir().join(format!("soma-profile-path-test-{}", uuid::Uuid::new_v4()));
  fs::create_dir_all(&root).unwrap();
  std::env::set_var("LOCALAPPDATA", &root);
  let runtime = json!({
    "providerId": "codex_sdk",
    "model": "gpt-test",
    "adapter": {
      "kind": "codex_sdk_profile",
      "profile": "default"
    }
  });

  let config = profile_agent_config(&runtime, &runtime["adapter"], ProfileCommand::Codex).unwrap();

  assert_eq!(config.max_output_bytes, ADAPTER_OUTPUT_MAX_BYTES);
  assert_eq!(config.prompt_mode, CliPromptMode::Stdin);
  assert_eq!(config.args.last().map(String::as_str), Some("-"));
  #[cfg(windows)]
  assert!(config.path_prepend.iter().any(|path| path == &root.join("Microsoft").join("WindowsApps")));
  #[cfg(not(windows))]
  assert!(config.path_prepend.is_empty());
  restore_env_var("SOMA_CODEX_COMMAND", previous_command);
  restore_env_var("LOCALAPPDATA", previous_local_app_data);
  let _ = fs::remove_dir_all(root);
}

#[test]
fn claude_profile_config_disables_tools_mcp_and_session_persistence() {
  let _guard = RUNTIME_ENV_LOCK.lock().unwrap();
  let previous_command = std::env::var_os("SOMA_CLAUDE_COMMAND");
  std::env::remove_var("SOMA_CLAUDE_COMMAND");
  let runtime = json!({
    "providerId": "claude_code",
    "model": "claude-test",
    "adapter": {
      "kind": "claude_code_profile",
      "profile": "default"
    }
  });

  let config = profile_agent_config(&runtime, &runtime["adapter"], ProfileCommand::Claude).unwrap();

  assert_eq!(config.program, "claude");
  assert_eq!(config.prompt_mode, CliPromptMode::Stdin);
  assert!(config.args.iter().any(|arg| arg == "-p"));
  assert!(config.args.windows(2).any(|pair| pair[0] == "--model" && pair[1] == "claude-test"));
  assert!(config.args.windows(2).any(|pair| pair[0] == "--tools" && pair[1].is_empty()));
  assert!(config.args.iter().any(|arg| arg == "--strict-mcp-config"));
  assert!(config.args.iter().any(|arg| arg == "--no-session-persistence"));
  restore_env_var("SOMA_CLAUDE_COMMAND", previous_command);
}

#[test]
fn claude_profile_compile_uses_embedded_context_and_extracts_stdout_patch() {
  let _guard = RUNTIME_ENV_LOCK.lock().unwrap();
  let previous_command = std::env::var_os("SOMA_CLAUDE_COMMAND");
  let root = std::env::temp_dir().join(format!("soma-claude-profile-test-{}", uuid::Uuid::new_v4()));
  let job_dir = root.join("job");
  let prompt_marker = root.join("prompt.txt");
  let args_marker = root.join("args.txt");
  write_runtime_test_job(&job_dir);
  let command = fake_claude_command(&root, &prompt_marker, &args_marker);
  std::env::set_var("SOMA_CLAUDE_COMMAND", command);
  let runtime = json!({
    "providerId": "claude_code",
    "model": "claude-test",
    "adapter": {
      "kind": "claude_code_profile",
      "profile": "default"
    }
  });

  let result = run_profile_command(&job_dir, &runtime, &runtime["adapter"], ProfileCommand::Claude).unwrap();
  let prompt = fs::read_to_string(&prompt_marker).unwrap();
  let args = fs::read_to_string(&args_marker).unwrap();
  let written: Value = serde_json::from_str(&fs::read_to_string(job_dir.join("output_patch.json")).unwrap()).unwrap();
  restore_env_var("SOMA_CLAUDE_COMMAND", previous_command);

  assert_eq!(result.status, "completed");
  assert!(result.wrote_output_patch);
  assert!(prompt.len() <= JOB_PROMPT_MAX_BYTES);
  assert!(prompt.contains("cannot access the local job folder"));
  assert!(prompt.contains("Runtime test source."));
  assert!(!prompt.contains("The current working directory is a Soma compile job folder."));
  assert!(args.contains("--tools"));
  assert!(args.contains("--strict-mcp-config"));
  assert!(args.contains("--no-session-persistence"));
  assert_eq!(written["warnings"][0]["message"], "claude stdout fixture");
  let _ = fs::remove_dir_all(root);
}

#[test]
fn claude_profile_chat_reports_oversized_stdout_as_invalid_response() {
  let _guard = RUNTIME_ENV_LOCK.lock().unwrap();
  let previous_command = std::env::var_os("SOMA_CLAUDE_COMMAND");
  let root = std::env::temp_dir().join(format!("soma-claude-output-limit-test-{}", uuid::Uuid::new_v4()));
  fs::create_dir_all(&root).unwrap();
  let payload = root.join("oversized-response.txt");
  fs::write(&payload, "x".repeat(ADAPTER_OUTPUT_MAX_BYTES + 1)).unwrap();
  std::env::set_var("SOMA_CLAUDE_COMMAND", fake_stdout_file_command(&root, &payload));
  let runtime = json!({
    "providerId": "claude_code",
    "model": "claude-test",
    "adapter": {
      "kind": "claude_code_profile",
      "profile": "default"
    }
  });
  let request = json!({
    "mode": "graph_chat",
    "context_packet": {
      "user_message": "Explain the selected context."
    }
  });

  let result = run_profile_chat_turn(&runtime, &runtime["adapter"], &request, ProfileCommand::Claude).unwrap();
  restore_env_var("SOMA_CLAUDE_COMMAND", previous_command);
  let _ = fs::remove_dir_all(root);

  assert_eq!(result.status, "failed");
  assert_eq!(result.failure_kind, Some(RuntimeFailureKind::InvalidResponse));
  assert!(result.message.contains("response exceeded 180000 bytes"));
  assert!(result.assistant_message.is_none());
}

#[test]
fn claude_profile_compile_reports_oversized_stdout_as_invalid_response() {
  let _guard = RUNTIME_ENV_LOCK.lock().unwrap();
  let previous_command = std::env::var_os("SOMA_CLAUDE_COMMAND");
  let root = std::env::temp_dir().join(format!("soma-claude-compile-output-limit-{}", uuid::Uuid::new_v4()));
  let job_dir = root.join("job");
  let payload = root.join("oversized-response.txt");
  write_runtime_test_job(&job_dir);
  fs::write(&payload, "x".repeat(ADAPTER_OUTPUT_MAX_BYTES + 1)).unwrap();
  std::env::set_var("SOMA_CLAUDE_COMMAND", fake_stdout_file_command(&root, &payload));
  let runtime = json!({
    "providerId": "claude_code",
    "model": "claude-test",
    "adapter": {
      "kind": "claude_code_profile",
      "profile": "default"
    }
  });

  let result = run_profile_command(&job_dir, &runtime, &runtime["adapter"], ProfileCommand::Claude).unwrap();
  restore_env_var("SOMA_CLAUDE_COMMAND", previous_command);
  let _ = fs::remove_dir_all(root);

  assert_eq!(result.status, "failed");
  assert_eq!(result.failure_kind, Some(RuntimeFailureKind::InvalidResponse));
  assert!(result.message.contains("response exceeded 180000 bytes"));
  assert!(!result.wrote_output_patch);
}

#[test]
fn hosted_job_prompt_embeds_required_context_and_reports_full_coverage() {
  let root = std::env::temp_dir().join(format!("soma-hosted-job-prompt-{}", uuid::Uuid::new_v4()));
  let chunks = vec![
    json!({ "chunk_id": "chunk_001", "content": "First source." }),
    json!({ "chunk_id": "chunk_002", "content": "Second source." }),
  ];
  write_hosted_prompt_fixture(&root, "instruction sentinel", &chunks, "graph sentinel", "schema sentinel");

  let prompt = hosted_job_prompt(&root).unwrap();

  assert_eq!(prompt.included_chunk_count, chunks.len());
  assert_eq!(prompt.total_chunk_count, chunks.len());
  assert!(prompt.text.len() <= JOB_PROMPT_MAX_BYTES);
  assert!(prompt.text.contains("instruction sentinel"));
  assert!(prompt.text.contains("graph sentinel"));
  assert!(prompt.text.contains("schema sentinel"));
  assert!(prompt.text.contains("Source chunk coverage in this request: 2 of 2 selected job chunks."));
  assert!(prompt.text.contains("cannot access the local job folder"));
  assert!(!prompt.text.contains("Use the files in the job folder"));
  assert_eq!(embedded_prompt_chunks(&prompt.text), chunks);
  let _ = fs::remove_dir_all(root);
}

#[test]
fn generated_hosted_compile_context_omits_local_source_paths() {
  let root = std::env::temp_dir().join(format!("soma-hosted-source-privacy-{}", uuid::Uuid::new_v4()));
  let paths = create_workspace_dir(&root).unwrap();
  let source = root.join("private-source.md");
  fs::write(&source, "User: Keep provenance local.\n\nAssistant: Send only model-relevant source context.").unwrap();
  let imported = import_source_file(&paths, &source).unwrap();

  let conn = open_existing_database(&paths.database_path).unwrap();
  let (stored_original_path, stored_raw_path): (String, String) = conn
    .query_row("SELECT original_path, raw_path FROM sources LIMIT 1", [], |row| Ok((row.get(0)?, row.get(1)?)))
    .unwrap();
  drop(conn);
  assert_eq!(stored_original_path, source.canonicalize().unwrap().to_string_lossy());
  assert_eq!(stored_raw_path, imported["rawPath"].as_str().unwrap());

  let runtime = json!({
    "providerId": "openai",
    "model": "gpt-test",
    "adapter": {
      "kind": "api_provider",
      "endpoint": "https://api.example.test"
    }
  });
  let job = create_graph_extraction_job_with_runtime(&paths, &runtime).unwrap();
  let job_dir = PathBuf::from(job["jobDir"].as_str().unwrap());
  let chunks_document: Value = serde_json::from_str(&fs::read_to_string(job_dir.join("chunks.json")).unwrap()).unwrap();
  let chunks = chunks_document["chunks"].as_array().unwrap();

  assert!(!chunks.is_empty());
  assert!(chunks.iter().all(|chunk| chunk.get("original_path").is_none() && chunk.get("raw_path").is_none()));

  let prompt = hosted_job_prompt(&job_dir).unwrap();
  assert!(embedded_prompt_chunks(&prompt.text)
    .iter()
    .all(|chunk| chunk.get("original_path").is_none() && chunk.get("raw_path").is_none()));
  assert!(!prompt.text.contains(&serde_json::to_string(&stored_original_path).unwrap()));
  assert!(!prompt.text.contains(&serde_json::to_string(&stored_raw_path).unwrap()));
  let _ = fs::remove_dir_all(root);
}

#[test]
fn hosted_job_prompt_adds_only_complete_chunks_that_fit() {
  let root = std::env::temp_dir().join(format!("soma-bounded-job-prompt-{}", uuid::Uuid::new_v4()));
  let chunks = (0..80)
    .map(|index| {
      json!({
        "chunk_id": format!("chunk_{index:03}"),
        "content": format!("source-{index:03} {}", "evidence ".repeat(220))
      })
    })
    .collect::<Vec<_>>();
  write_hosted_prompt_fixture(&root, "instruction sentinel", &chunks, "graph sentinel", "schema sentinel");

  let prompt = hosted_job_prompt(&root).unwrap();
  let embedded_chunks = embedded_prompt_chunks(&prompt.text);

  assert!(prompt.included_chunk_count > 0);
  assert!(prompt.included_chunk_count < chunks.len());
  assert_eq!(prompt.total_chunk_count, chunks.len());
  assert_eq!(embedded_chunks, chunks[..prompt.included_chunk_count]);
  assert!(prompt.text.len() <= JOB_PROMPT_MAX_BYTES);
  assert!(prompt.text.contains(&format!(
    "Source chunk coverage in this request: {} of {} selected job chunks.",
    prompt.included_chunk_count, prompt.total_chunk_count
  )));
  assert!(prompt.text.contains("instruction sentinel"));
  assert!(prompt.text.contains("graph sentinel"));
  assert!(prompt.text.contains("schema sentinel"));
  let _ = fs::remove_dir_all(root);
}

#[test]
fn hosted_job_prompt_rejects_required_context_over_the_request_limit() {
  let root = std::env::temp_dir().join(format!("soma-oversized-job-prompt-{}", uuid::Uuid::new_v4()));
  fs::create_dir_all(&root).unwrap();
  write_hosted_prompt_fixture(
    &root,
    &"界".repeat(JOB_PROMPT_MAX_BYTES),
    &[json!({ "chunk_id": "chunk_001", "content": "Source." })],
    "graph sentinel",
    "schema sentinel",
  );

  let error = hosted_job_prompt(&root).unwrap_err();

  assert_eq!(error.code, "Soma_VALIDATION_ERROR");
  assert!(error.message.contains("exceed"));
  assert!(error.message.contains("request limit"));
  let _ = fs::remove_dir_all(root);
}

fn write_hosted_prompt_fixture(
  root: &Path,
  instructions: &str,
  chunks: &[Value],
  graph_sentinel: &str,
  schema_sentinel: &str,
) {
  fs::create_dir_all(root).unwrap();
  fs::write(root.join("instructions.md"), instructions).unwrap();
  fs::write(root.join("runtime.json"), r#"{"providerId":"test-runtime"}"#).unwrap();
  fs::write(root.join("chunks.json"), json!({ "schema_version": 1, "chunks": chunks }).to_string()).unwrap();
  fs::write(
    root.join("current_graph_snapshot.json"),
    json!({ "schema_version": 1, "sentinel": graph_sentinel, "nodes": [], "edges": [] }).to_string(),
  )
  .unwrap();
  fs::write(root.join("graph_patch.schema.json"), json!({ "type": "object", "sentinel": schema_sentinel }).to_string())
    .unwrap();
}

fn embedded_prompt_chunks(prompt: &str) -> Vec<Value> {
  let section_start = prompt.find(HOSTED_CHUNKS_SECTION).unwrap() + HOSTED_CHUNKS_SECTION.len();
  let section = &prompt[section_start..];
  let section_end = section.find("\n\nSource chunk coverage in this request:").unwrap();
  serde_json::from_str(&section[..section_end]).unwrap()
}

#[test]
fn codex_profile_runtime_preserves_auth_home_and_isolates_sqlite() {
  let _guard = RUNTIME_ENV_LOCK.lock().unwrap();
  let previous_codex_home = std::env::var_os("CODEX_HOME");
  let source_home = std::env::temp_dir().join(format!("soma-codex-source-home-test-{}", uuid::Uuid::new_v4()));
  fs::create_dir_all(&source_home).unwrap();
  fs::write(source_home.join("auth.json"), r#"{"token":"test"}"#).unwrap();
  fs::write(source_home.join("config.toml"), "profile = \"default\"\n").unwrap();
  std::env::set_var("CODEX_HOME", &source_home);

  let runtime = json!({
    "providerId": "codex_sdk",
    "model": "gpt-test",
    "adapter": {
      "kind": "codex_sdk_profile",
      "profile": "default"
    }
  });
  let mut config = profile_agent_config(&runtime, &runtime["adapter"], ProfileCommand::Codex).unwrap();

  let first_sqlite_home = configure_codex_runtime_env(&mut config).unwrap();

  let sqlite_home = config
    .env
    .iter()
    .find_map(|(name, value)| (name == "CODEX_SQLITE_HOME").then_some(value))
    .expect("CODEX_SQLITE_HOME env is set");
  assert!(!config.env.iter().any(|(name, _)| name == "CODEX_HOME"));
  assert!(Path::new(sqlite_home).is_dir());
  assert!(Path::new(sqlite_home).file_name().unwrap().to_string_lossy().starts_with("soma-codex-sqlite-"));
  assert!(!Path::new(sqlite_home).join("auth.json").exists());
  assert!(!Path::new(sqlite_home).join("config.toml").exists());
  assert_eq!(fs::read_to_string(source_home.join("auth.json")).unwrap(), r#"{"token":"test"}"#);
  assert_eq!(fs::read_to_string(source_home.join("config.toml")).unwrap(), "profile = \"default\"\n");
  assert!(config.env.iter().any(|(name, value)| name == "CODEX_NON_INTERACTIVE" && value == "1"));

  let first_sqlite_path = sqlite_home.to_string();
  let mut second_config = profile_agent_config(&runtime, &runtime["adapter"], ProfileCommand::Codex).unwrap();
  let second_sqlite_home = configure_codex_runtime_env(&mut second_config).unwrap();
  let second_sqlite_path = second_config
    .env
    .iter()
    .find_map(|(name, value)| (name == "CODEX_SQLITE_HOME").then_some(value))
    .expect("second CODEX_SQLITE_HOME env is set")
    .to_string();
  assert_ne!(first_sqlite_path, second_sqlite_path);

  drop(first_sqlite_home);
  drop(second_sqlite_home);
  assert!(!Path::new(&first_sqlite_path).exists());
  assert!(!Path::new(&second_sqlite_path).exists());
  assert!(source_home.exists());

  restore_env_var("CODEX_HOME", previous_codex_home);
  let _ = fs::remove_dir_all(source_home);
}

#[test]
fn codex_profile_tasks_preserve_auth_home_and_clean_sqlite_after_every_outcome() {
  let _guard = RUNTIME_ENV_LOCK.lock().unwrap();
  let previous_codex_home = std::env::var_os("CODEX_HOME");
  let root = std::env::temp_dir().join(format!("soma-codex-cleanup-test-{}", uuid::Uuid::new_v4()));
  fs::create_dir_all(&root).unwrap();
  std::env::set_var("CODEX_HOME", &root);

  for (name, exit_code, timeout_ms, expected_status) in [
    ("success", 0, 2_000, AgentTaskStatus::Completed),
    ("failure", 7, 2_000, AgentTaskStatus::Failed),
    ("timeout", 0, 80, AgentTaskStatus::TimedOut),
  ] {
    let marker = root.join(format!("{name}-env.txt"));
    let (program, args) = fake_codex_command(&root, name, &marker, exit_code, name == "timeout");
    let result = run_profile_agent_task(
      CliAgentConfig {
        provider_id: ProviderId::from("codex_sdk"),
        program,
        args,
        prompt_mode: CliPromptMode::Stdin,
        path_prepend: Vec::new(),
        env: Vec::new(),
        max_output_bytes: ADAPTER_OUTPUT_MAX_BYTES,
      },
      AgentTaskRequest::new(&root, "", timeout_ms),
      ProfileCommand::Codex,
    )
    .unwrap();

    assert_eq!(result.status, expected_status);
    let (codex_home, sqlite_home) = read_codex_env_marker(&marker);
    assert_eq!(Path::new(&codex_home), root);
    assert!(!Path::new(&sqlite_home).exists());
    assert!(root.exists());
  }

  restore_env_var("CODEX_HOME", previous_codex_home);
  let _ = fs::remove_dir_all(root);
}

#[test]
fn codex_probe_preserves_auth_home_and_cleans_sqlite_after_timeout() {
  let _guard = RUNTIME_ENV_LOCK.lock().unwrap();
  let previous_codex_home = std::env::var_os("CODEX_HOME");
  let root = std::env::temp_dir().join(format!("soma-codex-probe-cleanup-{}", uuid::Uuid::new_v4()));
  fs::create_dir_all(&root).unwrap();
  std::env::set_var("CODEX_HOME", &root);
  let marker = root.join("probe-env.txt");
  let (program, args) = fake_codex_command(&root, "probe", &marker, 0, true);

  let result = run_codex_probe(&program, args, 80).unwrap();

  assert_eq!(result.status, AgentTaskStatus::TimedOut);
  let (codex_home, sqlite_home) = read_codex_env_marker(&marker);
  assert_eq!(Path::new(&codex_home), root);
  assert!(!Path::new(&sqlite_home).exists());
  assert!(root.exists());
  restore_env_var("CODEX_HOME", previous_codex_home);
  let _ = fs::remove_dir_all(root);
}

#[test]
fn runtime_temp_dir_cleans_up_during_early_error_return() {
  fn fail_after_creation(path: &Path) -> std::io::Result<()> {
    let temp_dir = RuntimeTempDir::create(path.to_path_buf())?;
    fs::write(temp_dir.path().join("partial.txt"), "partial")?;
    Err(std::io::Error::other("fixture failure"))
  }

  let path = std::env::temp_dir().join(format!("soma-chat-cleanup-test-{}", uuid::Uuid::new_v4()));

  assert!(fail_after_creation(&path).is_err());
  assert!(!path.exists());
}

#[test]
fn runtime_command_timeout_returns_without_hanging() {
  let root = std::env::temp_dir().join(format!("soma-runtime-timeout-test-{}", uuid::Uuid::new_v4()));
  fs::create_dir_all(&root).unwrap();

  #[cfg(windows)]
  let (program, args) = ("cmd.exe", vec!["/D".to_string(), "/C".to_string(), "ping -n 4 127.0.0.1 > nul".to_string()]);

  #[cfg(not(windows))]
  let (program, args) = ("sh", vec!["-c".to_string(), "sleep 2".to_string()]);

  let started = Instant::now();
  let output = CliAgentRuntime::new(CliAgentConfig {
    provider_id: ProviderId::from("timeout_test"),
    program: program.to_string(),
    args,
    prompt_mode: CliPromptMode::Stdin,
    path_prepend: Vec::new(),
    env: Vec::new(),
    max_output_bytes: ADAPTER_OUTPUT_MAX_BYTES,
  })
  .run_task(AgentTaskRequest::new(&root, "", 80))
  .unwrap();
  assert_eq!(output.status, AgentTaskStatus::TimedOut);
  assert!(started.elapsed() < Duration::from_secs(2));
  let _ = fs::remove_dir_all(root);
}

#[test]
fn local_endpoint_runtime_writes_output_patch() {
  let job_dir = std::env::temp_dir().join(format!("soma-local-runtime-test-{}", uuid::Uuid::new_v4()));
  write_runtime_test_job(&job_dir);
  let patch = json!({
    "schema_version": 1,
    "proposed_nodes": [],
    "proposed_edges": [],
    "proposed_node_body_updates": [],
    "proposed_edge_bridge_updates": [],
    "proposed_message_evidence_attachments": [],
    "proposed_paths": [],
    "ambiguities": [],
    "merge_candidates": [],
    "warnings": [{
      "message": "runtime fixture"
    }]
  });
  let listener = TcpListener::bind("127.0.0.1:0").unwrap();
  let endpoint = format!("http://{}", listener.local_addr().unwrap());
  let response_content = patch.to_string();
  let server = thread::spawn(move || {
    let (mut stream, _) = listener.accept().unwrap();
    read_http_request(&mut stream);
    let body = json!({
      "choices": [{
        "message": {
          "content": response_content
        }
      }]
    })
    .to_string();
    let response = format!(
      "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
      body.len(),
      body
    );
    stream.write_all(response.as_bytes()).unwrap();
  });
  let runtime = json!({
    "providerId": "local_llm",
    "model": "fixture-model",
    "endpoint": endpoint,
    "adapter": {
      "kind": "local_offline_endpoint",
      "endpoint": endpoint
    }
  });

  let result = run_compile_job(&job_dir, &runtime).unwrap();
  server.join().unwrap();

  assert_eq!(result.status, "completed");
  assert_eq!(result.adapter_kind, "local_offline_endpoint");
  assert!(result.wrote_output_patch);
  assert!(result.message.contains("using 1 of 1 job chunks"));
  let written: Value = serde_json::from_str(&fs::read_to_string(job_dir.join("output_patch.json")).unwrap()).unwrap();
  assert_eq!(written["warnings"][0]["message"], "runtime fixture");
  let _ = fs::remove_dir_all(job_dir);
}

#[test]
fn api_provider_runtime_uses_key_and_writes_output_patch() {
  let job_dir = std::env::temp_dir().join(format!("soma-api-runtime-test-{}", uuid::Uuid::new_v4()));
  write_runtime_test_job(&job_dir);
  let patch = json!({
    "schema_version": 1,
    "proposals": [],
    "proposed_nodes": [],
    "proposed_edges": [],
    "proposed_paths": [],
    "ambiguities": [],
    "merge_candidates": [],
    "warnings": [{
      "message": "api fixture"
    }]
  });
  let listener = TcpListener::bind("127.0.0.1:0").unwrap();
  let endpoint = format!("http://{}", listener.local_addr().unwrap());
  let response_content = patch.to_string();
  let server = thread::spawn(move || {
    let (mut stream, _) = listener.accept().unwrap();
    let request = read_http_request(&mut stream);
    let body = json!({
      "choices": [{
        "message": {
          "content": response_content
        }
      }]
    })
    .to_string();
    let response = format!(
      "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
      body.len(),
      body
    );
    stream.write_all(response.as_bytes()).unwrap();
    request
  });
  let runtime = json!({
    "providerId": "openrouter",
    "model": "openai/gpt-test",
    "endpoint": endpoint,
    "authProfile": "default",
    "adapter": {
      "kind": "api_provider",
      "endpoint": endpoint,
      "requireApiKey": true
    }
  });

  let result = run_compile_job_with_credentials(&job_dir, &runtime, &StaticCredential("router-key")).unwrap();
  let request = server.join().unwrap();

  assert_eq!(result.status, "completed");
  assert_eq!(result.adapter_kind, "api_provider");
  assert!(request.contains("authorization: bearer router-key"));
  let written: Value = serde_json::from_str(&fs::read_to_string(job_dir.join("output_patch.json")).unwrap()).unwrap();
  assert_eq!(written["warnings"][0]["message"], "api fixture");
  let _ = fs::remove_dir_all(job_dir);
}

#[test]
fn api_provider_model_listing_uses_key_and_runtime_model_endpoint() {
  let listener = TcpListener::bind("127.0.0.1:0").unwrap();
  let endpoint = format!("http://{}", listener.local_addr().unwrap());
  let server = thread::spawn(move || {
    let (mut stream, _) = listener.accept().unwrap();
    let request = read_http_request(&mut stream);
    let body = json!({
      "data": [
        { "id": "z-ai/glm-5.2" },
        { "id": "moonshotai/kimi-k2.6" }
      ]
    })
    .to_string();
    let response = format!(
      "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
      body.len(),
      body
    );
    stream.write_all(response.as_bytes()).unwrap();
    request
  });
  let runtime = json!({
    "providerId": "openrouter",
    "model": "z-ai/glm-5.2",
    "endpoint": endpoint,
    "authProfile": "default",
    "adapter": {
      "kind": "api_provider",
      "endpoint": endpoint,
      "requireApiKey": true
    }
  });

  let result = list_runtime_models(&runtime, &StaticCredential("router-key")).unwrap();
  let request = server.join().unwrap();

  assert_eq!(result["status"], "ready");
  let models = result["models"].as_array().unwrap();
  assert!(models.contains(&json!("z-ai/glm-5.2")));
  assert!(models.contains(&json!("moonshotai/kimi-k2.6")));
  assert!(request.contains("get /v1/models "));
  assert!(request.contains("authorization: bearer router-key"));
}

#[test]
fn anthropic_runtime_uses_messages_api_key_and_writes_output_patch() {
  let job_dir = std::env::temp_dir().join(format!("soma-anthropic-runtime-test-{}", uuid::Uuid::new_v4()));
  write_runtime_test_job(&job_dir);
  let patch = json!({
    "schema_version": 1,
    "proposed_nodes": [],
    "proposed_edges": [],
    "proposed_node_body_updates": [],
    "proposed_edge_bridge_updates": [],
    "proposed_message_evidence_attachments": [],
    "proposed_paths": [],
    "ambiguities": [],
    "merge_candidates": [],
    "warnings": [{ "message": "anthropic fixture" }]
  });
  let listener = TcpListener::bind("127.0.0.1:0").unwrap();
  let endpoint = format!("http://{}", listener.local_addr().unwrap());
  let response_content = patch.to_string();
  let server = thread::spawn(move || {
    let (mut stream, _) = listener.accept().unwrap();
    let request = read_http_request(&mut stream);
    let body = json!({
      "content": [{ "type": "text", "text": response_content }],
      "stop_reason": "end_turn"
    })
    .to_string();
    let response = format!(
      "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
      body.len(),
      body
    );
    stream.write_all(response.as_bytes()).unwrap();
    request
  });
  let runtime = json!({
    "providerId": "claude",
    "model": "claude-sonnet-5",
    "endpoint": endpoint,
    "authProfile": "default",
    "adapter": {
      "kind": "anthropic_messages_provider",
      "endpoint": endpoint,
      "requireApiKey": true,
      "anthropicVersion": "2023-06-01"
    }
  });

  let result = run_compile_job_with_credentials(&job_dir, &runtime, &StaticCredential("anthropic-key")).unwrap();
  let request = server.join().unwrap();

  assert_eq!(result.status, "completed");
  assert_eq!(result.adapter_kind, "anthropic_messages_provider");
  assert!(result.message.contains("using 1 of 1 job chunks"));
  assert!(request.contains("post /v1/messages "));
  assert!(request.contains("x-api-key: anthropic-key"));
  assert!(request.contains("\"model\":\"claude-sonnet-5\""));
  let written: Value = serde_json::from_str(&fs::read_to_string(job_dir.join("output_patch.json")).unwrap()).unwrap();
  assert_eq!(written["warnings"][0]["message"], "anthropic fixture");
  let _ = fs::remove_dir_all(job_dir);
}

#[test]
fn anthropic_model_listing_uses_messages_credentials() {
  let listener = TcpListener::bind("127.0.0.1:0").unwrap();
  let endpoint = format!("http://{}", listener.local_addr().unwrap());
  let server = thread::spawn(move || {
    let (mut stream, _) = listener.accept().unwrap();
    let request = read_http_request(&mut stream);
    let body = json!({
      "data": [
        { "id": "claude-sonnet-5" },
        { "id": "claude-opus-4-8" }
      ]
    })
    .to_string();
    let response = format!(
      "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
      body.len(),
      body
    );
    stream.write_all(response.as_bytes()).unwrap();
    request
  });
  let runtime = json!({
    "providerId": "claude",
    "model": "claude-sonnet-5",
    "endpoint": endpoint,
    "authProfile": "default",
    "adapter": {
      "kind": "anthropic_messages_provider",
      "endpoint": endpoint,
      "requireApiKey": true,
      "anthropicVersion": "2023-06-01"
    }
  });

  let result = list_runtime_models(&runtime, &StaticCredential("anthropic-key")).unwrap();
  let request = server.join().unwrap();

  assert_eq!(result["status"], "ready");
  let models = result["models"].as_array().unwrap();
  assert!(models.contains(&json!("claude-sonnet-5")));
  assert!(models.contains(&json!("claude-opus-4-8")));
  assert!(request.contains("get /v1/models "));
  assert!(request.contains("x-api-key: anthropic-key"));
}

fn fake_codex_command(root: &Path, name: &str, marker: &Path, exit_code: i32, hangs: bool) -> (String, Vec<String>) {
  #[cfg(windows)]
  {
    let script = root.join(format!("{name}.bat"));
    let wait = if hangs { "ping -n 8 127.0.0.1 > nul\r\n" } else { "" };
    fs::write(
      &script,
      format!(
        concat!(
          "@echo off\r\n> \"{}\" echo %CODEX_HOME%\r\n",
          ">> \"{}\" echo %CODEX_SQLITE_HOME%\r\n",
          "{}exit /b {}\r\n"
        ),
        marker.display(),
        marker.display(),
        wait,
        exit_code
      ),
    )
    .unwrap();
    ("cmd.exe".to_string(), vec!["/D".to_string(), "/C".to_string(), script.to_string_lossy().to_string()])
  }

  #[cfg(not(windows))]
  {
    let script = root.join(format!("{name}.sh"));
    let wait = if hangs { "sleep 2\n" } else { "" };
    fs::write(
      &script,
      format!(
        "printf '%s\\n%s\\n' \"$CODEX_HOME\" \"$CODEX_SQLITE_HOME\" > '{}'\n{wait}exit {exit_code}\n",
        marker.display()
      ),
    )
    .unwrap();
    ("sh".to_string(), vec![script.to_string_lossy().to_string()])
  }
}

fn fake_claude_command(root: &Path, prompt_marker: &Path, args_marker: &Path) -> String {
  let patch = json!({
    "schema_version": 1,
    "proposed_nodes": [],
    "proposed_edges": [],
    "proposed_node_body_updates": [],
    "proposed_edge_bridge_updates": [],
    "proposed_message_evidence_attachments": [],
    "proposed_paths": [],
    "ambiguities": [],
    "merge_candidates": [],
    "warnings": [{ "message": "claude stdout fixture" }]
  })
  .to_string();

  #[cfg(windows)]
  {
    let script = root.join("fake-claude.bat");
    fs::write(
      &script,
      format!(
        "@echo off\r\n> \"{}\" echo %*\r\nmore > \"{}\"\r\necho {}\r\n",
        args_marker.display(),
        prompt_marker.display(),
        patch
      ),
    )
    .unwrap();
    format!(r#"cmd.exe /D /C "{}""#, script.display())
  }

  #[cfg(not(windows))]
  {
    let script = root.join("fake-claude.sh");
    fs::write(
      &script,
      format!(
        "printf '%s\\n' \"$@\" > \"{}\"\ncat > \"{}\"\nprintf '%s\\n' '{}'\n",
        args_marker.display(),
        prompt_marker.display(),
        patch
      ),
    )
    .unwrap();
    format!(r#"sh "{}""#, script.display())
  }
}

fn fake_stdout_file_command(root: &Path, payload: &Path) -> String {
  #[cfg(windows)]
  {
    let script = root.join("fake-oversized-output.bat");
    fs::write(&script, format!("@echo off\r\ntype \"{}\"\r\n", payload.display())).unwrap();
    format!(r#"cmd.exe /D /C "{}""#, script.display())
  }

  #[cfg(not(windows))]
  {
    let script = root.join("fake-oversized-output.sh");
    fs::write(&script, format!("cat '{}'\n", payload.display())).unwrap();
    format!(r#"sh "{}""#, script.display())
  }
}

fn read_codex_env_marker(marker: &Path) -> (String, String) {
  let contents = fs::read_to_string(marker).unwrap();
  let mut lines = contents.lines();
  let codex_home = lines.next().expect("CODEX_HOME marker").trim().to_string();
  let sqlite_home = lines.next().expect("CODEX_SQLITE_HOME marker").trim().to_string();
  (codex_home, sqlite_home)
}

fn read_http_request(stream: &mut TcpStream) -> String {
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
      return String::from_utf8_lossy(&bytes).to_ascii_lowercase();
    }
  }
  String::new()
}

fn find_header_end(bytes: &[u8]) -> Option<usize> {
  bytes.windows(4).position(|window| window == b"\r\n\r\n")
}

fn restore_env_var(name: &str, value: Option<std::ffi::OsString>) {
  if let Some(value) = value {
    std::env::set_var(name, value);
  } else {
    std::env::remove_var(name);
  }
}

struct StaticCredential(&'static str);

impl CredentialResolver for StaticCredential {
  fn resolve(&self, _credential: &CredentialRef) -> Result<Option<String>, AiRuntimeError> {
    Ok(Some(self.0.to_string()))
  }
}
