import { expect, test, type Page } from '@playwright/test';

test('rapid workspace actions invoke one backend mutation', async ({ page }) => {
  await installDelayedWorkspaceMock(page);
  await page.goto('/');

  await page.getByRole('button', { name: 'Sources' }).click();
  const createButton = page.getByRole('button', { name: 'New graph' });
  await createButton.evaluate((button) => {
    button.click();
    button.click();
  });

  await expect.poll(() => createCount(page)).toBe(1);
  await expect(createButton).toBeDisabled();

  await page.evaluate(() => {
    (globalThis as typeof globalThis & WorkspaceActionTestState).__resolveWorkspaceCreates?.();
  });
  await expect(page.getByText('Local workspace')).toBeVisible();
  await expect(createButton).toBeEnabled();
});

async function installDelayedWorkspaceMock(page: Page) {
  await page.addInitScript(() => {
    const state = globalThis as typeof globalThis & WorkspaceActionTestState;
    const emptyWorkspace = {
      has_workspace: false,
      workspace_dir: null,
      database_path: null
    };
    const workspace = {
      has_workspace: true,
      workspace_dir: 'C:\\Soma\\workspace-action-test',
      database_path: 'C:\\Soma\\workspace-action-test\\soma.db',
      stats: { sources: 0, conversations: 0, messages: 0, chunks: 0, ftsRows: 0 }
    };
    const emptyReviewQueue = {
      generated_at: '2026-08-31T00:00:00.000Z',
      total_count: 0,
      counts_by_status: {},
      groups: Object.fromEntries(
        ['draft', 'proposed', 'deferred', 'superseded', 'rejected'].map((status) => [
          status,
          { status, title: status, count: 0, items: [] }
        ])
      ),
      items: [],
      latest_undoable_patch: null
    };
    const createResolvers: Array<(value: typeof workspace) => void> = [];

    state.isTauri = true;
    state.__workspaceCreateCount = 0;
    state.__resolveWorkspaceCreates = () => {
      createResolvers.splice(0).forEach((resolve) => resolve(workspace));
    };
    state.__TAURI_INTERNALS__ = {
      invoke: async (command) => {
        if (command === 'get_current_workspace') return emptyWorkspace;
        if (command === 'get_brain_settings') {
          return {
            providerId: 'codex_sdk',
            model: 'gpt-5.6-luna',
            endpoint: '',
            authProfile: '',
            credentialConfigured: true,
            updatedAt: '2026-08-31T00:00:00.000Z'
          };
        }
        if (command === 'create_workspace_auto') {
          state.__workspaceCreateCount = (state.__workspaceCreateCount ?? 0) + 1;
          return new Promise<typeof workspace>((resolve) => createResolvers.push(resolve));
        }
        if (command === 'load_workspace_bootstrap') {
          return {
            canvas: { schema_version: 1, nodes: [], edges: [], paths: [] },
            layout: { layoutOverrides: {}, pinnedNodeIds: [] }
          };
        }
        if (command === 'load_review_queue') return emptyReviewQueue;
        throw new Error(`Unexpected Tauri command in workspace action test: ${command}`);
      }
    };
  });
}

async function createCount(page: Page) {
  return page.evaluate(() => (
    (globalThis as typeof globalThis & WorkspaceActionTestState).__workspaceCreateCount ?? 0
  ));
}

type WorkspaceActionTestState = {
  isTauri: boolean;
  __workspaceCreateCount?: number;
  __resolveWorkspaceCreates?: () => void;
  __TAURI_INTERNALS__: {
    invoke: (command: string) => Promise<unknown>;
  };
};
