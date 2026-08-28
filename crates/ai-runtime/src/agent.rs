use std::path::PathBuf;
use std::sync::{
  atomic::{AtomicBool, Ordering},
  Arc,
};

#[derive(Debug, Clone)]
pub struct AgentTaskRequest {
  pub working_dir: PathBuf,
  pub prompt: String,
  pub timeout_ms: u64,
  pub cancellation: AgentTaskCancellation,
}

impl AgentTaskRequest {
  pub fn new(working_dir: impl Into<PathBuf>, prompt: impl Into<String>, timeout_ms: u64) -> Self {
    Self {
      working_dir: working_dir.into(),
      prompt: prompt.into(),
      timeout_ms,
      cancellation: AgentTaskCancellation::new(),
    }
  }

  pub fn with_cancellation(mut self, cancellation: AgentTaskCancellation) -> Self {
    self.cancellation = cancellation;
    self
  }
}

#[derive(Debug, Clone, Default)]
pub struct AgentTaskCancellation(Arc<AtomicBool>);

impl AgentTaskCancellation {
  pub fn new() -> Self {
    Self::default()
  }

  pub fn cancel(&self) {
    self.0.store(true, Ordering::Release);
  }

  pub fn is_cancelled(&self) -> bool {
    self.0.load(Ordering::Acquire)
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
  Cancelled,
}
