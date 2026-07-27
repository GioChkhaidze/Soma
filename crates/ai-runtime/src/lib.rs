//! Application-agnostic AI provider runtime contracts for Soma and other callers.
//!
//! This crate intentionally has no dependency on Soma graph, job-folder, Tauri,
//! storage, desktop settings, or UI code.

#![forbid(unsafe_code)]

pub mod adapters;
pub mod agent;
pub mod credentials;
pub mod errors;
pub mod ids;
pub mod message;
pub mod request;
pub mod response;

pub use adapters::anthropic_messages::{AnthropicMessagesConfig, AnthropicMessagesProvider};
pub use adapters::cli_agent::{CliAgentConfig, CliAgentRuntime, CliPromptMode};
pub use adapters::openai_compatible::{OpenAiCompatibleConfig, OpenAiCompatibleProvider};
pub use agent::{AgentTaskRequest, AgentTaskResult, AgentTaskStatus};
pub use credentials::{CredentialRef, CredentialResolver, NoopCredentialResolver};
pub use errors::AiRuntimeError;
pub use ids::{ModelId, ProviderId};
pub use message::{AiMessage, ContentPart, MessageRole};
pub use request::AiRequest;
pub use response::{AiResponse, FinishReason, TokenUsage};
