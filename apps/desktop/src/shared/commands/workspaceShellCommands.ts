import type { WorkspaceState } from '../../../../../packages/contracts/src';

import { invokeUnchecked } from './tauriCommandClient';

export async function getCurrentWorkspace(): Promise<WorkspaceState> {
  return parseWorkspaceShell(await invokeUnchecked('get_current_workspace'));
}

function parseWorkspaceShell(value: unknown): WorkspaceState {
  if (!value || typeof value !== 'object') {
    throw new Error('get_current_workspace result failed shell validation: expected object.');
  }
  const state = value as Record<string, unknown>;
  if (typeof state.has_workspace !== 'boolean') {
    throw new Error('get_current_workspace result failed shell validation: has_workspace must be boolean.');
  }
  return {
    has_workspace: state.has_workspace,
    workspace_dir: nullableString(state.workspace_dir, 'workspace_dir'),
    database_path: nullableString(state.database_path, 'database_path')
  };
}

function nullableString(value: unknown, field: string): string | null {
  if (value === null || value === undefined) return null;
  if (typeof value === 'string') return value;
  throw new Error(`get_current_workspace result failed shell validation: ${field} must be string or null.`);
}
