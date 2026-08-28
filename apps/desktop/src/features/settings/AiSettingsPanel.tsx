import { useEffect, useRef, useState, type ChangeEvent } from 'react';
import type { BrainReasoningEffort, SaveBrainSettingsInput } from '../../../../../packages/contracts/src';
import { listBrainModels } from '../../shared/commands/brainSettingsCommands';
import { formatError } from '../../shared/errorMessage';
import {
  aiProviderGroups,
  defaultModelForProvider,
  providerById,
  type AiModelOption,
  type AiProviderGroupId,
  type AiProviderId,
  type AiProviderOption
} from './aiProviderCatalog';
import {
  activeCredentialCue,
  aiSettingsSummary,
  effectiveBrainModel,
  filterModelOptions,
  modelOptionsForSettings
} from './aiSettingsViewModel';
import {
  credentialConfiguredForProvider,
  type AiSettingsDraft
} from './aiSettingsPolicy';
import './ai-settings-panel.css';

const MODEL_PAGE_SIZE = 80;
const STORED_CREDENTIAL_MASK = '************';
const REASONING_EFFORT_OPTIONS: Array<{ value: BrainReasoningEffort; label: string }> = [
  { value: 'none', label: 'None - fastest' },
  { value: 'low', label: 'Low - quick' },
  { value: 'medium', label: 'Medium - balanced' },
  { value: 'high', label: 'High - deeper' },
  { value: 'xhigh', label: 'XHigh - thorough' },
  { value: 'max', label: 'Max - slowest' }
];

type AiSettingsPanelProps = {
  value: AiSettingsDraft;
  notice: string | null;
  onChange: (value: AiSettingsDraft) => void;
  onNotice: (message: string | null) => void;
  onSave: (secretInput?: Pick<SaveBrainSettingsInput, 'apiKey' | 'clearApiKey'>) => Promise<boolean>;
  onAuthorizeCodex: () => void | Promise<void>;
  onEnableCodex: () => void | Promise<void>;
};

export function AiSettingsPanel({
  value,
  notice,
  onChange,
  onNotice,
  onSave,
  onAuthorizeCodex,
  onEnableCodex
}: AiSettingsPanelProps) {
  const [credentialDraft, setCredentialDraft] = useState('');
  const [clearCredential, setClearCredential] = useState(false);
  const [credentialVisible, setCredentialVisible] = useState(false);
  const credentialRevisionRef = useRef(0);
  const saveInFlightRef = useRef(false);
  const [saveBusy, setSaveBusy] = useState(false);
  const [modelOptions, setModelOptions] = useState<string[]>([]);
  const [modelListBusy, setModelListBusy] = useState(false);
  const [modelQuery, setModelQuery] = useState('');
  const [providerQuery, setProviderQuery] = useState('');
  const [modelLimit, setModelLimit] = useState(MODEL_PAGE_SIZE);
  const provider = providerById(value.providerId);
  const previousProviderIdRef = useRef(value.providerId);
  const [activeGroupId, setActiveGroupId] = useState<AiProviderGroupId>(provider.groupId);
  const activeGroup = aiProviderGroups.find((group) => group.id === activeGroupId) ?? aiProviderGroups[0];
  const hasEndpoint = provider.groupId === 'local' || provider.groupId === 'provider';
  const hasCredential = provider.groupId === 'provider';
  const hasAuthProfile = provider.id === 'codex_sdk';
  const isAgent = provider.groupId === 'agent';
  const dataUseNotice = hasCredential
    ? `Questions and bounded graph/paper context are sent to ${provider.name}. Secrets stay in desktop app data.`
    : isAgent
      ? `Questions and bounded graph/paper context are passed to the installed ${provider.name} runtime.`
      : 'Questions and bounded graph/paper context are sent to the configured endpoint.';
  const canListModels = provider.groupId === 'provider' || provider.groupId === 'local';
  const modelChoices = modelOptionsForSettings(value, modelOptions);
  const matchingModelChoices = filterModelOptions(modelChoices, modelQuery, modelChoices.length);
  const visibleModelChoices = matchingModelChoices.slice(0, modelLimit);
  const selectedModel = modelChoices.find((model) => model.id === value.model);
  const providerChoices = filterProviderChoices(activeGroup.providers, providerQuery);
  const credentialLabel = provider.credentialLabel ?? 'API key';
  const credentialPlaceholderText = value.credentialConfigured && !credentialDraft
    ? STORED_CREDENTIAL_MASK
    : 'sk-...';
  const credentialHint = credentialDraft
    ? 'Unsaved replacement key.'
    : value.credentialConfigured
      ? 'Stored key is masked.'
      : null;
  const credentialCue = activeCredentialCue(value, credentialLabel);
  const chatReasoningEffort = value.defaultReasoningEffort ?? 'medium';
  const graphReasoningEffort = value.graphReasoningEffort ?? 'xhigh';
  const policyState = value.updatedAt ? 'Active' : 'Draft';

  useEffect(() => {
    setModelOptions([]);
  }, [value.endpoint, value.providerId]);

  useEffect(() => {
    if (previousProviderIdRef.current === value.providerId) return;
    previousProviderIdRef.current = value.providerId;
    setActiveGroupId(provider.groupId);
  }, [provider.groupId, value.providerId]);

  return (
    <section className="settingsPanel" aria-label="Settings detail">
      <div className="settingsSectionRail settingsSectionRail--families" aria-label="Brain sections">
        {aiProviderGroups.map((group) => (
          <button
            className={[
              activeGroupId === group.id ? 'isActive' : '',
              provider.groupId === group.id ? 'hasSelection' : ''
            ].filter(Boolean).join(' ')}
            type="button"
            aria-pressed={activeGroupId === group.id}
            key={group.id}
            onClick={() => selectGroup(group.id)}
          >
            <span>{group.title}</span>
            <small>{group.description}</small>
          </button>
        ))}
      </div>

      <div className="settingsContent">
        <div className="settingsIntro">
          <h3>Brain</h3>
          <p>Choose the runtime, provider, and model Soma should use for chat and graph update drafts.</p>
        </div>

        <div className="aiRuntimeSummary" aria-label="Selected brain">
          <strong>{aiSettingsSummary(value)}</strong>
          <small>{provider.status}</small>
          {provider.id === 'codex_sdk' ? (
            <small className="aiRuntimePolicy">
              {policyState}: {effectiveBrainModel(value)} · chat {chatReasoningEffort} · graph {graphReasoningEffort}
            </small>
          ) : null}
          <small className={`aiRuntimeCredentialCue is-${credentialCue.tone}`}>{credentialCue.label}</small>
        </div>

        <section className="brainRuntimePicker" aria-label={`${activeGroup.title} options`}>
          <div className="settingsSubhead">
            <h4>{activeGroup.title}</h4>
          </div>

          {activeGroup.id === 'provider' ? (
            <label className="aiProviderSearchRow">
              <span>Provider</span>
              <input
                value={providerQuery}
                onChange={(event) => {
                  setProviderQuery(event.target.value);
                }}
                placeholder="Search providers"
                spellCheck={false}
                aria-label="Search providers"
              />
            </label>
          ) : null}

          <div className="aiProviderDrawer" role="radiogroup" aria-label={`${activeGroup.title} providers`}>
            {providerChoices.map((option) => {
              const optionSelected = value.providerId === option.id;
              return (
              <button
                className={`aiProviderOption ${optionSelected ? 'isSelected' : ''}`}
                type="button"
                role="radio"
                aria-checked={optionSelected}
                key={option.id}
                onClick={() => selectProvider(option.id)}
              >
                <span className="aiProviderBody">
                  <span className="aiProviderTopline">
                    <strong>{option.shortName}</strong>
                    <em>{option.status}</em>
                  </span>
                  <span>{option.description}</span>
                  <small>{option.modelPlaceholder}</small>
                </span>
              </button>
              );
            })}
            {providerChoices.length === 0 ? (
              <p className="aiProviderEmpty">No matching providers.</p>
            ) : null}
          </div>
        </section>

        <section className="brainConfiguration" aria-label="Brain configuration">
          <div className="settingsSubhead">
            <h4>Model</h4>
            <p>{configurationCopy(provider.groupId)}</p>
          </div>

          <div className="aiRuntimeDetails">
            {provider.groupId === 'agent' ? (
              <div className="aiRuntimeWide">
                <span>{provider.id === 'codex_sdk' ? 'Codex model' : 'Claude Code model'}</span>
                <input
                  value={value.model}
                  onChange={(event) => updateDraft('model', event)}
                  placeholder={provider.modelPlaceholder}
                  spellCheck={false}
                  aria-label={provider.id === 'codex_sdk' ? 'Codex model ID' : 'Claude Code model ID'}
                />
                {provider.id === 'codex_sdk' ? (
                  <span className="aiAgentModelChoices" role="radiogroup" aria-label="Codex model">
                    {provider.models.map((model) => (
                      <button
                        className={effectiveBrainModel(value) === model.id ? 'isSelected' : ''}
                        type="button"
                        role="radio"
                        aria-checked={effectiveBrainModel(value) === model.id}
                        key={model.id}
                        onClick={() => onChange({ ...value, model: model.id })}
                      >
                        {model.label}
                      </button>
                    ))}
                  </span>
                ) : null}
              </div>
            ) : (
              <label className="aiRuntimeModelField aiRuntimeWide">
                <span>Model</span>
                <div className="aiRuntimeModelSearchRow">
                  <input
                    value={modelQuery}
                    onChange={(event) => {
                      setModelQuery(event.target.value);
                      setModelLimit(MODEL_PAGE_SIZE);
                    }}
                    placeholder="Search model name, provider, or ID"
                    spellCheck={false}
                    aria-label="Search models"
                  />
                  {canListModels ? (
                    <button
                      type="button"
                      disabled={modelListBusy}
                      onClick={() => { void handleListModels(); }}
                    >
                      {modelListBusy ? 'Refreshing' : 'Refresh'}
                    </button>
                  ) : null}
                </div>
                <div className="aiRuntimeSelectedModel" aria-live="polite">
                  <span>Selected</span>
                  <strong>{selectedModel?.label ?? (value.model || 'No model selected')}</strong>
                  <small>{selectedModel ? modelDetail(selectedModel) : (value.model || 'Choose a model below')}</small>
                </div>
                <div className="aiModelOptionList" role="listbox" aria-label="Model options">
                  {visibleModelChoices.map((model) => (
                    <button
                      className={`aiModelOption ${value.model === model.id ? 'isSelected' : ''}`}
                      type="button"
                      role="option"
                      aria-selected={value.model === model.id}
                      key={model.id}
                      onClick={() => onChange({ ...value, model: model.id })}
                    >
                      <span>
                        <strong>{model.label}</strong>
                        <small>{modelDetail(model)}</small>
                      </span>
                      <em>{sourceLabel(model)}</em>
                    </button>
                  ))}
                  {visibleModelChoices.length === 0 ? (
                    <p className="aiModelEmpty">No matching models. Use Advanced for a custom model ID.</p>
                  ) : null}
                </div>
                {matchingModelChoices.length > visibleModelChoices.length ? (
                  <div className="aiModelOptionLimit">
                    <span>
                      Showing {visibleModelChoices.length.toLocaleString()} of{' '}
                      {matchingModelChoices.length.toLocaleString()} matches.
                    </span>
                    <button
                      type="button"
                      onClick={() => setModelLimit((limit) => Math.min(
                        limit + MODEL_PAGE_SIZE,
                        matchingModelChoices.length
                      ))}
                    >
                      Show{' '}
                      {Math.min(
                        MODEL_PAGE_SIZE,
                        matchingModelChoices.length - visibleModelChoices.length
                      ).toLocaleString()} more
                    </button>
                  </div>
                ) : null}
              </label>
            )}

            {provider.id === 'codex_sdk' ? (
              <div className="aiReasoningControls aiRuntimeWide" aria-label="Codex reasoning effort">
                <label>
                  <span>Chat effort</span>
                  <select
                    value={chatReasoningEffort}
                    onChange={(event) => onChange({
                      ...value,
                      defaultReasoningEffort: event.target.value as BrainReasoningEffort
                    })}
                  >
                    {REASONING_EFFORT_OPTIONS.map((option) => (
                      <option value={option.value} key={option.value}>{option.label}</option>
                    ))}
                  </select>
                  <small>Normal questions and paper chat.</small>
                </label>
                <label>
                  <span>Graph effort</span>
                  <select
                    value={graphReasoningEffort}
                    onChange={(event) => onChange({
                      ...value,
                      graphReasoningEffort: event.target.value as BrainReasoningEffort
                    })}
                  >
                    {REASONING_EFFORT_OPTIONS.map((option) => (
                      <option value={option.value} key={option.value}>{option.label}</option>
                    ))}
                  </select>
                  <small>Graph capture, connections, node content, and compile.</small>
                </label>
                <p>Higher effort can improve hard decisions, but it takes longer.</p>
              </div>
            ) : null}

            {hasEndpoint ? (
              <label className="aiRuntimeWide">
                <span>{provider.groupId === 'local' ? 'Endpoint' : 'Base URL'}</span>
                <input
                  value={value.endpoint}
                  onChange={(event) => updateDraft('endpoint', event)}
                  placeholder={provider.endpointDefault
                    ?? provider.endpointPlaceholder
                    ?? endpointPlaceholder(provider.groupId)}
                  spellCheck={false}
                />
              </label>
            ) : null}

            {hasCredential ? (
              <div className="aiRuntimeSecretField">
                <span>{credentialLabel}</span>
                <div className="aiRuntimeSecretInput">
                  <input
                    type={credentialVisible ? 'text' : 'password'}
                    value={credentialDraft}
                    onChange={(event) => {
                      credentialRevisionRef.current += 1;
                      setCredentialDraft(event.target.value);
                      setClearCredential(false);
                    }}
                    placeholder={credentialPlaceholderText}
                    aria-label={credentialLabel}
                    autoComplete="off"
                    spellCheck={false}
                  />
                  <button
                    type="button"
                    disabled={!credentialDraft}
                    onClick={() => setCredentialVisible((visible) => !visible)}
                  >
                    {credentialVisible ? 'Hide' : 'Show'}
                  </button>
                </div>
                {credentialHint ? <small>{credentialHint}</small> : null}
              </div>
            ) : null}

            {hasAuthProfile ? (
              <label>
                <span>Codex auth</span>
                <input
                  value={value.authProfile}
                  onChange={(event) => updateDraft('authProfile', event)}
                  placeholder="default"
                  spellCheck={false}
                />
              </label>
            ) : null}

            {hasCredential && value.credentialConfigured ? (
              <label className="aiRuntimeSecretClear aiRuntimeWide">
                <input
                  type="checkbox"
                  checked={clearCredential}
                  onChange={(event) => {
                    credentialRevisionRef.current += 1;
                    setClearCredential(event.target.checked);
                  }}
                />
                <span>Clear stored key on save</span>
              </label>
            ) : null}

            {provider.id === 'codex_sdk' ? (
              <div className="aiRuntimeAction aiRuntimeWide">
                <button type="button" onClick={() => { void onAuthorizeCodex(); }}>Authorize</button>
                <button type="button" onClick={() => { void onEnableCodex(); }}>Enable Codex</button>
                <small>Uses your local Codex login. Save Brain after changing model or effort.</small>
              </div>
            ) : null}

            {provider.groupId !== 'agent' ? (
              <details className="aiRuntimeAdvanced">
                <summary>Advanced</summary>
                <label>
                  <span>Custom model ID</span>
                  <input
                    value={value.model}
                    onChange={(event) => updateDraft('model', event)}
                    placeholder={provider.modelPlaceholder}
                    spellCheck={false}
                  />
                </label>
              </details>
            ) : null}
          </div>
        </section>

        <div className="aiSettingsFooter">
          <p>{dataUseNotice}</p>
          <button
            type="button"
            disabled={saveBusy}
            onClick={() => { void handleSave(); }}
          >
            {saveBusy ? 'Saving' : 'Save Brain'}
          </button>
        </div>
        {notice ? <p className="settingsNotice">{notice}</p> : null}
      </div>
    </section>
  );

  function updateDraft(field: 'model' | 'endpoint' | 'authProfile', event: ChangeEvent<HTMLInputElement>) {
    onChange({ ...value, [field]: event.target.value });
  }

  async function handleSave() {
    if (saveInFlightRef.current) return;
    saveInFlightRef.current = true;
    setSaveBusy(true);
    const credentialRevision = credentialRevisionRef.current;
    try {
      const saved = await onSave({
        apiKey: credentialDraft.trim() ? credentialDraft.trim() : undefined,
        clearApiKey: clearCredential
      });
      if (!saved || credentialRevision !== credentialRevisionRef.current) return;
      resetCredentialEditor();
    } finally {
      saveInFlightRef.current = false;
      setSaveBusy(false);
    }
  }

  async function handleListModels() {
    setModelListBusy(true);
    try {
      const result = await listBrainModels({
        providerId: value.providerId,
        model: value.model,
        endpoint: value.endpoint,
        authProfile: value.authProfile,
        defaultReasoningEffort: chatReasoningEffort,
        graphReasoningEffort,
        apiKey: credentialDraft.trim() ? credentialDraft.trim() : undefined,
        clearApiKey: false
      });
      setModelOptions(result.models);
      onNotice(formatError(result.message));
    } catch (error) {
      onNotice(formatError(error));
    } finally {
      setModelListBusy(false);
    }
  }

  function selectProvider(providerId: AiProviderId) {
    const nextProvider = providerById(providerId);
    setActiveGroupId(nextProvider.groupId);
    setProviderQuery('');
    if (providerId === value.providerId) return;
    resetCredentialEditor();
    setModelQuery('');
    setModelLimit(MODEL_PAGE_SIZE);
    onChange({
      ...value,
      providerId,
      model: defaultModelForProvider(nextProvider),
      endpoint: '',
      authProfile: '',
      credentialConfigured: credentialConfiguredForProvider(value, providerId)
    });
  }

  function selectGroup(groupId: AiProviderGroupId) {
    setActiveGroupId(groupId);
    setModelQuery('');
    setProviderQuery('');
    setModelLimit(MODEL_PAGE_SIZE);
    const nextGroup = aiProviderGroups.find((group) => group.id === groupId);
    if (!nextGroup || nextGroup.providers.some((option) => option.id === value.providerId)) return;
    selectProvider(nextGroup.providers[0].id);
  }

  function resetCredentialEditor() {
    credentialRevisionRef.current += 1;
    setCredentialDraft('');
    setClearCredential(false);
    setCredentialVisible(false);
  }
}

function sourceLabel(model: AiModelOption) {
  if (model.source === 'live') return 'Live';
  if (model.source === 'saved') return 'Saved';
  return 'Default';
}

function modelDetail(model: AiModelOption) {
  return model.note ? `${model.id} - ${model.note}` : model.id;
}

function configurationCopy(groupId: AiProviderGroupId) {
  if (groupId === 'local') return 'Choose a known local model or refresh the endpoint for its live models.';
  if (groupId === 'provider') {
    return 'Choose a registered model or refresh the provider; Advanced accepts a custom model ID.';
  }
  return 'Use the installed coding agent with its active local login and optional model alias.';
}

function endpointPlaceholder(groupId: AiProviderGroupId) {
  if (groupId === 'provider') return 'provider default';
  return 'http://localhost:11434/v1';
}

function filterProviderChoices(providers: AiProviderOption[], query: string) {
  const terms = searchTerms(query);
  if (terms.length === 0) return providers;
  return providers.filter((provider) => matchesTerms([
    provider.name,
    provider.shortName,
    provider.description,
    provider.id
  ], terms));
}

function searchTerms(query: string) {
  return query.trim().toLowerCase().split(/\s+/).filter(Boolean);
}

function matchesTerms(values: string[], terms: string[]) {
  const haystack = values.join(' ').toLowerCase();
  return terms.every((term) => haystack.includes(term));
}
