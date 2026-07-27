use crate::ids::{ModelId, ProviderId};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AiResponse {
  pub content: String,
  pub finish_reason: FinishReason,
  pub usage: Option<TokenUsage>,
  pub provider: ProviderId,
  pub model: ModelId,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FinishReason {
  Stop,
  Length,
  ContentFilter,
  ToolCalls,
  Error,
  Other(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TokenUsage {
  pub input_tokens: u32,
  pub output_tokens: u32,
  pub total_tokens: u32,
}
