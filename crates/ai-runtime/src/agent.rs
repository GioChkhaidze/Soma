use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentTaskRequest {
  pub working_dir: PathBuf,
  pub prompt: String,
  pub timeout_ms: u64,
}

impl AgentTaskRequest {
  pub fn new(working_dir: impl Into<PathBuf>, prompt: impl Into<String>, timeout_ms: u64) -> Self {
    Self { working_dir: working_dir.into(), prompt: prompt.into(), timeout_ms }
  }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentTaskResult {
  pub status: AgentTaskStatus,
  pub stdout: String,
  pub stdout_truncated: bool,
  pub stderr: String,
  pub stderr_truncated: bool,
  pub exit_code: Option<i32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentTaskStatus {
  Completed,
  Failed,
  TimedOut,
}
