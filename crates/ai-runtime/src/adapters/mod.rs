use std::error::Error as _;
use std::io::Read;

use crate::errors::AiRuntimeError;
use crate::ids::ProviderId;

pub mod anthropic_messages;
pub mod cli_agent;
pub mod openai_compatible;

pub(crate) const PROVIDER_RESPONSE_BODY_MAX_BYTES: u64 = 4 * 1024 * 1024;

pub(crate) fn handle_provider_response(
  provider: &ProviderId,
  response: Result<ureq::Response, ureq::Error>,
) -> Result<ureq::Response, AiRuntimeError> {
  match response {
    Ok(response) => Ok(response),
    Err(ureq::Error::Status(status, _response)) => {
      Err(AiRuntimeError::HttpStatus { provider: provider.clone(), status })
    }
    Err(ureq::Error::Transport(error)) => {
      let timed_out = transport_timed_out(&error);
      let message = error.to_string();
      if timed_out {
        return Err(AiRuntimeError::Timeout { provider: provider.clone(), message });
      }
      Err(AiRuntimeError::ProviderExecution { provider: provider.clone(), message })
    }
  }
}

fn transport_timed_out(error: &ureq::Transport) -> bool {
  let message = error.to_string().to_ascii_lowercase();
  if message.contains("timed out") || message.contains("timeout") {
    return true;
  }

  let mut source = error.source();
  while let Some(cause) = source {
    if cause
      .downcast_ref::<std::io::Error>()
      .is_some_and(|error| matches!(error.kind(), std::io::ErrorKind::TimedOut | std::io::ErrorKind::WouldBlock))
    {
      return true;
    }
    source = cause.source();
  }
  false
}

pub(crate) fn read_provider_response_body(
  provider: &ProviderId,
  response: ureq::Response,
) -> Result<String, AiRuntimeError> {
  let mut reader = response.into_reader().take(PROVIDER_RESPONSE_BODY_MAX_BYTES + 1);
  let mut raw = Vec::new();
  reader.read_to_end(&mut raw).map_err(|error| AiRuntimeError::InvalidProviderResponse {
    provider: provider.clone(),
    message: format!("could not read response body: {error}"),
  })?;
  if raw.len() as u64 > PROVIDER_RESPONSE_BODY_MAX_BYTES {
    return Err(AiRuntimeError::ResponseBodyTooLarge {
      provider: provider.clone(),
      limit_bytes: PROVIDER_RESPONSE_BODY_MAX_BYTES,
    });
  }
  String::from_utf8(raw).map_err(|error| AiRuntimeError::InvalidProviderResponse {
    provider: provider.clone(),
    message: format!("response body is not valid UTF-8: {error}"),
  })
}
