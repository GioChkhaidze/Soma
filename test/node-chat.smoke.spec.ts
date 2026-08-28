import { expect, test, type Page } from '@playwright/test';

test('delayed node history preserves text typed while it loads', async ({ page }) => {
  await installDelayedNodeHistoryMock(page);
  await page.goto('/');

  await page.getByLabel('Draft-safe node, concept').click();
  const composer = page.getByLabel('Message this node');
  await expect(composer).toBeVisible();
  await composer.fill('Keep this draft while history loads.');

  await page.evaluate(() => {
    (globalThis as typeof globalThis & NodeChatTestState).__resolveNodeHistory?.();
  });
  await expect.poll(() => page.evaluate(
    () => (globalThis as typeof globalThis & NodeChatTestState).__nodeHistoryDelivered === true
  )).toBe(true);

  await expect(composer).toHaveValue('Keep this draft while history loads.');
  const nodeChat = page.getByLabel('Node chat');
  await expect(nodeChat.getByText('3 turns')).toBeVisible();
  await expect(nodeChat.getByText('Earlier question one')).toHaveCount(0);
  await nodeChat.getByRole('button', { name: 'History' }).click();
  await expect(nodeChat.getByText('Earlier question one')).toBeVisible();
  await nodeChat.getByRole('button', { name: 'Latest' }).click();
  await expect(nodeChat.getByText('Earlier question one')).toHaveCount(0);
});

test('node chat shares capture state and routes backend-authoritative undo', async ({ page }) => {
  await installNodeCaptureUndoMock(page);
  await page.setViewportSize({ width: 960, height: 640 });
  await page.goto('/');

  await page.getByLabel('Capture-safe node, concept').click();
  const nodeChat = page.getByLabel('Node chat');
  const graphChat = page.getByLabel('Graph chat');
  const composer = nodeChat.getByLabel('Message this node');

  await expect(nodeChat.getByRole('button', { name: 'Graph capture off' })).toBeVisible();
  await nodeChat.getByRole('button', { name: 'Graph capture off' }).click();
  await expect(graphChat.getByRole('button', { name: 'Graph capture on' })).toBeVisible();
  await graphChat.getByRole('button', { name: 'Graph capture on' }).click();
  await expect(nodeChat.getByRole('button', { name: 'Graph capture off' })).toBeVisible();

  await composer.fill('Answer without changing the graph.');
  await composer.press('Enter');
  await expect.poll(() => page.evaluate(() => (
    globalThis as typeof globalThis & NodeChatTestState
  ).__lastNodeChatArgs)).toMatchObject({
    node_id: 'node-capture-safe',
    content: 'Answer without changing the graph.',
    capture_graph_changes: false
  });

  const undo = nodeChat.getByRole('button', { name: 'Undo graph update' });
  await expect(undo).toBeVisible();
  await expect(nodeChat.getByRole('button', { name: 'Graph capture off' })).toBeInViewport();
  await expect(undo).toBeInViewport();
  await expect.poll(() => composer.evaluate((element) => {
    const box = element.getBoundingClientRect();
    return box.top >= 0 && box.bottom <= window.innerHeight;
  })).toBe(true);
  const graphChatBox = await graphChat.boundingBox();
  const nodeChatBox = await nodeChat.boundingBox();
  expect(graphChatBox).not.toBeNull();
  expect(nodeChatBox).not.toBeNull();
  expect(graphChatBox!.x + graphChatBox!.width).toBeLessThanOrEqual(nodeChatBox!.x);
  await undo.click();
  await expect.poll(() => page.evaluate(() => (
    globalThis as typeof globalThis & NodeChatTestState
  ).__lastUndoPatchId)).toBe('node-patch');
  await expect(undo).toHaveCount(0);
});

test('node chat shows its Brain and accepts the next draft while blocking duplicate sends', async ({ page }) => {
  await installNodeCaptureUndoMock(page, { deferNodeChatTurn: true });
  await page.goto('/');

  await page.getByLabel('Capture-safe node, concept').click();
  const nodeChat = page.getByLabel('Node chat');
  const composer = nodeChat.getByLabel('Message this node');

  await composer.fill('Wait for the backend before allowing another turn.');
  await composer.press('Enter');

  await expect(nodeChat.getByRole('button', { name: 'Thinking' })).toBeDisabled();
  await expect(composer).toBeEnabled();
  await expect(nodeChat.getByRole('status')).toContainText('Running Codex · gpt-5.6-luna');
  await expect(nodeChat.getByRole('status')).toContainText('medium');
  await expect(nodeChat.getByRole('button', { name: 'Stop' })).toBeVisible();
  await expect(nodeChat.getByText('Thinking...')).toBeVisible();
  await expect.poll(() => page.evaluate(() => (
    globalThis as typeof globalThis & NodeChatTestState
  ).__nodeChatCallCount)).toBe(1);

  await composer.fill('The next turn is ready.');
  await nodeChat.locator('form').evaluate((form) => {
    (form as HTMLFormElement).requestSubmit();
  });
  await expect.poll(() => page.evaluate(() => (
    globalThis as typeof globalThis & NodeChatTestState
  ).__nodeChatCallCount)).toBe(1);

  await page.evaluate(() => {
    (globalThis as typeof globalThis & NodeChatTestState).__resolveNodeChatTurn?.();
  });

  await expect(nodeChat.getByText('The graph was left unchanged.')).toBeVisible();
  await expect(composer).toBeEnabled();
  await expect(composer).toHaveValue('The next turn is ready.');
  await expect(nodeChat.getByRole('button', { name: 'Send' })).toBeEnabled();
});

test('node chat preserves an overlong draft instead of invoking the backend', async ({ page }) => {
  await installNodeCaptureUndoMock(page);
  await page.goto('/');

  await page.getByLabel('Capture-safe node, concept').click();
  const nodeChat = page.getByLabel('Node chat');
  const composer = nodeChat.getByLabel('Message this node');
  const overlongDraft = '🙂'.repeat(4_001);
  await composer.fill(overlongDraft);
  await composer.press('Enter');

  await expect(composer).toHaveValue(overlongDraft);
  await expect(nodeChat.getByText('Chat messages are limited to 4,000 characters.')).toBeVisible();
  expect(await page.evaluate(() => (
    globalThis as typeof globalThis & NodeChatTestState
  ).__nodeChatCallCount ?? 0)).toBe(0);
});

async function installDelayedNodeHistoryMock(page: Page) {
  await page.addInitScript(() => {
    const state = globalThis as typeof globalThis & NodeChatTestState;
    const workspace = {
      has_workspace: true,
      workspace_dir: 'C:\\Soma\\node-chat-test',
      database_path: 'C:\\Soma\\node-chat-test\\soma.db'
    };
    const canvasNode = {
      id: 'node-draft-safe',
      type: 'concept',
      title: 'Draft-safe node',
      preview: 'A focused node-chat test fixture.',
      source_chunk_ids: [],
      body_version: 1,
      status: 'active',
      markers: []
    };
    const nodeDetail = {
      ...canvasNode,
      compiled_body: 'Node chat history must not replace newer composer state.',
      evidence: [],
      update_history: [],
      relations: { items: [], is_partial: false }
    };
    let resolveHistory: ((messages: unknown[]) => void) | null = null;
    const history = new Promise<unknown[]>((resolve) => {
      resolveHistory = resolve;
    });
    const historyMessages = Array.from({ length: 6 }, (_, index) => ({
      id: `node-history-${index + 1}`,
      node_id: canvasNode.id,
      role: index % 2 === 0 ? 'user' : 'assistant',
      content: index === 0 ? 'Earlier question one' : `History message ${index + 1}`,
      created_at: `2026-07-25T00:00:0${index}.000Z`
    }));

    state.isTauri = true;
    state.__resolveNodeHistory = () => {
      resolveHistory?.(historyMessages);
    };
    state.__TAURI_INTERNALS__ = {
      invoke: async (command) => {
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
              nodes: [canvasNode],
              edges: [],
              paths: []
            },
            layout: {
              layoutOverrides: {},
              pinnedNodeIds: []
            }
          };
        }
        if (command === 'load_review_queue') return emptyReviewQueue();
        if (command === 'load_graph_node_detail') return nodeDetail;
        if (command === 'list_node_messages') {
          const messages = await history;
          state.__nodeHistoryDelivered = true;
          return messages;
        }
        throw new Error(`Unexpected Tauri command in node-chat smoke test: ${command}`);
      }
    };
  });
}

async function installNodeCaptureUndoMock(
  page: Page,
  options: { deferNodeChatTurn?: boolean } = {}
) {
  await page.addInitScript(({ deferNodeChatTurn }) => {
    const state = globalThis as typeof globalThis & NodeChatTestState;
    const workspace = {
      has_workspace: true,
      workspace_dir: 'C:\\Soma\\node-capture-test',
      database_path: 'C:\\Soma\\node-capture-test\\soma.db'
    };
    const canvasNode = {
      id: 'node-capture-safe',
      type: 'concept',
      title: 'Capture-safe node',
      preview: 'Node chat uses the workspace capture policy.',
      source_chunk_ids: [],
      body_version: 1,
      status: 'active',
      markers: []
    };
    const nodeDetail = {
      ...canvasNode,
      compiled_body: 'Node capture and undo share the graph-chat mutation authority.',
      evidence: [],
      update_history: [],
      relations: { items: [], is_partial: false }
    };
    const canvas = {
      schema_version: 1,
      nodes: [canvasNode],
      edges: [],
      paths: []
    };
    const persistedAssistant = {
      id: 'node-assistant',
      node_id: canvasNode.id,
      role: 'assistant',
      content: 'This earlier answer changed the graph.',
      created_at: '2026-07-25T00:00:00.000Z'
    };
    let undoReady = true;
    const reviewQueue = () => {
      const group = (status: string, title: string) => ({ status, title, count: 0, items: [] });
      return {
        generated_at: 'node-capture-test',
        total_count: 0,
        counts_by_status: {},
        groups: {
          draft: group('draft', 'Draft'),
          proposed: group('proposed', 'Needs review'),
          deferred: group('deferred', 'Deferred'),
          superseded: group('superseded', 'Superseded'),
          rejected: group('rejected', 'Rejected')
        },
        items: [],
        latest_undoable_patch: undoReady ? {
          patch_id: 'node-patch',
          source: 'node_thread_message',
          source_message_id: persistedAssistant.id,
          change_count: 1
        } : null
      };
    };

    state.isTauri = true;
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
            canvas,
            layout: {
              layoutOverrides: {},
              pinnedNodeIds: []
            }
          };
        }
        if (command === 'load_graph_canvas_snapshot') return canvas;
        if (command === 'load_review_queue') return reviewQueue();
        if (command === 'load_graph_node_detail') return nodeDetail;
        if (command === 'list_node_messages') return [persistedAssistant];
        if (command === 'list_graph_messages') return [];
        if (command === 'send_node_chat_turn') {
          state.__nodeChatCallCount = (state.__nodeChatCallCount ?? 0) + 1;
          state.__lastNodeChatArgs = args;
          const contextPacket = {
            mode: 'node_chat',
            focused_node_id: canvasNode.id,
            user_message: String(args?.content ?? ''),
            focused_node_body: {
              id: canvasNode.id,
              title: canvasNode.title,
              type: canvasNode.type,
              compiled_body: nodeDetail.compiled_body,
              body_version: 1,
              source_chunk_ids: []
            },
            neighbor_bodies: [],
            bridge_texts: [],
            node_thread_recent_messages: [persistedAssistant],
            source_evidence_excerpts: []
          };
          const createdAt = '2026-07-25T00:01:00.000Z';
          const result = {
            user_message_id: 'node-user-new',
            user_message: {
              id: 'node-user-new',
              node_id: canvasNode.id,
              role: 'user',
              content: String(args?.content ?? ''),
              created_at: createdAt
            },
            assistant_message: {
              id: 'node-assistant-new',
              node_id: canvasNode.id,
              role: 'assistant',
              content: 'The graph was left unchanged.',
              created_at: createdAt
            },
            context_packet: contextPacket,
            used_graph_areas: [],
            proposal_count: 0,
            patch_import_status: 'none',
            patch_import_result: {
              valid: true,
              imported: false,
              trusted: false,
              proposalCount: 0,
              proposals: [],
              errors: [],
              warnings: []
            },
            runtime_status: 'completed',
            runtime_adapter_kind: 'mock',
            runtime_failure_kind: null,
            runtime_message: 'Completed.'
          };
          if (!deferNodeChatTurn) return result;
          return new Promise((resolve) => {
            state.__resolveNodeChatTurn = () => resolve(result);
          });
        }
        if (command === 'undo_graph_patch') {
          state.__lastUndoPatchId = String(args?.patch_id ?? '');
          undoReady = false;
          return {
            patchId: state.__lastUndoPatchId,
            undoneCount: 1,
            status: 'undone'
          };
        }
        throw new Error(`Unexpected Tauri command in node capture smoke test: ${command}`);
      }
    };
  }, { deferNodeChatTurn: options.deferNodeChatTurn ?? false });
}

function emptyReviewQueue() {
  const group = (status: string, title: string) => ({ status, title, count: 0, items: [] });
  return {
    generated_at: 'node-chat-test',
    total_count: 0,
    counts_by_status: {},
    groups: {
      draft: group('draft', 'Draft'),
      proposed: group('proposed', 'Needs review'),
      deferred: group('deferred', 'Deferred'),
      superseded: group('superseded', 'Superseded'),
      rejected: group('rejected', 'Rejected')
    },
    items: [],
    latest_undoable_patch: null
  };
}

type NodeChatTestState = {
  isTauri: boolean;
  __TAURI_INTERNALS__: {
    invoke: (command: string, args?: Record<string, unknown>) => Promise<unknown>;
  };
  __resolveNodeHistory?: () => void;
  __nodeHistoryDelivered?: boolean;
  __lastNodeChatArgs?: Record<string, unknown>;
  __lastUndoPatchId?: string;
  __nodeChatCallCount?: number;
  __resolveNodeChatTurn?: () => void;
};
