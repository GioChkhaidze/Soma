use crate::ids::ModelId;
use crate::message::AiMessage;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AiRequest {
  pub model: ModelId,
  pub messages: Vec<AiMessage>,
  pub temperature: Option<f32>,
  pub max_output_tokens: Option<u32>,
  pub timeout_ms: Option<u64>,
}

impl AiRequest {
  pub fn new(model: ModelId, messages: Vec<AiMessage>) -> Self {
    Self { model, messages, temperature: None, max_output_tokens: None, timeout_ms: None }
  }
}
