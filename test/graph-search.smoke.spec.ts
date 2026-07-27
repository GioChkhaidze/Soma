import { expect, test, type Page } from '@playwright/test';

test('global search owns async results and opens a node beyond the startup canvas', async ({ page }) => {
  await installGraphSearchMock(page);
  await page.goto('/');

  await page.getByRole('button', { name: 'Search' }).click();
  await expect(page.getByText('Canvas shows 160 of 165 nodes. Search covers all.')).toBeVisible();

  const input = page.getByPlaceholder('Find in graph');
  await input.fill('slow');
  await expect.poll(() => searchQueries(page)).toContain('slow');
  await input.fill('fast');
  await expect(page.getByRole('option', { name: /Fast result/ })).toBeVisible();

  await page.evaluate(() => {
    (globalThis as typeof globalThis & GraphSearchTestState).__resolveSlowSearch?.();
  });
  await expect.poll(() => page.evaluate(() => (
    (globalThis as typeof globalThis & GraphSearchTestState).__slowSearchDelivered === true
  ))).toBe(true);
  await expect(page.getByRole('option', { name: /Fast result/ })).toBeVisible();
  await expect(page.getByRole('option', { name: /Slow result/ })).toHaveCount(0);

  await input.fill('tailneedle');
  const tailResult = page.getByRole('option', { name: /Tail Node 164/ });
  await expect(tailResult).toBeVisible();
  await tailResult.click();

  const inspector = page.getByLabel('Node detail');
  await expect(inspector.getByRole('heading', { name: 'Tail Node 164' })).toBeVisible();
  await expect(inspector.getByRole('button', { name: 'Context' })).toHaveCount(0);
  await expect.poll(() => detailRequests(page)).toContain('node-164');
});

test('an off-canvas graph-chat context link hydrates and opens its node', async ({ page }) => {
  await installGraphSearchMock(page);
  await page.goto('/');

  const graphChat = page.getByLabel('Graph chat');
  await graphChat.getByPlaceholder('Ask Soma').click();
  const usedNode = graphChat.getByRole('button', { name: 'Tail Node 164' });
  await expect(usedNode).toBeVisible();
  await usedNode.click();

  const inspector = page.getByLabel('Node detail');
  await expect(inspector.getByRole('heading', { name: 'Tail Node 164' })).toBeVisible();
  await expect.poll(() => detailRequests(page)).toContain('node-164');
});

async function installGraphSearchMock(page: Page) {
  await page.addInitScript(() => {
    const state = globalThis as typeof globalThis & GraphSearchTestState;
    const workspace = {
      has_workspace: true,
      workspace_dir: 'C:\\Soma\\global-search-test',
      database_path: 'C:\\Soma\\global-search-test\\soma.db'
    };
    const nodeCard = (id: string, title: string, preview: string) => ({
      id,
      type: 'concept',
      title,
      preview,
      source_chunk_ids: [],
      body_version: 1,
      status: 'active',
      markers: []
    });
    const canvasNodes = Array.from({ length: 160 }, (_, index) => (
      nodeCard(
        `node-${String(index).padStart(3, '0')}`,
        `Canvas Node ${String(index).padStart(3, '0')}`,
        'Startup canvas node.'
      )
    ));
    const tailNode = nodeCard('node-164', 'Tail Node 164', 'Contains tailneedle beyond the startup canvas.');
    const slowNode = nodeCard('node-slow', 'Slow result', 'This response must stay stale.');
    const fastNode = nodeCard('node-fast', 'Fast result', 'This is the latest response.');
    const nodeDetail = {
      ...tailNode,
      compiled_body: 'This node is active but omitted from the bounded startup canvas.',
      evidence: [],
      body_sections: [],
      update_history: []
    };
    const emptyReviewQueue = {
      generated_at: '2026-07-25T00:00:00.000Z',
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
    const graphMessage = {
      id: 'assistant-off-canvas',
      role: 'assistant',
      content: 'The relevant concept is outside the startup canvas.',
      created_at: '2026-07-25T00:00:00.000Z',
      context_packet: {
        used_graph_areas: [{ id: tailNode.id, title: tailNode.title, type: tailNode.type }]
      }
    };

    state.isTauri = true;
    state.__searchQueries = [];
    state.__detailRequests = [];
    state.__TAURI_INTERNALS__ = {
      invoke: async (command, args) => {
        if (command === 'get_current_workspace') return workspace;
        if (command === 'get_brain_settings') {
          return {
            providerId: 'codex_sdk',
            model: '',
            endpoint: '',
            authProfile: '',
            credentialConfigured: true,
            updatedAt: '2026-07-25T00:00:00.000Z'
          };
        }
        if (command === 'load_workspace_bootstrap') {
          return {
            canvas: {
              schema_version: 1,
              nodes: canvasNodes,
              edges: [],
              paths: [],
              is_partial: true,
              node_limit: 160,
              edge_limit: 320,
              total_node_count: 165,
              total_edge_count: 0
            },
            layout: {
              layoutOverrides: {},
              pinnedNodeIds: []
            }
          };
        }
        if (command === 'load_review_queue') return emptyReviewQueue;
        if (command === 'search_graph_node_cards') {
          const query = String(args?.query ?? '');
          state.__searchQueries?.push(query);
          if (query === 'slow') {
            return new Promise((resolve) => {
              state.__resolveSlowSearch = () => {
                state.__slowSearchDelivered = true;
                resolve([slowNode]);
              };
            });
          }
          if (query === 'fast') return [fastNode];
          if (query === 'tailneedle') return [tailNode];
          return [];
        }
        if (command === 'load_graph_node_detail') {
          state.__detailRequests?.push(String(args?.node_id ?? ''));
          return nodeDetail;
        }
        if (command === 'list_node_messages') return [];
        if (command === 'list_graph_messages') return [graphMessage];
        throw new Error(`Unexpected Tauri command in graph-search smoke test: ${command}`);
      }
    };
  });
}

async function searchQueries(page: Page) {
  return page.evaluate(() => (
    (globalThis as typeof globalThis & GraphSearchTestState).__searchQueries ?? []
  ));
}

async function detailRequests(page: Page) {
  return page.evaluate(() => (
    (globalThis as typeof globalThis & GraphSearchTestState).__detailRequests ?? []
  ));
}

type GraphSearchTestState = {
  isTauri: boolean;
  __searchQueries?: string[];
  __detailRequests?: string[];
  __resolveSlowSearch?: () => void;
  __slowSearchDelivered?: boolean;
  __TAURI_INTERNALS__: {
    invoke: (command: string, args?: Record<string, unknown>) => Promise<unknown>;
  };
};
