use crate::agent::{AgentTaskRequest, AgentTaskResult, AgentTaskStatus};
use crate::errors::AiRuntimeError;
use crate::ids::ProviderId;
use std::env;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::thread;
use std::time::{Duration, Instant};

const DEFAULT_CAPTURE_MAX_BYTES: usize = 180_000;
const PIPE_COLLECT_TIMEOUT_MS: u64 = 500;
const POLL_INTERVAL_MS: u64 = 20;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CliAgentConfig {
  pub provider_id: ProviderId,
  pub program: String,
  pub args: Vec<String>,
  pub prompt_mode: CliPromptMode,
  pub path_prepend: Vec<PathBuf>,
  pub env: Vec<(String, String)>,
  pub max_output_bytes: usize,
}

impl CliAgentConfig {
  pub fn new(
    provider_id: impl Into<ProviderId>,
    program: impl Into<String>,
    args: Vec<String>,
    prompt_mode: CliPromptMode,
  ) -> Self {
    Self {
      provider_id: provider_id.into(),
      program: program.into(),
      args,
      prompt_mode,
      path_prepend: Vec::new(),
      env: Vec::new(),
      max_output_bytes: DEFAULT_CAPTURE_MAX_BYTES,
    }
  }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CliPromptMode {
  Stdin,
  Arg { placeholder: String },
}

#[derive(Debug, Clone)]
pub struct CliAgentRuntime {
  config: CliAgentConfig,
}

impl CliAgentRuntime {
  pub fn new(config: CliAgentConfig) -> Self {
    Self { config }
  }

  pub fn run_task(&self, request: AgentTaskRequest) -> Result<AgentTaskResult, AiRuntimeError> {
    if self.config.program.trim().is_empty() {
      return Err(self.invalid_config("program is empty"));
    }
    if request.timeout_ms == 0 {
      return Err(self.invalid_config("timeout_ms must be greater than zero"));
    }
    if self.config.max_output_bytes == 0 {
      return Err(self.invalid_config("max_output_bytes must be greater than zero"));
    }

    let args = self.render_args(&request.prompt)?;
    let command_program = resolve_program(&self.config.program, &self.config.path_prepend);
    let mut command = Command::new(&command_program);
    command.args(&args).current_dir(&request.working_dir).stdout(Stdio::piped()).stderr(Stdio::piped());
    apply_path_prepend(&mut command, &self.config.path_prepend);
    apply_env(&mut command, &self.config.env);
    match self.config.prompt_mode {
      CliPromptMode::Stdin => {
        command.stdin(Stdio::piped());
      }
      CliPromptMode::Arg { .. } => {
        command.stdin(Stdio::null());
      }
    }
    configure_child_process(&mut command);

    let mut child = command.spawn().map_err(|error| AiRuntimeError::ProviderExecution {
      provider: self.config.provider_id.clone(),
      message: format!("Could not start runtime command `{}`: {error}", self.config.program),
    })?;

    if matches!(self.config.prompt_mode, CliPromptMode::Stdin) {
      write_prompt_async(child.stdin.take(), request.prompt.clone());
    }

    let stdout_reader = read_pipe_async(child.stdout.take(), self.config.max_output_bytes);
    let stderr_reader = read_pipe_async(child.stderr.take(), self.config.max_output_bytes);
    let timeout = Duration::from_millis(request.timeout_ms);
    let started = Instant::now();
    let mut timed_out = false;
    let mut cancelled = false;

    let exit_status = loop {
      if let Some(status) = child.try_wait().map_err(|error| AiRuntimeError::ProviderExecution {
        provider: self.config.provider_id.clone(),
        message: format!("Runtime command wait failed: {error}"),
      })? {
        break status;
      }
      if started.elapsed() >= timeout {
        timed_out = true;
        let _ = kill_process_tree(&mut child);
        let status = child.wait().map_err(|error| AiRuntimeError::ProviderExecution {
          provider: self.config.provider_id.clone(),
          message: format!("Runtime command wait failed after timeout: {error}"),
        })?;
        break status;
      }
      if request.cancellation.is_cancelled() {
        cancelled = true;
        let _ = kill_process_tree(&mut child);
        let status = child.wait().map_err(|error| AiRuntimeError::ProviderExecution {
          provider: self.config.provider_id.clone(),
          message: format!("Runtime command wait failed after cancellation: {error}"),
        })?;
        break status;
      }
      thread::sleep(Duration::from_millis(POLL_INTERVAL_MS));
    };

    let stdout_capture = collect_pipe(stdout_reader);
    let stderr_capture = collect_pipe(stderr_reader);
    let stdout = String::from_utf8_lossy(&stdout_capture.bytes).to_string();
    let stderr = String::from_utf8_lossy(&stderr_capture.bytes).to_string();
    let status = if cancelled {
      AgentTaskStatus::Cancelled
    } else if timed_out {
      AgentTaskStatus::TimedOut
    } else if exit_status.success() {
      AgentTaskStatus::Completed
    } else {
      AgentTaskStatus::Failed
    };

    Ok(AgentTaskResult {
      status,
      stdout,
      stdout_truncated: stdout_capture.truncated,
      stderr,
      stderr_truncated: stderr_capture.truncated,
      exit_code: exit_status.code(),
    })
  }

  fn render_args(&self, prompt: &str) -> Result<Vec<String>, AiRuntimeError> {
    match &self.config.prompt_mode {
      CliPromptMode::Stdin => Ok(self.config.args.clone()),
      CliPromptMode::Arg { placeholder } => {
        if placeholder.is_empty() {
          return Err(self.invalid_config("prompt placeholder is empty"));
        }
        let mut found = false;
        let args = self
          .config
          .args
          .iter()
          .map(|arg| {
            if arg == placeholder {
              found = true;
              prompt.to_string()
            } else {
              arg.clone()
            }
          })
          .collect::<Vec<_>>();
        if !found {
          return Err(self.invalid_config(format!("prompt placeholder `{placeholder}` is missing from args")));
        }
        Ok(args)
      }
    }
  }

  fn invalid_config(&self, message: impl Into<String>) -> AiRuntimeError {
    AiRuntimeError::InvalidAgentConfig { provider: self.config.provider_id.clone(), message: message.into() }
  }
}

fn write_prompt_async(stdin: Option<impl Write + Send + 'static>, prompt: String) {
  thread::spawn(move || {
    if let Some(mut stdin) = stdin {
      let _ = stdin.write_all(prompt.as_bytes());
    }
  });
}

struct PipeCapture {
  bytes: Vec<u8>,
  truncated: bool,
}

fn read_pipe_async(pipe: Option<impl Read + Send + 'static>, limit: usize) -> Receiver<PipeCapture> {
  let (sender, receiver) = mpsc::channel();
  thread::spawn(move || {
    let mut bytes = Vec::new();
    let mut truncated = false;
    if let Some(mut pipe) = pipe {
      let mut buffer = [0_u8; 8192];
      loop {
        match pipe.read(&mut buffer) {
          Ok(0) | Err(_) => break,
          Ok(count) => {
            let remaining = limit.saturating_sub(bytes.len());
            if remaining > 0 {
              bytes.extend_from_slice(&buffer[..count.min(remaining)]);
            }
            truncated |= count > remaining;
          }
        }
      }
    }
    let _ = sender.send(PipeCapture { bytes, truncated });
  });
  receiver
}

fn collect_pipe(receiver: Receiver<PipeCapture>) -> PipeCapture {
  receiver
    .recv_timeout(Duration::from_millis(PIPE_COLLECT_TIMEOUT_MS))
    .unwrap_or(PipeCapture { bytes: Vec::new(), truncated: false })
}

fn resolve_program(program: &str, path_prepend: &[PathBuf]) -> String {
  if path_prepend.is_empty() || has_path_separator(program) {
    return program.to_string();
  }
  candidate_programs(program)
    .into_iter()
    .flat_map(|name| path_prepend.iter().map(move |dir| dir.join(&name)))
    .find(|path| path.is_file())
    .map(|path| path.to_string_lossy().to_string())
    .unwrap_or_else(|| program.to_string())
}

fn has_path_separator(program: &str) -> bool {
  program.contains('/') || program.contains('\\')
}

fn candidate_programs(program: &str) -> Vec<String> {
  let mut candidates = vec![program.to_string()];
  #[cfg(windows)]
  {
    if Path::new(program).extension().is_none() {
      candidates.extend([".exe", ".cmd", ".bat"].map(|extension| format!("{program}{extension}")));
    }
  }
  candidates
}

fn apply_path_prepend(command: &mut Command, path_prepend: &[PathBuf]) {
  if path_prepend.is_empty() {
    return;
  }
  let mut paths = path_prepend.to_vec();
  paths.extend(env::var_os("PATH").map(|value| env::split_paths(&value).collect::<Vec<_>>()).unwrap_or_default());
  if let Ok(joined) = env::join_paths(paths) {
    command.env("PATH", joined);
  }
}

fn apply_env(command: &mut Command, env_vars: &[(String, String)]) {
  for (name, value) in env_vars {
    command.env(name, value);
  }
}

#[cfg(unix)]
fn configure_child_process(command: &mut Command) {
  use std::os::unix::process::CommandExt;

  command.process_group(0);
}

#[cfg(windows)]
fn configure_child_process(command: &mut Command) {
  use std::os::windows::process::CommandExt;

  const CREATE_NO_WINDOW: u32 = 0x08000000;
  command.creation_flags(CREATE_NO_WINDOW);
}

#[cfg(not(any(windows, unix)))]
fn configure_child_process(_: &mut Command) {}

#[cfg(windows)]
fn kill_process_tree(child: &mut Child) -> std::io::Result<()> {
  let pid = child.id().to_string();
  let mut command = Command::new("taskkill");
  configure_child_process(&mut command);
  let _ = command
    .args(["/PID", pid.as_str(), "/T", "/F"])
    .stdin(Stdio::null())
    .stdout(Stdio::null())
    .stderr(Stdio::null())
    .status();
  child.kill()
}

#[cfg(unix)]
fn kill_process_tree(child: &mut Child) -> std::io::Result<()> {
  let process_group = format!("-{}", child.id());
  let _ = Command::new("/bin/kill")
    .args(["-KILL", process_group.as_str()])
    .stdin(Stdio::null())
    .stdout(Stdio::null())
    .stderr(Stdio::null())
    .status();
  child.kill()
}

#[cfg(not(any(windows, unix)))]
fn kill_process_tree(child: &mut Child) -> std::io::Result<()> {
  child.kill()
}

#[cfg(test)]
mod tests {
  use super::*;
  use std::fs;
  use std::process::Command;
  use std::time::{SystemTime, UNIX_EPOCH};

  #[test]
  fn captures_stdout_from_local_executable() {
    let helper = TestHelper::compile();
    let work_dir = temp_dir("stdout-work");
    fs::create_dir_all(&work_dir).unwrap();
    let runtime = runtime_with_args(&helper.program, vec!["--stdout", "hello from helper"], CliPromptMode::Stdin);

    let result = runtime.run_task(AgentTaskRequest::new(&work_dir, "", 1_000)).unwrap();

    assert_eq!(result.status, AgentTaskStatus::Completed);
    assert_eq!(result.stdout.trim(), "hello from helper");
    assert_eq!(result.exit_code, Some(0));
    let _ = fs::remove_dir_all(work_dir);
  }

  #[test]
  fn resolves_bare_program_from_path_prepend() {
    let helper = TestHelper::compile();
    let work_dir = temp_dir("path-prepend-work");
    fs::create_dir_all(&work_dir).unwrap();
    let mut config =
      config_with_args(&helper.program_name(), vec!["--stdout", "found through path"], CliPromptMode::Stdin);
    config.path_prepend = vec![helper.program_dir()];
    let runtime = CliAgentRuntime::new(config);

    let result = runtime.run_task(AgentTaskRequest::new(&work_dir, "", 1_000)).unwrap();

    assert_eq!(result.status, AgentTaskStatus::Completed);
    assert_eq!(result.stdout.trim(), "found through path");
    let _ = fs::remove_dir_all(work_dir);
  }

  #[test]
  fn passes_prompt_through_stdin() {
    let helper = TestHelper::compile();
    let work_dir = temp_dir("stdin-work");
    fs::create_dir_all(&work_dir).unwrap();
    let runtime = runtime_with_args(&helper.program, vec!["--stdin-to-stdout"], CliPromptMode::Stdin);

    let result = runtime.run_task(AgentTaskRequest::new(&work_dir, "prompt body", 1_000)).unwrap();

    assert_eq!(result.status, AgentTaskStatus::Completed);
    assert_eq!(result.stdout, "prompt body");
    let _ = fs::remove_dir_all(work_dir);
  }

  #[test]
  fn passes_configured_environment() {
    let helper = TestHelper::compile();
    let work_dir = temp_dir("env-work");
    fs::create_dir_all(&work_dir).unwrap();
    let mut config = config_with_args(&helper.program, vec!["--env-to-stdout", "SOMA_TEST_ENV"], CliPromptMode::Stdin);
    config.env.push(("SOMA_TEST_ENV".to_string(), "configured".to_string()));
    let runtime = CliAgentRuntime::new(config);

    let result = runtime.run_task(AgentTaskRequest::new(&work_dir, "", 1_000)).unwrap();

    assert_eq!(result.status, AgentTaskStatus::Completed);
    assert_eq!(result.stdout.trim(), "configured");
    let _ = fs::remove_dir_all(work_dir);
  }

  #[test]
  fn reports_non_zero_exit_and_captures_stderr() {
    let helper = TestHelper::compile();
    let work_dir = temp_dir("failure-work");
    fs::create_dir_all(&work_dir).unwrap();
    let runtime =
      runtime_with_args(&helper.program, vec!["--stderr", "bad runtime", "--exit", "7"], CliPromptMode::Stdin);

    let result = runtime.run_task(AgentTaskRequest::new(&work_dir, "", 1_000)).unwrap();

    assert_eq!(result.status, AgentTaskStatus::Failed);
    assert_eq!(result.exit_code, Some(7));
    assert!(result.stderr.contains("bad runtime"));
    let _ = fs::remove_dir_all(work_dir);
  }

  #[cfg(windows)]
  #[test]
  fn windows_cli_child_does_not_open_console_window() {
    let helper = TestHelper::compile();
    let work_dir = temp_dir("no-console-window-work");
    fs::create_dir_all(&work_dir).unwrap();
    let runtime = runtime_with_args(&helper.program, vec!["--console-window-present"], CliPromptMode::Stdin);

    let result = runtime.run_task(AgentTaskRequest::new(&work_dir, "", 1_000)).unwrap();

    assert_eq!(result.status, AgentTaskStatus::Completed);
    assert_eq!(result.stdout.trim(), "false");
    let _ = fs::remove_dir_all(work_dir);
  }

  #[test]
  fn times_out_and_returns_without_hanging() {
    let helper = TestHelper::compile();
    let work_dir = temp_dir("timeout-work");
    fs::create_dir_all(&work_dir).unwrap();
    let runtime = runtime_with_args(&helper.program, vec!["--sleep-ms", "2000"], CliPromptMode::Stdin);
    let started = Instant::now();

    let result = runtime.run_task(AgentTaskRequest::new(&work_dir, "", 80)).unwrap();

    assert_eq!(result.status, AgentTaskStatus::TimedOut);
    assert!(started.elapsed() < Duration::from_secs(2));
    let _ = fs::remove_dir_all(work_dir);
  }

  #[test]
  fn cancellation_kills_the_runtime_without_waiting_for_timeout() {
    let helper = TestHelper::compile();
    let work_dir = temp_dir("cancel-work");
    fs::create_dir_all(&work_dir).unwrap();
    let runtime = runtime_with_args(&helper.program, vec!["--sleep-ms", "5000"], CliPromptMode::Stdin);
    let cancellation = crate::agent::AgentTaskCancellation::new();
    let request = AgentTaskRequest::new(&work_dir, "", 10_000).with_cancellation(cancellation.clone());
    let started = Instant::now();

    let task = std::thread::spawn(move || runtime.run_task(request).unwrap());
    std::thread::sleep(Duration::from_millis(80));
    cancellation.cancel();
    let result = task.join().unwrap();

    assert_eq!(result.status, AgentTaskStatus::Cancelled);
    assert!(started.elapsed() < Duration::from_secs(2));
    let _ = fs::remove_dir_all(work_dir);
  }

  #[test]
  fn reports_truncation_for_bounded_stdout_and_stderr_capture() {
    let helper = TestHelper::compile();
    let work_dir = temp_dir("capture-limit-work");
    fs::create_dir_all(&work_dir).unwrap();
    let mut config = config_with_args(
      &helper.program,
      vec!["--stdout-repeat", "x", "1024", "--stderr-repeat", "y", "1024"],
      CliPromptMode::Stdin,
    );
    config.max_output_bytes = 64;
    let runtime = CliAgentRuntime::new(config);

    let result = runtime.run_task(AgentTaskRequest::new(&work_dir, "", 1_000)).unwrap();

    assert_eq!(result.status, AgentTaskStatus::Completed);
    assert_eq!(result.stdout.len(), 64);
    assert_eq!(result.stderr.len(), 64);
    assert!(result.stdout_truncated);
    assert!(result.stderr_truncated);
    let _ = fs::remove_dir_all(work_dir);
  }

  #[test]
  fn exact_output_capture_limit_is_not_truncated() {
    let helper = TestHelper::compile();
    let work_dir = temp_dir("exact-capture-limit-work");
    fs::create_dir_all(&work_dir).unwrap();
    let mut config = config_with_args(&helper.program, vec!["--stdout-repeat", "x", "64"], CliPromptMode::Stdin);
    config.max_output_bytes = 64;
    let runtime = CliAgentRuntime::new(config);

    let result = runtime.run_task(AgentTaskRequest::new(&work_dir, "", 1_000)).unwrap();

    assert_eq!(result.stdout.len(), 64);
    assert!(!result.stdout_truncated);
    let _ = fs::remove_dir_all(work_dir);
  }

  #[test]
  fn passes_configured_arg_without_shell_interpolation() {
    let helper = TestHelper::compile();
    let work_dir = temp_dir("arg-prompt-work");
    fs::create_dir_all(&work_dir).unwrap();
    let prompt = "literal && echo hacked";
    let runtime = runtime_with_args(
      &helper.program,
      vec!["--arg-to-stdout", "{prompt}"],
      CliPromptMode::Arg { placeholder: "{prompt}".to_string() },
    );

    let result = runtime.run_task(AgentTaskRequest::new(&work_dir, prompt, 1_000)).unwrap();

    assert_eq!(result.status, AgentTaskStatus::Completed);
    assert_eq!(result.stdout.trim(), prompt);
    let _ = fs::remove_dir_all(work_dir);
  }

  #[test]
  fn missing_arg_placeholder_is_typed_config_error() {
    let helper = TestHelper::compile();
    let work_dir = temp_dir("bad-config-work");
    fs::create_dir_all(&work_dir).unwrap();
    let runtime = runtime_with_args(
      &helper.program,
      vec!["--arg-to-stdout"],
      CliPromptMode::Arg { placeholder: "{prompt}".to_string() },
    );

    let error = runtime.run_task(AgentTaskRequest::new(&work_dir, "prompt", 1_000)).unwrap_err();

    assert!(matches!(
        error,
        AiRuntimeError::InvalidAgentConfig { provider, .. }
            if provider == ProviderId::from("test_cli")
    ));
    let _ = fs::remove_dir_all(work_dir);
  }

  fn runtime_with_args(program: &str, args: Vec<&str>, prompt_mode: CliPromptMode) -> CliAgentRuntime {
    CliAgentRuntime::new(config_with_args(program, args, prompt_mode))
  }

  fn config_with_args(program: &str, args: Vec<&str>, prompt_mode: CliPromptMode) -> CliAgentConfig {
    CliAgentConfig::new("test_cli", program.to_string(), args.into_iter().map(str::to_string).collect(), prompt_mode)
  }

  struct TestHelper {
    root: PathBuf,
    program: String,
  }

  impl TestHelper {
    fn compile() -> Self {
      let root = temp_dir("cli-helper");
      fs::create_dir_all(&root).unwrap();
      let source = root.join("helper.rs");
      let executable = root.join(if cfg!(windows) { "helper.exe" } else { "helper" });
      fs::write(&source, helper_source()).unwrap();
      let rustc = std::env::var("RUSTC").unwrap_or_else(|_| "rustc".to_string());
      let output = Command::new(rustc).arg(&source).arg("-o").arg(&executable).output().unwrap();
      assert!(output.status.success(), "helper compile failed: {}", String::from_utf8_lossy(&output.stderr));
      Self { root, program: executable.to_string_lossy().to_string() }
    }

    fn program_dir(&self) -> PathBuf {
      PathBuf::from(&self.program).parent().unwrap().to_path_buf()
    }

    fn program_name(&self) -> String {
      PathBuf::from(&self.program).file_name().unwrap().to_string_lossy().to_string()
    }
  }

  impl Drop for TestHelper {
    fn drop(&mut self) {
      let _ = fs::remove_dir_all(&self.root);
    }
  }

  fn temp_dir(label: &str) -> PathBuf {
    let nanos = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
    std::env::temp_dir().join(format!("soma-ai-runtime-{label}-{}-{nanos}", std::process::id()))
  }

  fn helper_source() -> &'static str {
    r#"
use std::env;
use std::io::{self, Read, Write};
use std::thread;
use std::time::Duration;
#[cfg(windows)]
#[link(name = "Kernel32")]
extern "system" {
    fn GetConsoleWindow() -> *mut std::ffi::c_void;
}

#[cfg(windows)]
fn console_window_present() -> bool {
    unsafe { !GetConsoleWindow().is_null() }
}

#[cfg(not(windows))]
fn console_window_present() -> bool {
    false
}


fn main() {
    let mut args = env::args().skip(1);
    let mut exit_code = 0_i32;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--stdout" => println!("{}", args.next().unwrap_or_default()),
            "--stderr" => eprintln!("{}", args.next().unwrap_or_default()),
            "--console-window-present" => println!("{}", console_window_present()),
            "--exit" => {
                exit_code = args.next().unwrap_or_default().parse().unwrap_or(1);
            }
            "--sleep-ms" => {
                let ms = args.next().unwrap_or_default().parse().unwrap_or(0);
                thread::sleep(Duration::from_millis(ms));
            }
            "--stdin-to-stdout" => {
                let mut input = String::new();
                io::stdin().read_to_string(&mut input).unwrap();
                print!("{}", input);
            }
            "--arg-to-stdout" => println!("{}", args.next().unwrap_or_default()),
            "--env-to-stdout" => {
                let name = args.next().unwrap_or_default();
                println!("{}", env::var(name).unwrap_or_default());
            }
            "--stdout-repeat" => {
                let value = args.next().unwrap_or_default();
                let count: usize = args.next().unwrap_or_default().parse().unwrap_or(0);
                for _ in 0..count {
                    print!("{}", value);
                }
                io::stdout().flush().unwrap();
            }
            "--stderr-repeat" => {
                let value = args.next().unwrap_or_default();
                let count: usize = args.next().unwrap_or_default().parse().unwrap_or(0);
                for _ in 0..count {
                    eprint!("{}", value);
                }
                io::stderr().flush().unwrap();
            }
            other => println!("arg:{other}"),
        }
    }
    std::process::exit(exit_code);
}
"#
  }
}
