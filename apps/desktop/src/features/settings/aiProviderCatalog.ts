import type { BrainProviderId } from '../../../../../packages/contracts/src/appCommands.ts';

export type AiProviderId = BrainProviderId;

export type AiProviderGroupId = 'local' | 'provider' | 'agent';

export type AiModelOption = {
  id: string;
  label: string;
  note?: string;
  source?: 'catalog' | 'live' | 'saved';
};

export type AiProviderOption = {
  id: AiProviderId;
  groupId: AiProviderGroupId;
  name: string;
  shortName: string;
  description: string;
  status: string;
  credentialLabel?: string;
  modelPlaceholder: string;
  endpointPlaceholder?: string;
  endpointDefault?: string;
  models: AiModelOption[];
};

type AiProviderGroup = {
  id: AiProviderGroupId;
  title: string;
  description: string;
  providers: AiProviderOption[];
};

const openCatalogModels: AiModelOption[] = [
  { id: 'openai/gpt-5.5', label: 'GPT-5.5', note: 'OpenAI' },
  { id: 'anthropic/claude-opus-4.8', label: 'Claude Opus 4.8', note: 'Anthropic' },
  { id: 'anthropic/claude-sonnet-4.5', label: 'Claude Sonnet 4.5', note: 'Anthropic' },
  { id: 'xai/grok-4.3', label: 'Grok 4.3', note: 'xAI' },
  { id: 'zhipuai/glm-5.2', label: 'GLM 5.2', note: 'Z.AI' },
  { id: 'moonshotai/kimi-k2.6', label: 'Kimi K2.6', note: 'Moonshot' },
  { id: 'minimax/MiniMax-M3', label: 'MiniMax M3', note: 'MiniMax' },
  { id: 'google/gemma-4-31b-it', label: 'Gemma 4 31B', note: 'Google' },
  { id: 'deepseek/deepseek-chat', label: 'DeepSeek Chat', note: 'DeepSeek' },
  { id: 'meta/llama-4-maverick', label: 'Llama 4 Maverick', note: 'Meta' }
];

const claudeModels: AiModelOption[] = [
  { id: 'claude-sonnet-5', label: 'Claude Sonnet 5', note: 'Anthropic' },
  { id: 'claude-opus-4-8', label: 'Claude Opus 4.8', note: 'Anthropic' },
  { id: 'claude-sonnet-4-5', label: 'Claude Sonnet 4.5', note: 'Anthropic' },
  { id: 'claude-haiku-4-5-20251001', label: 'Claude Haiku 4.5', note: 'Anthropic' }
];

const localStarterModels: AiModelOption[] = [
  { id: 'llama3.3', label: 'Llama 3.3', note: 'Ollama' },
  { id: 'qwen3', label: 'Qwen 3', note: 'Ollama' },
  { id: 'gemma3', label: 'Gemma 3', note: 'Ollama' },
  { id: 'mistral', label: 'Mistral', note: 'Ollama' },
  { id: 'deepseek-r1', label: 'DeepSeek R1', note: 'Ollama' }
];

export const aiProviderGroups: AiProviderGroup[] = [
  {
    id: 'local',
    title: 'Local',
    description: 'Use a model server running on this machine.',
    providers: [
      {
        id: 'ollama',
        groupId: 'local',
        name: 'Ollama',
        shortName: 'Ollama',
        description: 'Local Ollama models through its OpenAI-compatible server.',
        status: 'Local endpoint',
        modelPlaceholder: 'llama3.3',
        endpointDefault: 'http://localhost:11434/v1',
        endpointPlaceholder: 'http://localhost:11434/v1',
        models: localStarterModels
      },
      {
        id: 'lm_studio',
        groupId: 'local',
        name: 'LM Studio',
        shortName: 'LM Studio',
        description: 'Use the model currently served by LM Studio.',
        status: 'Local endpoint',
        modelPlaceholder: 'loaded local model',
        endpointDefault: 'http://localhost:1234/v1',
        endpointPlaceholder: 'http://localhost:1234/v1',
        models: [
          { id: 'local-model', label: 'Current loaded model', note: 'LM Studio' },
          ...localStarterModels
        ]
      },
      {
        id: 'vllm',
        groupId: 'local',
        name: 'vLLM',
        shortName: 'vLLM',
        description: 'Use a vLLM OpenAI-compatible server.',
        status: 'Local endpoint',
        modelPlaceholder: 'served-model-name',
        endpointDefault: 'http://localhost:8000/v1',
        endpointPlaceholder: 'http://localhost:8000/v1',
        models: [
          { id: 'served-model-name', label: 'Configured served model', note: 'vLLM' },
          { id: 'meta-llama/Llama-3.3-70B-Instruct', label: 'Llama 3.3 70B Instruct', note: 'vLLM' },
          { id: 'google/gemma-3-27b-it', label: 'Gemma 3 27B', note: 'vLLM' },
          { id: 'Qwen/Qwen3-32B', label: 'Qwen3 32B', note: 'vLLM' }
        ]
      },
      {
        id: 'local_llm',
        groupId: 'local',
        name: 'Other local endpoint',
        shortName: 'Custom local',
        description: 'Any local server that accepts OpenAI chat-completions requests.',
        status: 'Endpoint required',
        modelPlaceholder: 'served model id',
        endpointPlaceholder: 'http://localhost:8000/v1',
        models: localStarterModels
      }
    ]
  },
  {
    id: 'provider',
    title: 'Providers',
    description: 'Use hosted models or gateways with a stored API key.',
    providers: [
      {
        id: 'openrouter',
        groupId: 'provider',
        name: 'OpenRouter',
        shortName: 'OpenRouter',
        description: 'Broad hosted model catalog behind one OpenAI-compatible API.',
        status: 'Catalog gateway',
        credentialLabel: 'OpenRouter API key',
        modelPlaceholder: 'openai/gpt-5.5',
        endpointDefault: 'https://openrouter.ai/api/v1',
        endpointPlaceholder: 'https://openrouter.ai/api/v1',
        models: openCatalogModels
      },
      {
        id: 'vercel_ai_gateway',
        groupId: 'provider',
        name: 'Vercel AI Gateway',
        shortName: 'Vercel Gateway',
        description: 'Vercel gateway catalog with provider routing and model fallbacks.',
        status: 'Catalog gateway',
        credentialLabel: 'Vercel AI Gateway key',
        modelPlaceholder: 'xai/grok-4.3',
        endpointDefault: 'https://ai-gateway.vercel.sh/v1',
        endpointPlaceholder: 'https://ai-gateway.vercel.sh/v1',
        models: openCatalogModels
      },
      {
        id: 'openai',
        groupId: 'provider',
        name: 'OpenAI',
        shortName: 'OpenAI',
        description: 'OpenAI models through the standard OpenAI API.',
        status: 'Direct API',
        credentialLabel: 'OpenAI API key',
        modelPlaceholder: 'gpt-5.5',
        endpointDefault: 'https://api.openai.com/v1',
        endpointPlaceholder: 'https://api.openai.com/v1',
        models: [
          { id: 'gpt-5.5', label: 'GPT-5.5' },
          { id: 'gpt-5.4', label: 'GPT-5.4' },
          { id: 'gpt-5.4-mini', label: 'GPT-5.4 mini' },
          { id: 'gpt-5.4-nano', label: 'GPT-5.4 nano' }
        ]
      },
      {
        id: 'claude',
        groupId: 'provider',
        name: 'Claude',
        shortName: 'Claude',
        description: 'Claude models through the direct Anthropic Messages API.',
        status: 'Direct API',
        credentialLabel: 'Anthropic API key',
        modelPlaceholder: 'claude-sonnet-5',
        endpointDefault: 'https://api.anthropic.com/v1',
        endpointPlaceholder: 'https://api.anthropic.com/v1',
        models: claudeModels
      },
      {
        id: 'gemini',
        groupId: 'provider',
        name: 'Gemini',
        shortName: 'Gemini',
        description: 'Gemini through Google AI Studio OpenAI-compatible endpoint.',
        status: 'Direct API',
        credentialLabel: 'Google AI API key',
        modelPlaceholder: 'gemini-3.5-flash',
        endpointDefault: 'https://generativelanguage.googleapis.com/v1beta/openai',
        endpointPlaceholder: 'https://generativelanguage.googleapis.com/v1beta/openai',
        models: [
          { id: 'gemini-3.5-flash', label: 'Gemini 3.5 Flash' },
          { id: 'gemini-3.5-pro', label: 'Gemini 3.5 Pro' },
          { id: 'gemini-3.1-flash-lite', label: 'Gemini 3.1 Flash Lite' }
        ]
      },
      {
        id: 'deepseek',
        groupId: 'provider',
        name: 'DeepSeek',
        shortName: 'DeepSeek',
        description: 'DeepSeek chat and reasoning models.',
        status: 'Direct API',
        credentialLabel: 'DeepSeek API key',
        modelPlaceholder: 'deepseek-chat',
        endpointDefault: 'https://api.deepseek.com/chat/completions',
        endpointPlaceholder: 'https://api.deepseek.com/chat/completions',
        models: [
          { id: 'deepseek-chat', label: 'DeepSeek Chat' },
          { id: 'deepseek-reasoner', label: 'DeepSeek Reasoner' }
        ]
      },
      {
        id: 'zai',
        groupId: 'provider',
        name: 'Z.AI',
        shortName: 'Z.AI',
        description: 'GLM models from Z.AI.',
        status: 'Direct API',
        credentialLabel: 'Z.AI API key',
        modelPlaceholder: 'glm-5.2',
        endpointDefault: 'https://api.z.ai/api/paas/v4',
        endpointPlaceholder: 'https://api.z.ai/api/paas/v4',
        models: [
          { id: 'glm-5.2', label: 'GLM 5.2' },
          { id: 'glm-5.1', label: 'GLM 5.1' },
          { id: 'glm-4.7', label: 'GLM 4.7' }
        ]
      },
      {
        id: 'moonshot',
        groupId: 'provider',
        name: 'Moonshot / Kimi',
        shortName: 'Kimi',
        description: 'Kimi models from Moonshot AI.',
        status: 'Direct API',
        credentialLabel: 'Moonshot API key',
        modelPlaceholder: 'kimi-k2.6',
        endpointDefault: 'https://api.moonshot.ai/v1',
        endpointPlaceholder: 'https://api.moonshot.ai/v1',
        models: [
          { id: 'kimi-k2.6', label: 'Kimi K2.6' },
          { id: 'kimi-k2', label: 'Kimi K2' },
          { id: 'moonshot-v1-128k', label: 'Moonshot v1 128K' }
        ]
      },
      {
        id: 'minimax',
        groupId: 'provider',
        name: 'MiniMax',
        shortName: 'MiniMax',
        description: 'MiniMax text models through its compatible API.',
        status: 'Direct API',
        credentialLabel: 'MiniMax API key',
        modelPlaceholder: 'MiniMax-M3',
        endpointDefault: 'https://api.minimax.io/v1',
        endpointPlaceholder: 'https://api.minimax.io/v1',
        models: [
          { id: 'MiniMax-M3', label: 'MiniMax M3' },
          { id: 'MiniMax-M2', label: 'MiniMax M2' },
          { id: 'MiniMax-Text-01', label: 'MiniMax Text 01' }
        ]
      },
      {
        id: 'mistral',
        groupId: 'provider',
        name: 'Mistral',
        shortName: 'Mistral',
        description: 'Mistral hosted models.',
        status: 'Direct API',
        credentialLabel: 'Mistral API key',
        modelPlaceholder: 'mistral-large-latest',
        endpointDefault: 'https://api.mistral.ai/v1',
        endpointPlaceholder: 'https://api.mistral.ai/v1',
        models: [
          { id: 'mistral-large-latest', label: 'Mistral Large' },
          { id: 'codestral-latest', label: 'Codestral' },
          { id: 'ministral-8b-latest', label: 'Ministral 8B' }
        ]
      },
      {
        id: 'groq',
        groupId: 'provider',
        name: 'Groq',
        shortName: 'Groq',
        description: 'Fast hosted open models through Groq.',
        status: 'Direct API',
        credentialLabel: 'Groq API key',
        modelPlaceholder: 'llama-3.3-70b-versatile',
        endpointDefault: 'https://api.groq.com/openai/v1',
        endpointPlaceholder: 'https://api.groq.com/openai/v1',
        models: [
          { id: 'llama-3.3-70b-versatile', label: 'Llama 3.3 70B' },
          { id: 'deepseek-r1-distill-llama-70b', label: 'DeepSeek R1 Distill Llama 70B' },
          { id: 'gemma2-9b-it', label: 'Gemma 2 9B' }
        ]
      },
      {
        id: 'xai',
        groupId: 'provider',
        name: 'xAI',
        shortName: 'xAI',
        description: 'Grok models through xAI.',
        status: 'Direct API',
        credentialLabel: 'xAI API key',
        modelPlaceholder: 'grok-4.3',
        endpointDefault: 'https://api.x.ai/v1',
        endpointPlaceholder: 'https://api.x.ai/v1',
        models: [
          { id: 'grok-4.3', label: 'Grok 4.3' },
          { id: 'grok-code-fast-1', label: 'Grok Code Fast 1' }
        ]
      },
      {
        id: 'together',
        groupId: 'provider',
        name: 'Together AI',
        shortName: 'Together',
        description: 'Open and proprietary models hosted by Together AI.',
        status: 'Direct API',
        credentialLabel: 'Together API key',
        modelPlaceholder: 'meta-llama/Llama-3.3-70B-Instruct-Turbo',
        endpointDefault: 'https://api.together.xyz/v1',
        endpointPlaceholder: 'https://api.together.xyz/v1',
        models: [
          { id: 'meta-llama/Llama-3.3-70B-Instruct-Turbo', label: 'Llama 3.3 70B Turbo' },
          { id: 'Qwen/Qwen3-235B-A22B-fp8-tput', label: 'Qwen3 235B' },
          { id: 'deepseek-ai/DeepSeek-R1', label: 'DeepSeek R1' }
        ]
      },
      {
        id: 'fireworks',
        groupId: 'provider',
        name: 'Fireworks AI',
        shortName: 'Fireworks',
        description: 'Hosted open models from Fireworks AI.',
        status: 'Direct API',
        credentialLabel: 'Fireworks API key',
        modelPlaceholder: 'accounts/fireworks/models/llama-v3p3-70b-instruct',
        endpointDefault: 'https://api.fireworks.ai/inference/v1',
        endpointPlaceholder: 'https://api.fireworks.ai/inference/v1',
        models: [
          { id: 'accounts/fireworks/models/llama-v3p3-70b-instruct', label: 'Llama 3.3 70B' },
          { id: 'accounts/fireworks/models/deepseek-r1', label: 'DeepSeek R1' },
          { id: 'accounts/fireworks/models/qwen3-235b-a22b', label: 'Qwen3 235B' }
        ]
      },
      {
        id: 'cerebras',
        groupId: 'provider',
        name: 'Cerebras',
        shortName: 'Cerebras',
        description: 'Fast hosted open models from Cerebras.',
        status: 'Direct API',
        credentialLabel: 'Cerebras API key',
        modelPlaceholder: 'llama3.3-70b',
        endpointDefault: 'https://api.cerebras.ai/v1',
        endpointPlaceholder: 'https://api.cerebras.ai/v1',
        models: [
          { id: 'llama3.3-70b', label: 'Llama 3.3 70B' },
          { id: 'qwen-3-32b', label: 'Qwen3 32B' }
        ]
      },
      {
        id: 'openai_compatible',
        groupId: 'provider',
        name: 'Other compatible API',
        shortName: 'Custom API',
        description: 'Any hosted provider that accepts OpenAI chat-completions requests.',
        status: 'Base URL required',
        credentialLabel: 'Provider API key',
        modelPlaceholder: 'provider/model-name',
        endpointPlaceholder: 'https://api.provider.example/v1',
        models: openCatalogModels
      }
    ]
  },
  {
    id: 'agent',
    title: 'Coding Agents',
    description: 'Use an installed coding agent with its active local login.',
    providers: [
      {
        id: 'codex_sdk',
        groupId: 'agent',
        name: 'Codex',
        shortName: 'Codex',
        description: 'Use the installed Codex runtime as Soma brain.',
        status: 'Luna · adaptive reasoning',
        modelPlaceholder: 'gpt-5.6-luna',
        models: [
          { id: 'gpt-5.6-luna', label: 'GPT-5.6 Luna', note: 'Fast default' },
          { id: 'gpt-5.6-terra', label: 'GPT-5.6 Terra', note: 'Balanced' },
          { id: 'gpt-5.6-sol', label: 'GPT-5.6 Sol', note: 'Deepest' }
        ]
      },
      {
        id: 'claude_code',
        groupId: 'agent',
        name: 'Claude Code',
        shortName: 'Claude Code',
        description: 'Use the installed Claude Code CLI with its active local login.',
        status: 'Local CLI login',
        modelPlaceholder: 'Claude Code model alias (optional)',
        models: []
      }
    ]
  }
];

export function allAiProviders(): AiProviderOption[] {
  return aiProviderGroups.flatMap((group) => group.providers);
}

export function providerById(providerId: AiProviderId): AiProviderOption {
  return allAiProviders().find((provider) => provider.id === providerId)
    ?? allAiProviders().find((provider) => provider.id === 'codex_sdk')
    ?? aiProviderGroups[0].providers[0];
}

export function defaultModelForProvider(provider: AiProviderOption): string {
  return provider.models[0]?.id ?? '';
}
