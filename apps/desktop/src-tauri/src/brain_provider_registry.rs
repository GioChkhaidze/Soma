#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BrainProviderAdapter {
  LocalOpenAiCompatible,
  OpenAiCompatibleApi,
  AnthropicMessages,
  CodexSdk,
  ClaudeCode,
  Managed,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct BrainProviderSpec {
  pub id: &'static str,
  pub adapter: BrainProviderAdapter,
  pub default_endpoint: Option<&'static str>,
}

pub(crate) const DEFAULT_PROVIDER_ID: &str = "codex_sdk";

use BrainProviderAdapter::*;

pub(crate) const BRAIN_PROVIDER_REGISTRY: &[BrainProviderSpec] = &[
  provider("local_llm", LocalOpenAiCompatible, None),
  provider("ollama", LocalOpenAiCompatible, Some("http://localhost:11434/v1")),
  provider("lm_studio", LocalOpenAiCompatible, Some("http://localhost:1234/v1")),
  provider("vllm", LocalOpenAiCompatible, Some("http://localhost:8000/v1")),
  provider("openai_compatible", OpenAiCompatibleApi, None),
  provider("openrouter", OpenAiCompatibleApi, Some("https://openrouter.ai/api/v1")),
  provider("vercel_ai_gateway", OpenAiCompatibleApi, Some("https://ai-gateway.vercel.sh/v1")),
  provider("gemini", OpenAiCompatibleApi, Some("https://generativelanguage.googleapis.com/v1beta/openai")),
  provider("openai", OpenAiCompatibleApi, Some("https://api.openai.com/v1")),
  provider("claude", AnthropicMessages, Some("https://api.anthropic.com/v1")),
  provider("deepseek", OpenAiCompatibleApi, Some("https://api.deepseek.com/chat/completions")),
  provider("zai", OpenAiCompatibleApi, Some("https://api.z.ai/api/paas/v4")),
  provider("moonshot", OpenAiCompatibleApi, Some("https://api.moonshot.ai/v1")),
  provider("minimax", OpenAiCompatibleApi, Some("https://api.minimax.io/v1")),
  provider("mistral", OpenAiCompatibleApi, Some("https://api.mistral.ai/v1")),
  provider("groq", OpenAiCompatibleApi, Some("https://api.groq.com/openai/v1")),
  provider("xai", OpenAiCompatibleApi, Some("https://api.x.ai/v1")),
  provider("together", OpenAiCompatibleApi, Some("https://api.together.xyz/v1")),
  provider("fireworks", OpenAiCompatibleApi, Some("https://api.fireworks.ai/inference/v1")),
  provider("cerebras", OpenAiCompatibleApi, Some("https://api.cerebras.ai/v1")),
  provider("codex_sdk", CodexSdk, None),
  provider("claude_code", ClaudeCode, None),
  provider("soma_cloud", Managed, None),
];

const fn provider(
  id: &'static str,
  adapter: BrainProviderAdapter,
  default_endpoint: Option<&'static str>,
) -> BrainProviderSpec {
  BrainProviderSpec { id, adapter, default_endpoint }
}

pub(crate) fn brain_provider(id: &str) -> Option<&'static BrainProviderSpec> {
  BRAIN_PROVIDER_REGISTRY.iter().find(|provider| provider.id == id)
}

pub(crate) fn is_known_provider(id: &str) -> bool {
  brain_provider(id).is_some()
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn registry_has_unique_provider_ids() {
    let mut ids = BRAIN_PROVIDER_REGISTRY.iter().map(|provider| provider.id).collect::<Vec<_>>();
    ids.sort_unstable();
    ids.dedup();

    assert_eq!(ids.len(), BRAIN_PROVIDER_REGISTRY.len());
    assert!(is_known_provider(DEFAULT_PROVIDER_ID));
    assert_eq!(
      brain_provider("claude").map(|provider| provider.adapter),
      Some(BrainProviderAdapter::AnthropicMessages)
    );
    assert_eq!(
      brain_provider("openrouter").and_then(|provider| provider.default_endpoint),
      Some("https://openrouter.ai/api/v1")
    );
    assert_eq!(
      brain_provider("vercel_ai_gateway").and_then(|provider| provider.default_endpoint),
      Some("https://ai-gateway.vercel.sh/v1")
    );
  }
}
