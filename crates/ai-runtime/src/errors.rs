use crate::credentials::CredentialRef;
use crate::ids::ProviderId;
use std::error::Error;
use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AiRuntimeError {
  MissingCredential { credential: CredentialRef },
  HttpStatus { provider: ProviderId, status: u16 },
  Timeout { provider: ProviderId, message: String },
  InvalidAgentConfig { provider: ProviderId, message: String },
  CredentialResolution { credential: CredentialRef, message: String },
  ProviderExecution { provider: ProviderId, message: String },
  ResponseBodyTooLarge { provider: ProviderId, limit_bytes: u64 },
  InvalidProviderResponse { provider: ProviderId, message: String },
}

impl fmt::Display for AiRuntimeError {
  fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
    match self {
      Self::MissingCredential { credential } => {
        write!(formatter, "missing credential `{credential}`")
      }
      Self::HttpStatus { provider, status } => {
        write!(formatter, "AI provider `{provider}` returned HTTP status {status}")
      }
      Self::Timeout { provider, message } => {
        write!(formatter, "AI provider `{provider}` timed out: {message}")
      }
      Self::InvalidAgentConfig { provider, message } => {
        write!(formatter, "AI provider `{provider}` has invalid CLI agent config: {message}")
      }
      Self::CredentialResolution { credential, message } => {
        write!(formatter, "could not resolve credential `{credential}`: {message}")
      }
      Self::ProviderExecution { provider, message } => {
        write!(formatter, "AI provider `{provider}` failed: {message}")
      }
      Self::ResponseBodyTooLarge { provider, limit_bytes } => {
        write!(formatter, "AI provider `{provider}` response exceeded {limit_bytes} bytes")
      }
      Self::InvalidProviderResponse { provider, message } => {
        write!(formatter, "AI provider `{provider}` returned an invalid response: {message}")
      }
    }
  }
}

impl Error for AiRuntimeError {}
