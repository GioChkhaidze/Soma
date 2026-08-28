import { providerById, type AiModelOption } from './aiProviderCatalog.ts';
import type { AiSettingsDraft } from './aiSettingsPolicy.ts';

type AiCredentialCue = {
  label: string;
  tone: 'ready' | 'missing' | 'neutral';
};

export function modelOptionsForSettings(
  settings: AiSettingsDraft,
  liveModelIds: string[] = []
): AiModelOption[] {
  const provider = providerById(settings.providerId);
  const models = new Map<string, AiModelOption>();
  const result: AiModelOption[] = [];
  const addModel = (model: AiModelOption) => {
    if (models.has(model.id)) return;
    models.set(model.id, model);
    result.push(model);
  };

  const liveModels = liveModelIds
    .map((id) => id.trim())
    .filter(Boolean)
    .sort((a, b) => modelLabel(a).localeCompare(modelLabel(b)));
  for (const id of liveModels) {
    addModel({
      id,
      label: modelLabel(id),
      note: 'Live',
      source: 'live'
    });
  }
  const selected = settings.model.trim();
  if (selected && !models.has(selected)) {
    addModel({
      id: selected,
      label: modelLabel(selected),
      note: 'Saved',
      source: 'saved'
    });
  }
  for (const model of provider.models) {
    addModel({ ...model, source: model.source ?? 'catalog' });
  }
  return result;
}

export function filterModelOptions(models: AiModelOption[], query: string, limit = 80): AiModelOption[] {
  const terms = query
    .trim()
    .toLowerCase()
    .split(/\s+/)
    .filter(Boolean);
  const ranked = models
    .map((model, index) => ({
      model,
      index,
      score: modelSearchScore(model, terms)
    }))
    .filter((entry) => entry.score >= 0)
    .sort((a, b) => b.score - a.score || a.index - b.index);
  return ranked.slice(0, limit).map((entry) => entry.model);
}

export function aiSettingsSummary(settings: AiSettingsDraft) {
  const provider = providerById(settings.providerId);
  const model = effectiveBrainModel(settings) || provider.modelPlaceholder;
  return `${model} with ${provider.name}`;
}
export function effectiveBrainModel(settings: AiSettingsDraft) {
  return settings.model.trim() || settings.effectiveModel?.trim() || '';
}

export function activeBrainLabel(settings: AiSettingsDraft | null) {
  if (!settings) return 'Loading Brain';
  const provider = providerById(settings.providerId);
  const model = effectiveBrainModel(settings);
  return model ? `${provider.shortName} · ${model}` : provider.shortName;
}

export function activeBrainEffort(settings: AiSettingsDraft | null, capturesGraph: boolean) {
  if (!settings || settings.providerId !== 'codex_sdk') return null;
  return capturesGraph
    ? settings.graphReasoningEffort ?? 'xhigh'
    : settings.defaultReasoningEffort ?? 'medium';
}

export function activeCredentialCue(settings: AiSettingsDraft, credentialLabel?: string): AiCredentialCue {
  const provider = providerById(settings.providerId);

  if (provider.groupId === 'local') {
    return { label: 'Credential: none required', tone: 'neutral' };
  }

  if (provider.groupId === 'agent') {
    return { label: `Auth: local ${provider.shortName} login`, tone: 'neutral' };
  }

  const label = credentialLabel?.trim()
    || provider.credentialLabel
    || 'API key';

  return settings.credentialConfigured
    ? { label: `Credential: stored ${label}`, tone: 'ready' }
    : { label: `Credential: missing ${label}`, tone: 'missing' };
}

function modelSearchScore(model: AiModelOption, terms: string[]) {
  if (terms.length === 0) return 0;
  const id = model.id.toLowerCase();
  const label = model.label.toLowerCase();
  const note = (model.note ?? '').toLowerCase();
  let score = 0;
  for (const term of terms) {
    if (id === term || label === term) score += 100;
    else if (id.startsWith(term) || label.startsWith(term)) score += 60;
    else if (id.includes(`/${term}`) || id.includes(`-${term}`)) score += 36;
    else if (id.includes(term) || label.includes(term)) score += 24;
    else if (note.includes(term)) score += 8;
    else return -1;
  }
  return score;
}

function modelLabel(modelId: string): string {
  const parts = modelId.split('/');
  return parts[parts.length - 1]
    .replace(/[-_]/g, ' ')
    .replace(/\b\w/g, (letter) => letter.toUpperCase());
}
