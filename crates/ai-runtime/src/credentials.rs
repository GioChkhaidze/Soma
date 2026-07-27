use crate::errors::AiRuntimeError;
use crate::ids::ProviderId;
use serde::{Deserialize, Serialize};
use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum CredentialRef {
  ApiKey { provider: ProviderId, profile: String },
  None,
}

impl fmt::Display for CredentialRef {
  fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
    match self {
      Self::ApiKey { provider, profile } => {
        write!(formatter, "api_key:{provider}/{profile}")
      }
      Self::None => formatter.write_str("none"),
    }
  }
}

pub trait CredentialResolver {
  fn resolve(&self, credential: &CredentialRef) -> Result<Option<String>, AiRuntimeError>;
}

#[derive(Debug, Default, Clone, Copy)]
pub struct NoopCredentialResolver;

impl CredentialResolver for NoopCredentialResolver {
  fn resolve(&self, _credential: &CredentialRef) -> Result<Option<String>, AiRuntimeError> {
    Ok(None)
  }
}
