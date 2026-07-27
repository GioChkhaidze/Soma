use serde::Serialize;

pub(crate) const STORAGE_BUSY_MESSAGE: &str = "Soma is busy finishing another local write. Try again in a moment.";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeFailureKind {
  Unsupported,
  Configuration,
  Credential,
  Unavailable,
  Busy,
  Timeout,
  InvalidResponse,
  Execution,
}

impl RuntimeFailureKind {
  const fn code(self) -> &'static str {
    match self {
      Self::Unsupported => "SOMA_RUNTIME_UNSUPPORTED",
      Self::Configuration => "SOMA_RUNTIME_CONFIGURATION",
      Self::Credential => "SOMA_RUNTIME_CREDENTIAL",
      Self::Unavailable => "SOMA_RUNTIME_UNAVAILABLE",
      Self::Busy => "SOMA_RUNTIME_BUSY",
      Self::Timeout => "SOMA_RUNTIME_TIMEOUT",
      Self::InvalidResponse => "SOMA_RUNTIME_INVALID_RESPONSE",
      Self::Execution => "SOMA_RUNTIME_EXECUTION",
    }
  }
}

#[derive(Debug, Serialize)]
pub struct CommandError {
  pub code: String,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub kind: Option<RuntimeFailureKind>,
  pub message: String,
}

pub type CommandResult<T = serde_json::Value> = Result<T, CommandError>;

impl CommandError {
  pub fn new(code: impl Into<String>, message: impl Into<String>) -> Self {
    Self { code: code.into(), kind: None, message: message.into() }
  }

  pub fn runtime(kind: RuntimeFailureKind, message: impl Into<String>) -> Self {
    Self { code: kind.code().to_string(), kind: Some(kind), message: message.into() }
  }

  pub fn validation(message: impl Into<String>) -> Self {
    Self::new("Soma_VALIDATION_ERROR", message)
  }

  pub fn not_found(message: impl Into<String>) -> Self {
    Self::new("Soma_NOT_FOUND", message)
  }

  pub fn storage(message: impl Into<String>) -> Self {
    let message = message.into();
    if is_storage_busy_message(&message) {
      return Self::new("Soma_STORAGE_BUSY", STORAGE_BUSY_MESSAGE);
    }
    Self::new("Soma_STORAGE_ERROR", message)
  }

  pub fn runtime_failure_kind(&self) -> RuntimeFailureKind {
    self.kind.unwrap_or_else(|| {
      if is_storage_busy_message(&self.message) {
        RuntimeFailureKind::Busy
      } else {
        RuntimeFailureKind::Execution
      }
    })
  }
}

impl From<rusqlite::Error> for CommandError {
  fn from(value: rusqlite::Error) -> Self {
    Self::storage(value.to_string())
  }
}

impl From<std::io::Error> for CommandError {
  fn from(value: std::io::Error) -> Self {
    Self::storage(value.to_string())
  }
}

impl From<time::error::Format> for CommandError {
  fn from(value: time::error::Format) -> Self {
    Self::storage(value.to_string())
  }
}

pub(crate) fn is_storage_busy_message(message: &str) -> bool {
  let message = message.to_ascii_lowercase();
  message.contains("sqlite_busy")
    || message.contains("sqlite_locked")
    || message.contains("database is locked")
    || message.contains("database table is locked")
    || message.contains("database schema is locked")
    || message.contains("soma is busy finishing another local write")
    || message.contains("write lock was poisoned")
    || (message.contains("sqlite") && message.contains("locked"))
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn storage_errors_hide_raw_sqlite_lock_messages() {
    for message in [
      "Error: database is locked",
      "SQLITE_BUSY: database is locked",
      "sqlite database is locked",
      "database table is locked",
      "database schema is locked",
      "SQLite write lock was poisoned.",
    ] {
      let error = CommandError::storage(message);

      assert_eq!(error.code, "Soma_STORAGE_BUSY");
      assert_eq!(error.message, STORAGE_BUSY_MESSAGE);
      assert!(!error.message.contains("database is locked"));
    }
  }

  #[test]
  fn runtime_errors_publish_stable_machine_readable_kind_and_code() {
    let error = CommandError::runtime(RuntimeFailureKind::Timeout, "provider timed out");
    let value = serde_json::to_value(&error).unwrap();

    assert_eq!(value["code"], "SOMA_RUNTIME_TIMEOUT");
    assert_eq!(value["kind"], "timeout");
    assert_eq!(value["message"], "provider timed out");
  }
}
