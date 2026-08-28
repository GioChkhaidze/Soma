import {
  BRAIN_PROVIDER_IDS,
  BRAIN_REASONING_EFFORTS,
  type BrainReasoningEffort,
  type BrainSettings
} from '../../../../../packages/contracts/src/appCommands.ts';
import {
  allAiProviders,
  type AiProviderGroupId,
  type AiProviderId
} from './aiProviderCatalog.ts';

export type AiProviderCredentialState = Partial<Record<AiProviderId, boolean>>;

export type AiSettingsDraft = BrainSettings & {
  credentialConfiguredByProvider?: AiProviderCredentialState;
};

export type BrainSetupIssue = {
  message: string;
};

type ProviderPolicy = {
  id: AiProviderId;
  groupId: AiProviderGroupId | 'unsupported';
  endpointDefault?: string;
};

const defaultProviderId: AiProviderId = 'codex_sdk';

export function defaultAiSettings(): AiSettingsDraft {
  return {
    providerId: 'codex_sdk',
    model: '',
    endpoint: '',
    authProfile: '',
    credentialConfigured: false,
    updatedAt: null,
    effectiveModel: 'gpt-5.6-luna',
    modelSource: 'soma_default',
    defaultReasoningEffort: 'medium',
    graphReasoningEffort: 'xhigh'
  };
}

export function sanitizeAiSettings(value: unknown): AiSettingsDraft {
  const input = value && typeof value === 'object' ? value as Partial<AiSettingsDraft> : {};
  const provider = supportedProviderPolicyById(input.providerId);
  const providerChanged = provider.id !== input.providerId;
  return rememberProviderCredential({
    providerId: provider.id,
    model: !providerChanged && typeof input.model === 'string' ? input.model : '',
    endpoint: !providerChanged && typeof input.endpoint === 'string' ? input.endpoint : '',
    authProfile: !providerChanged && provider.id === 'codex_sdk' && typeof input.authProfile === 'string'
      ? input.authProfile
      : '',
    credentialConfigured: !providerChanged && input.credentialConfigured === true,
    updatedAt: typeof input.updatedAt === 'string' ? input.updatedAt : null,
    effectiveModel: !providerChanged && typeof input.effectiveModel === 'string'
      ? input.effectiveModel
      : provider.id === 'codex_sdk' ? 'gpt-5.6-luna' : undefined,
    modelSource: !providerChanged && (input.modelSource === 'selected' || input.modelSource === 'soma_default')
      ? input.modelSource
      : provider.id === 'codex_sdk' ? 'soma_default' : undefined,
    defaultReasoningEffort: !providerChanged
      ? reasoningEffort(input.defaultReasoningEffort, 'medium')
      : 'medium',
    graphReasoningEffort: !providerChanged
      ? reasoningEffort(input.graphReasoningEffort, 'xhigh')
      : 'xhigh',
    credentialConfiguredByProvider: sanitizeProviderCredentialState(input.credentialConfiguredByProvider)
  });
}

export function mergePersistedAiSettings(value: unknown, currentDraft: AiSettingsDraft): AiSettingsDraft {
  const persisted = value && typeof value === 'object' ? value : {};
  return sanitizeAiSettings({
    ...persisted,
    credentialConfiguredByProvider: currentDraft.credentialConfiguredByProvider
  });
}

export function rememberProviderCredential(settings: AiSettingsDraft): AiSettingsDraft {
  const current = settings.credentialConfiguredByProvider ?? {};
  if (current[settings.providerId] === settings.credentialConfigured) return settings;
  return {
    ...settings,
    credentialConfiguredByProvider: {
      ...current,
      [settings.providerId]: settings.credentialConfigured
    }
  };
}

export function credentialConfiguredForProvider(settings: AiSettingsDraft, providerId: AiProviderId): boolean {
  if (settings.providerId === providerId) return settings.credentialConfigured;
  return settings.credentialConfiguredByProvider?.[providerId] === true;
}

export function brainSetupIssue(settings: AiSettingsDraft): BrainSetupIssue | null {
  const provider = providerPolicyById(settings.providerId);
  const endpoint = settings.endpoint.trim() || provider.endpointDefault || '';

  if (provider.groupId === 'unsupported') {
    return {
      message: 'This brain is not connected yet. '
        + 'Choose Codex, Claude Code, a local server, or a provider in Brain Settings.'
    };
  }

  if ((provider.groupId === 'local' || provider.groupId === 'provider') && !settings.model.trim()) {
    return {
      message: 'Choose a model in Brain Settings before compiling.'
    };
  }

  if (provider.groupId === 'local' && !endpoint) {
    return {
      message: 'Local brain needs an endpoint before Soma can compile.'
    };
  }

  if (provider.groupId === 'provider' && !endpoint) {
    return {
      message: 'Provider brain needs a base URL before Soma can compile.'
    };
  }

  if (provider.groupId === 'provider' && !settings.credentialConfigured) {
    return {
      message: 'Provider brain needs a stored key before Soma can compile. Open Brain Settings and save the API key.'
    };
  }

  if (!settings.updatedAt) {
    if (settings.providerId === 'codex_sdk') {
      return {
        message: 'Set up Brain first. Open Brain Settings, authorize Codex if needed, then click Enable Codex.'
      };
    }
    if (settings.providerId === 'claude_code') {
      return {
        message: 'Set up Brain first. Open Brain Settings and save Claude Code.'
      };
    }
    return {
      message: 'Save Brain Settings before compiling.'
    };
  }

  return null;
}

function providerPolicyById(providerId: unknown): ProviderPolicy {
  if (providerId === 'soma_cloud') {
    return { id: 'soma_cloud', groupId: 'unsupported' };
  }
  const provider = typeof providerId === 'string'
    ? allAiProviders().find((option) => option.id === providerId)
    : null;
  return provider ?? allAiProviders().find((option) => option.id === defaultProviderId)
    ?? { id: defaultProviderId, groupId: 'agent' };
}

function supportedProviderPolicyById(providerId: unknown): ProviderPolicy {
  const provider = providerPolicyById(providerId);
  return provider.groupId === 'unsupported'
    ? providerPolicyById(defaultProviderId)
    : provider;
}

function reasoningEffort(value: unknown, fallback: BrainReasoningEffort): BrainReasoningEffort {
  return typeof value === 'string' && BRAIN_REASONING_EFFORTS.includes(value as BrainReasoningEffort)
    ? value as BrainReasoningEffort
    : fallback;
}

function sanitizeProviderCredentialState(value: unknown): AiProviderCredentialState {
  if (!value || typeof value !== 'object') return {};
  const state: AiProviderCredentialState = {};
  const input = value as Record<string, unknown>;
  for (const providerId of BRAIN_PROVIDER_IDS) {
    if (input[providerId] === true) {
      state[providerId] = true;
    }
  }
  return state;
}
