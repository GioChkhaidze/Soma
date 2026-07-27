use serde::{Deserialize, Serialize};
use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct ProviderId(String);

impl ProviderId {
  pub fn new(value: impl Into<String>) -> Self {
    Self(value.into())
  }

  pub fn as_str(&self) -> &str {
    &self.0
  }
}

impl From<&str> for ProviderId {
  fn from(value: &str) -> Self {
    Self::new(value)
  }
}

impl From<String> for ProviderId {
  fn from(value: String) -> Self {
    Self::new(value)
  }
}

impl fmt::Display for ProviderId {
  fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
    formatter.write_str(&self.0)
  }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct ModelId(String);

impl ModelId {
  pub fn new(value: impl Into<String>) -> Self {
    Self(value.into())
  }

  pub fn as_str(&self) -> &str {
    &self.0
  }
}

impl From<&str> for ModelId {
  fn from(value: &str) -> Self {
    Self::new(value)
  }
}

impl From<String> for ModelId {
  fn from(value: String) -> Self {
    Self::new(value)
  }
}

impl fmt::Display for ModelId {
  fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
    formatter.write_str(&self.0)
  }
}
