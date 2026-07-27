import type {
  BrainModelListResult,
  BrainRuntimeStatus,
  BrainSettings,
  ListBrainModelsInput,
  SaveBrainSettingsInput
} from '../../../../../packages/contracts/src';

import { contractSchema, invokeRequired, withClientTimeout } from './tauriCommandClient';

const MODEL_LIST_CLIENT_TIMEOUT_MS = 30_000;

const brainModelListResultSchema = contractSchema<BrainModelListResult>('brainModelListResultSchema');
const brainRuntimeStatusSchema = contractSchema<BrainRuntimeStatus>('brainRuntimeStatusSchema');
const brainSettingsSchema = contractSchema<BrainSettings>('brainSettingsSchema');
const listBrainModelsArgsSchema = contractSchema<{ settings?: ListBrainModelsInput }>('listBrainModelsArgsSchema');
const saveBrainSettingsArgsSchema = contractSchema<{ settings: SaveBrainSettingsInput }>('saveBrainSettingsArgsSchema');

export async function getBrainSettings(): Promise<BrainSettings> {
  return invokeRequired('get_brain_settings', brainSettingsSchema);
}

export async function saveBrainSettings(settings: SaveBrainSettingsInput): Promise<BrainSettings> {
  return invokeRequired('save_brain_settings', brainSettingsSchema, saveBrainSettingsArgsSchema, { settings });
}

export async function enableCodexBrain(settings?: SaveBrainSettingsInput): Promise<BrainRuntimeStatus> {
  const args = settings ? { settings } : undefined;
  return invokeRequired(
    'enable_codex_brain',
    brainRuntimeStatusSchema,
    args ? saveBrainSettingsArgsSchema : undefined,
    args
  );
}

export async function authorizeCodexBrain(): Promise<BrainRuntimeStatus> {
  return invokeRequired('authorize_codex_brain', brainRuntimeStatusSchema);
}

export async function listBrainModels(settings?: ListBrainModelsInput): Promise<BrainModelListResult> {
  const args = settings ? { settings } : undefined;
  return withClientTimeout(
    invokeRequired(
      'list_brain_models',
      brainModelListResultSchema,
      args ? listBrainModelsArgsSchema : undefined,
      args
    ),
    MODEL_LIST_CLIENT_TIMEOUT_MS,
    'Model list refresh timed out.'
  );
}
