import { expect, test, type Page } from '@playwright/test';

test('node body editing serializes saves and rollbacks while publishing each version', async ({ page }) => {
  await installNodeBodyMock(page);
  await page.goto('/');

  await page.getByLabel('Editable node, concept').click();
  const inspector = page.getByLabel('Node detail');
  await inspector.getByRole('button', { name: 'Edit' }).click();
  await inspector.getByLabel('Compiled node body').fill('A safer edited body.');
  await inspector.locator('.nodeBodyEditor').evaluate((form) => {
    (form as HTMLFormElement).requestSubmit();
    (form as HTMLFormElement).requestSubmit();
  });

  await expect.poll(() => nodeBodySaveCount(page)).toBe(1);
  await expect(inspector.getByRole('button', { name: 'Saving' })).toBeDisabled();

  await page.evaluate(() => {
    (globalThis as typeof globalThis & NodeBodyTestState).__resolveNodeBodySaves?.();
  });
  await expect(inspector.getByText('A safer edited body.')).toBeVisible();
  await expect(inspector.getByRole('button', { name: 'Edit' })).toBeEnabled();

  await inspector.getByRole('button', { name: 'Versions' }).click();
  const rollbackButton = inspector.locator('.versionList li')
    .filter({ hasText: 'v1' })
    .getByRole('button', { name: 'Rollback' });
  await rollbackButton.evaluate((button) => {
    button.click();
    button.click();
  });
  await expect.poll(() => nodeBodyRollbackCount(page)).toBe(1);
  await expect(rollbackButton).toBeDisabled();

  await page.evaluate(() => {
    (globalThis as typeof globalThis & NodeBodyTestState).__resolveNodeBodyRollbacks?.();
  });
  await expect(inspector.getByText('The original node body.')).toBeVisible();
});

async function installNodeBodyMock(page: Page) {
  await page.addInitScript(() => {
    const state = globalThis as typeof globalThis & NodeBodyTestState;
    const workspace = {
      has_workspace: true,
      workspace_dir: 'C:\\Soma\\node-body-test',
      database_path: 'C:\\Soma\\node-body-test\\soma.db'
    };
    let compiledBody = 'The original node body.';
    let bodyVersion = 1;
    const saveResolvers: Array<(value: unknown) => void> = [];
    const rollbackResolvers: Array<(value: unknown) => void> = [];
    const canvasNode = () => ({
      id: 'node-editable',
      type: 'concept',
      title: 'Editable node',
      preview: 'A node body mutation fixture.',
      source_chunk_ids: [],
      body_version: bodyVersion,
      body_version_id: `body-${bodyVersion}`,
      status: 'active',
      markers: []
    });
    const nodeDetail = () => ({
      ...canvasNode(),
      compiled_body: compiledBody,
      evidence: [],
      update_history: Array.from({ length: bodyVersion }, (_, index) => ({
        id: `body-${index + 1}`,
        version_number: index + 1,
        authored_by_user: index > 0,
        created_at: `2026-08-31T00:00:0${index}.000Z`,
        is_current: index + 1 === bodyVersion,
        source_chunk_ids: []
      })),
      relations: { items: [], is_partial: false }
    });
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

    state.isTauri = true;
    state.__nodeBodySaveCount = 0;
    state.__nodeBodyRollbackCount = 0;
    state.__resolveNodeBodySaves = () => {
      compiledBody = 'A safer edited body.';
      bodyVersion = 2;
      saveResolvers.splice(0).forEach((resolve) => resolve({
        nodeId: 'node-editable',
        bodyVersion,
        bodyVersionId: 'body-' + bodyVersion
      }));
    };
    state.__resolveNodeBodyRollbacks = () => {
      compiledBody = 'The original node body.';
      bodyVersion = 3;
      rollbackResolvers.splice(0).forEach((resolve) => resolve({
        nodeId: 'node-editable',
        bodyVersion,
        bodyVersionId: 'body-' + bodyVersion
      }));
    };
    state.__TAURI_INTERNALS__ = {
      invoke: async (command) => {
        if (command === 'get_current_workspace') return workspace;
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
        if (command === 'load_workspace_bootstrap') {
          return {
            canvas: { schema_version: 1, nodes: [canvasNode()], edges: [], paths: [] },
            layout: { layoutOverrides: {}, pinnedNodeIds: [] }
          };
        }
        if (command === 'load_graph_canvas_snapshot') {
          return { schema_version: 1, nodes: [canvasNode()], edges: [], paths: [] };
        }
        if (command === 'load_review_queue') return emptyReviewQueue;
        if (command === 'load_graph_node_detail') return nodeDetail();
        if (command === 'list_node_messages') return [];
        if (command === 'update_node_body') {
          state.__nodeBodySaveCount = (state.__nodeBodySaveCount ?? 0) + 1;
          return new Promise((resolve) => saveResolvers.push(resolve));
        }
        if (command === 'rollback_node_body') {
          state.__nodeBodyRollbackCount = (state.__nodeBodyRollbackCount ?? 0) + 1;
          return new Promise((resolve) => rollbackResolvers.push(resolve));
        }
        throw new Error(`Unexpected Tauri command in node body test: ${command}`);
      }
    };
  });
}

async function nodeBodySaveCount(page: Page) {
  return page.evaluate(() => (
    (globalThis as typeof globalThis & NodeBodyTestState).__nodeBodySaveCount ?? 0
  ));
}

async function nodeBodyRollbackCount(page: Page) {
  return page.evaluate(() => (
    (globalThis as typeof globalThis & NodeBodyTestState).__nodeBodyRollbackCount ?? 0
  ));
}

type NodeBodyTestState = {
  isTauri: boolean;
  __nodeBodySaveCount?: number;
  __nodeBodyRollbackCount?: number;
  __resolveNodeBodySaves?: () => void;
  __resolveNodeBodyRollbacks?: () => void;
  __TAURI_INTERNALS__: {
    invoke: (command: string) => Promise<unknown>;
  };
};
