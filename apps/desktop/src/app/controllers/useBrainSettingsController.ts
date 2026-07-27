import { useCallback, useEffect, useRef, useState } from 'react';

import type { SaveBrainSettingsInput } from '../../../../../packages/contracts/src';
import {
  brainSetupIssue,
  defaultAiSettings,
  mergePersistedAiSettings,
  type AiSettingsDraft
} from '../../features/settings/aiSettingsPolicy';
import { formatError } from './controllerUtils';

type BrainSettingsSecretInput = Pick<SaveBrainSettingsInput, 'apiKey' | 'clearApiKey'>;

let settingsLoad: Promise<AiSettingsDraft> | null = null;
let commandsLoad: Promise<typeof import('../../shared/commands/brainSettingsCommands')> | null = null;

export function useBrainSettingsController() {
  const loadStartedRef = useRef(false);
  const draftRevisionRef = useRef(0);
  const writeRevisionRef = useRef(0);
  const writeInFlightRef = useRef(false);
  const [activeSettings, setActiveSettings] = useState<AiSettingsDraft | null>(null);
  const [draft, setDraft] = useState<AiSettingsDraft>(() => defaultAiSettings());
  const draftRef = useRef(draft);
  const [notice, setNotice] = useState<string | null>(null);

  const load = useCallback(async () => {
    if (loadStartedRef.current) return;
    loadStartedRef.current = true;
    const request = currentRevision(draftRevisionRef, writeRevisionRef);
    try {
      const settings = await loadStoredSettings();
      if (request.write !== writeRevisionRef.current) return;
      const next = mergePersistedAiSettings(settings, draftRef.current);
      setActiveSettings(next);
      if (request.draft === draftRevisionRef.current) {
        draftRef.current = next;
        setDraft(next);
      }
    } catch (error) {
      if (request.write === writeRevisionRef.current) {
        setNotice(formatError(error));
      }
    }
  }, []);

  useEffect(() => {
    void load();
  }, [load]);

  const updateDraft = useCallback((next: AiSettingsDraft) => {
    const edited = { ...next, updatedAt: null };
    draftRevisionRef.current += 1;
    draftRef.current = edited;
    setDraft(edited);
    setNotice(null);
  }, []);

  const reportNotice = useCallback((message: string | null) => {
    setNotice(message);
  }, []);

  const save = useCallback((secretInput?: BrainSettingsSecretInput) => {
    if (writeInFlightRef.current) return Promise.resolve(false);
    writeInFlightRef.current = true;
    const requestedDraft = draftRef.current;
    const request = beginWrite(draftRevisionRef, writeRevisionRef);

    return persistSettings(requestedDraft, secretInput)
      .then((saved) => {
        if (request.write !== writeRevisionRef.current) return true;
        const next = mergePersistedAiSettings(saved, draftRef.current);
        setActiveSettings(next);
        if (request.draft === draftRevisionRef.current) {
          draftRef.current = next;
          setDraft(next);
          setNotice('Brain settings saved.');
        } else {
          setNotice('Brain settings saved. Newer edits are not saved yet.');
        }
        return true;
      })
      .catch((error) => {
        if (request.write === writeRevisionRef.current) {
          setNotice(formatError(error));
        }
        return false;
      })
      .finally(() => {
        writeInFlightRef.current = false;
      });
  }, []);

  const authorizeCodex = useCallback(async () => {
    try {
      const { authorizeCodexBrain } = await brainSettingsCommands();
      const result = await authorizeCodexBrain();
      setNotice(formatError(result.message));
    } catch (error) {
      setNotice(formatError(error));
    }
  }, []);

  const enableCodex = useCallback(async () => {
    if (writeInFlightRef.current) return;
    writeInFlightRef.current = true;
    const requestedDraft = draftRef.current;
    const request = beginWrite(draftRevisionRef, writeRevisionRef);
    try {
      const { enableCodexBrain } = await brainSettingsCommands();
      const result = await enableCodexBrain({
        providerId: 'codex_sdk',
        model: requestedDraft.model,
        endpoint: '',
        authProfile: requestedDraft.authProfile
      });
      if (request.write !== writeRevisionRef.current) return;
      if (result.settings) {
        const next = mergePersistedAiSettings(result.settings, draftRef.current);
        setActiveSettings(next);
        if (request.draft === draftRevisionRef.current) {
          draftRef.current = next;
          setDraft(next);
        }
      }
      setNotice(
        result.status === 'ready'
          ? `Codex enabled. ${result.version ? `Detected ${result.version}.` : formatError(result.message)}`
          : `Codex is not ready. ${formatError(result.message)}`
      );
    } catch (error) {
      if (request.write === writeRevisionRef.current) {
        setNotice(formatError(error));
      }
    } finally {
      writeInFlightRef.current = false;
    }
  }, []);

  return {
    draft,
    notice,
    setupMessage: activeSettings ? brainSetupIssue(activeSettings)?.message ?? null : null,
    updateDraft,
    reportNotice,
    save,
    authorizeCodex,
    enableCodex
  };
}

async function persistSettings(draft: AiSettingsDraft, secretInput?: BrainSettingsSecretInput) {
  const { saveBrainSettings } = await brainSettingsCommands();
  return saveBrainSettings({
    providerId: draft.providerId,
    model: draft.model,
    endpoint: draft.endpoint,
    authProfile: draft.authProfile,
    apiKey: secretInput?.apiKey,
    clearApiKey: secretInput?.clearApiKey
  });
}

function currentRevision(
  draftRevision: { current: number },
  writeRevision: { current: number }
) {
  return {
    draft: draftRevision.current,
    write: writeRevision.current
  };
}

function beginWrite(
  draftRevision: { current: number },
  writeRevision: { current: number }
) {
  writeRevision.current += 1;
  return currentRevision(draftRevision, writeRevision);
}

function loadStoredSettings(): Promise<AiSettingsDraft> {
  settingsLoad ??= brainSettingsCommands()
    .then(({ getBrainSettings }) => getBrainSettings())
    .then((settings) => mergePersistedAiSettings(settings, defaultAiSettings()))
    .finally(() => {
      settingsLoad = null;
    });
  return settingsLoad;
}

function brainSettingsCommands(): Promise<typeof import('../../shared/commands/brainSettingsCommands')> {
  commandsLoad ??= import('../../shared/commands/brainSettingsCommands');
  return commandsLoad;
}
