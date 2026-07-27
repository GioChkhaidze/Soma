use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MessageRole {
  System,
  User,
  Assistant,
  Tool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", content = "value", rename_all = "snake_case")]
pub enum ContentPart {
  Text(String),
  Json(serde_json::Value),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AiMessage {
  pub role: MessageRole,
  pub content: Vec<ContentPart>,
}

impl AiMessage {
  pub fn new(role: MessageRole, content: Vec<ContentPart>) -> Self {
    Self { role, content }
  }

  pub fn system_text(content: impl Into<String>) -> Self {
    Self::text(MessageRole::System, content)
  }

  pub fn user_text(content: impl Into<String>) -> Self {
    Self::text(MessageRole::User, content)
  }

  pub fn assistant_text(content: impl Into<String>) -> Self {
    Self::text(MessageRole::Assistant, content)
  }

  pub fn text(role: MessageRole, content: impl Into<String>) -> Self {
    Self { role, content: vec![ContentPart::Text(content.into())] }
  }
}
