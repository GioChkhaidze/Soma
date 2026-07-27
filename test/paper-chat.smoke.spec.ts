import { expect, test, type Locator, type Page } from '@playwright/test';

test('paper chat waits for current-page extraction before sending', async ({ page }) => {
  await installTauriMock(page);
  let releaseWorker = () => {};
  let markWorkerRequested = () => {};
  const workerRelease = new Promise<void>((resolve) => {
    releaseWorker = resolve;
  });
  const workerRequested = new Promise<void>((resolve) => {
    markWorkerRequested = resolve;
  });
  await page.route('**/*pdf.worker*', async (route) => {
    markWorkerRequested();
    await workerRelease;
    await route.continue();
  });
  await page.goto('/');
  await page.locator('input[type="file"]').setInputFiles({
    name: 'context-readiness.pdf',
    mimeType: 'application/pdf',
    buffer: minimalPdf()
  });
  await workerRequested;

  const chatInput = page.getByPlaceholder('Ask Soma');
  const chatForm = page.locator('.graphChatForm');
  const sendButton = page.getByRole('button', { name: 'Send message' });
  await chatInput.fill('Use the page that is still loading.');
  await expect(chatForm).toHaveAttribute('aria-busy', 'true');
  await expect(sendButton).toBeDisabled();
  await chatInput.press('Enter');
  expect(await page.evaluate(() => (
    globalThis as typeof globalThis & { __lastGraphChatArgs?: Record<string, unknown> }
  ).__lastGraphChatArgs)).toBeUndefined();

  releaseWorker();
  await expect(page.getByLabel('Paper context')).toContainText('p. 1 / 2');
  await expect(chatForm).toHaveAttribute('aria-busy', 'false');
  await expect(sendButton).toBeEnabled();
  await chatInput.press('Enter');
  await expect.poll(async () => {
    const args = await page.evaluate(() => (
      globalThis as typeof globalThis & { __lastGraphChatArgs?: Record<string, unknown> }
    ).__lastGraphChatArgs);
    return (args?.reading_context as { page_text?: string } | undefined)?.page_text;
  }).toContain('Soma paper selection on page one');
});

test('paper state and chat dock stay stable while reading', async ({ page }) => {
  await installTauriMock(page);
  await page.setViewportSize({ width: 1280, height: 820 });
  await page.goto('/');
  await expect(page.getByLabel('Conversation graph')).toBeVisible();

  await page.locator('input[type="file"]').setInputFiles({
    name: 'grounded-reading.pdf',
    mimeType: 'application/pdf',
    buffer: minimalPdf()
  });

  const reader = page.locator('.paperReader');
  const viewport = page.locator('.paperViewport');
  const zoom = page.locator('.paperZoom');
  const context = page.getByLabel('Paper context');
  await expect(reader).toBeVisible();
  await expect(page.locator('.textLayer span').first()).toContainText('Soma paper selection');
  await expect(context).toBeVisible();
  await expect(page.getByRole('button', { name: 'Graph capture off' })).toBeVisible();

  await selectText(page.locator('.textLayer span').first());
  await expect(page.locator('.paperSelection')).toContainText('Selection');
  await expect(context.locator('strong')).toContainText('Selection from p. 1');
  await selectAcrossPaperAndChat(page);
  await expect(page.locator('.paperSelection')).toHaveAttribute('title', 'Soma paper selection on page one.');

  await expect.poll(() => viewport.evaluate((element) => element.scrollHeight > element.clientHeight)).toBe(true);
  await viewport.evaluate((element) => {
    element.scrollTop = Math.min(420, element.scrollHeight - element.clientHeight);
  });
  await expect.poll(() => viewport.evaluate((element) => element.scrollTop)).toBeGreaterThan(0);

  const initialZoom = await zoom.textContent();
  await page.getByRole('button', { name: 'Zoom in' }).click();
  await expect.poll(() => zoom.textContent()).not.toBe(initialZoom);
  const buttonZoom = await zoom.textContent();
  await viewport.dispatchEvent('wheel', {
    bubbles: true,
    cancelable: true,
    clientX: 640,
    clientY: 410,
    ctrlKey: true,
    deltaMode: 0,
    deltaY: -120
  });
  await expect.poll(() => zoom.textContent()).not.toBe(buttonZoom);
  const savedZoom = await zoom.textContent();

  const panel = page.getByLabel('Graph chat');
  const dock = page.locator('.statusDock');
  const composer = page.locator('.graphChatComposer');
  const chatInput = page.getByPlaceholder('Ask Soma');
  const initialDockBox = await requiredBox(dock);
  const initialComposerBox = await requiredBox(composer);
  await chatInput.click();
  await chatInput.pressSequentially('Why does this matter?');
  await expect(panel).toHaveClass(/isExpanded/);
  expect(await requiredBox(dock)).toEqual(initialDockBox);
  expect(await requiredBox(composer)).toEqual(initialComposerBox);

  await page.locator('.paperToolbar').hover();
  await page.waitForTimeout(250);
  await expect(panel).toHaveClass(/isExpanded/);
  await expect(chatInput).toBeFocused();
  expect(await requiredBox(dock)).toEqual(initialDockBox);
  expect(await requiredBox(composer)).toEqual(initialComposerBox);
  await expectOpaqueSurface(composer);
  await expectOpaqueSurface(context);

  await chatInput.press('Enter');
  const transcript = page.locator('.graphChatTranscriptInner');
  await expect(transcript).toBeVisible();
  await expectOpaqueSurface(transcript);
  const chatArgs = await page.evaluate(() => (
    globalThis as typeof globalThis & { __lastGraphChatArgs?: Record<string, unknown> }
  ).__lastGraphChatArgs);
  expect(chatArgs).toMatchObject({
    capture_graph_changes: false,
    reading_context: {
      kind: 'pdf',
      document_name: 'grounded-reading.pdf',
      page_number: 1,
      selected_text: 'Soma paper selection on page one.'
    }
  });

  await page.getByRole('button', { name: 'Next page' }).click();
  await expect(page.locator('.paperPageCount')).toHaveText('2 / 2');
  await chatInput.fill('What changes on this page?');
  await chatInput.press('Enter');
  await expect.poll(async () => {
    const args = await page.evaluate(() => (
      globalThis as typeof globalThis & { __lastGraphChatArgs?: Record<string, unknown> }
    ).__lastGraphChatArgs);
    return (args?.reading_context as { page_number?: number } | undefined)?.page_number;
  }).toBe(2);
  const secondPageArgs = await page.evaluate(() => (
    globalThis as typeof globalThis & { __lastGraphChatArgs?: Record<string, unknown> }
  ).__lastGraphChatArgs);
  const secondPageText = (
    secondPageArgs?.reading_context as { page_text?: string } | undefined
  )?.page_text ?? '';
  expect(secondPageText).not.toContain('page one');
  expect(secondPageText).toContain('Second page reading context');

  await page.setViewportSize({ width: 960, height: 640 });
  const activePageScroll = await viewport.evaluate((element) => element.scrollTop);
  const compactDockBox = await requiredBox(dock);
  const compactComposerBox = await requiredBox(composer);
  await page.locator('.paperToolbar').hover();
  await page.waitForTimeout(250);
  await expect(panel).toHaveClass(/isExpanded/);
  expect(await requiredBox(dock)).toEqual(compactDockBox);
  expect(await requiredBox(composer)).toEqual(compactComposerBox);
  const compactTranscriptBox = await requiredBox(transcript);
  expect(compactTranscriptBox.x).toBeGreaterThanOrEqual(0);
  expect(compactTranscriptBox.y).toBeGreaterThanOrEqual(0);
  expect(compactTranscriptBox.x + compactTranscriptBox.width).toBeLessThanOrEqual(960);
  expect(compactTranscriptBox.y + compactTranscriptBox.height).toBeLessThanOrEqual(640);

  await chatInput.press('Escape');
  await expect(panel).toHaveClass(/isCollapsed/);
  await expect(chatInput).not.toBeFocused();

  await page.getByRole('button', { name: 'Graph', exact: true }).click();
  await expect(reader).toHaveCount(1);
  await expect(reader).toBeHidden();
  expect(await viewport.evaluate((element) => element.scrollTop)).toBe(activePageScroll);
  expect(await zoom.textContent()).toBe(savedZoom);
  await expect(page.getByRole('button', { name: 'Graph capture off' })).toBeVisible();
  await page.getByRole('button', { name: 'Graph capture off' }).click();
  await expect(page.getByRole('button', { name: 'Graph capture on' })).toBeVisible();

  await page.getByRole('button', { name: 'Paper', exact: true }).click();
  await expect(reader).toBeVisible();
  await expect(page.locator('.paperPageCount')).toHaveText('2 / 2');
  expect(await viewport.evaluate((element) => element.scrollTop)).toBe(activePageScroll);
  expect(await zoom.textContent()).toBe(savedZoom);
  await expect(page.locator('.paperSelection')).toContainText('Selection');
  await expect(context.locator('strong')).toContainText('Selection from p. 1');
  await expect(page.getByRole('button', { name: 'Graph capture on' })).toBeVisible();

  await chatInput.fill('Keep this explanation in the graph.');
  await chatInput.press('Enter');
  await expect.poll(async () => {
    const args = await page.evaluate(() => (
      globalThis as typeof globalThis & { __lastGraphChatArgs?: Record<string, unknown> }
    ).__lastGraphChatArgs);
    return args?.capture_graph_changes;
  }).toBe(true);
  const undo = panel.getByRole('button', { name: 'Undo graph update' });
  await expect(undo).toBeVisible();
  await undo.click();
  await expect.poll(() => page.evaluate(() => (
    globalThis as typeof globalThis & { __lastUndoPatchId?: string }
  ).__lastUndoPatchId)).toBe('paper-chat-patch');
  await expect(undo).toHaveCount(0);
  await page.getByRole('button', { name: 'Clear selection from page 1' }).click();
  await expect(page.locator('.paperSelection')).toHaveCount(0);
  await expect(context.locator('strong')).toHaveCount(0);

  await page.getByRole('button', { name: 'Close paper' }).click();
  await expect(reader).toHaveCount(0);
  await expect(page.getByRole('button', { name: 'Open paper' })).toBeVisible();
  await expect(page.getByRole('button', { name: 'Graph capture on' })).toBeVisible();
  await page.locator('input[type="file"]').setInputFiles({
    name: 'new-reading-session.pdf',
    mimeType: 'application/pdf',
    buffer: minimalPdf()
  });
  await expect(reader).toBeVisible();
  await expect(page.getByRole('button', { name: 'Graph capture off' })).toBeVisible();
});

test('a delayed bootstrap cannot replace a newly opened workspace', async ({ page }) => {
  await installWorkspaceSwitchMock(page);
  await page.goto('/');
  await expect(page.getByLabel('Conversation graph')).toBeVisible();

  await page.getByRole('button', { name: 'Sources' }).click();
  await page.getByRole('button', { name: 'Open workspace' }).click();

  await expect(page.getByLabel('Current B, concept')).toBeVisible();
  await page.waitForTimeout(350);
  await expect(page.getByLabel('Current B, concept')).toBeVisible();
  await expect(page.getByLabel('Stale A, concept')).toHaveCount(0);
});

async function installTauriMock(page: Page) {
  await page.addInitScript(() => {
    const workspace = {
      has_workspace: false,
      workspace_dir: null,
      database_path: null
    };
    let graphTurnCount = 0;
    let undoReady = false;
    let undoSourceMessageId: string | null = null;
    const reviewQueue = () => {
      const group = (status: string, title: string) => ({ status, title, count: 0, items: [] });
      return {
        generated_at: 'paper-chat-test',
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
          patch_id: 'paper-chat-patch',
          source: 'graph_thread_message',
          source_message_id: undoSourceMessageId,
          change_count: 1
        } : null
      };
    };
    const brainSettings = {
      providerId: 'codex_sdk',
      model: '',
      endpoint: '',
      authProfile: '',
      credentialConfigured: true,
      updatedAt: '2026-07-25T00:00:00.000Z'
    };
    const tauri = globalThis as typeof globalThis & {
      isTauri: boolean;
      __lastGraphChatArgs?: Record<string, unknown>;
      __lastUndoPatchId?: string;
      __TAURI_INTERNALS__: {
        invoke: (command: string, args?: Record<string, unknown>) => Promise<unknown>;
      };
    };

    tauri.isTauri = true;
    tauri.__TAURI_INTERNALS__ = {
      invoke: async (command, args) => {
        if (command === 'get_current_workspace') return workspace;
        if (command === 'get_brain_settings') return brainSettings;
        if (command === 'create_workspace_auto') {
          return {
            has_workspace: true,
            workspace_dir: 'C:\\Soma\\test-workspace',
            database_path: 'C:\\Soma\\test-workspace\\soma.db'
          };
        }
        if (command === 'load_review_queue') return reviewQueue();
        if (command === 'load_graph_canvas_snapshot') {
          return { schema_version: 1, nodes: [], edges: [], paths: [] };
        }
        if (command === 'send_graph_chat_turn') {
          tauri.__lastGraphChatArgs = args;
          graphTurnCount += 1;
          const captured = args?.capture_graph_changes === true;
          const createdAt = '2026-07-25T00:00:01.000Z';
          undoReady = captured;
          undoSourceMessageId = `graph-assistant-${graphTurnCount}`;
          return {
            user_message_id: `graph-user-${graphTurnCount}`,
            user_message: {
              id: `graph-user-${graphTurnCount}`,
              role: 'user',
              content: graphTurnCount === 1 ? 'Why does this matter?' : 'What changes on this page?',
              created_at: createdAt
            },
            assistant_message: {
              id: `graph-assistant-${graphTurnCount}`,
              role: 'assistant',
              content: 'The selected passage provides the immediate reading context.',
              created_at: createdAt
            },
            context_packet: {
              mode: 'graph_chat',
              user_message: 'Why does this matter?',
              top_matching_nodes: [],
              top_matching_node_bodies: [],
              relevant_path_fragments: [],
              unresolved_questions: [],
              open_tasks: [],
              recent_graph_thread_messages: [],
              source_evidence_excerpts: [],
              used_graph_areas: []
            },
            used_graph_areas: [],
            proposal_count: captured ? 1 : 0,
            patch_import_status: captured ? 'accepted_to_graph' : 'none',
            patch_import_result: {
              messageId: `graph-assistant-${graphTurnCount}`,
              patchId: captured ? 'paper-chat-patch' : undefined,
              valid: true,
              imported: captured,
              trusted: captured,
              proposal_status: captured ? 'accepted' : undefined,
              proposalCount: captured ? 1 : 0,
              proposals: [],
              errors: [],
              warnings: []
            },
            runtime_status: 'completed',
            runtime_adapter_kind: 'mock',
            runtime_failure_kind: null,
            runtime_message: 'Completed.'
          };
        }
        if (command === 'undo_graph_patch') {
          tauri.__lastUndoPatchId = String(args?.patch_id ?? '');
          undoReady = false;
          return { patchId: tauri.__lastUndoPatchId, undoneCount: 1, status: 'undone' };
        }
        throw new Error(`Unexpected Tauri command in UI smoke test: ${command}`);
      }
    };
  });
}

async function installWorkspaceSwitchMock(page: Page) {
  await page.addInitScript(() => {
    const stats = { sources: 0, conversations: 0, messages: 0, chunks: 0, ftsRows: 0 };
    const workspaceA = {
      has_workspace: true,
      workspace_dir: 'C:\\Soma\\workspace-a',
      database_path: 'C:\\Soma\\workspace-a\\soma.db',
      stats
    };
    const workspaceB = {
      has_workspace: true,
      workspace_dir: 'C:\\Soma\\workspace-b',
      database_path: 'C:\\Soma\\workspace-b\\soma.db',
      stats
    };
    const node = (id: string, title: string) => ({
      id,
      type: 'concept',
      title,
      preview: `${title} preview`,
      source_chunk_ids: [],
      body_version: 1,
      status: 'active',
      markers: []
    });
    const bootstrap = (id: string, title: string) => ({
      canvas: {
        schema_version: 1,
        nodes: [node(id, title)],
        edges: [],
        paths: []
      },
      layout: {
        layoutOverrides: {},
        pinnedNodeIds: []
      }
    });
    const brainSettings = {
      providerId: 'codex_sdk',
      model: '',
      endpoint: '',
      authProfile: '',
      credentialConfigured: true,
      updatedAt: '2026-07-25T00:00:00.000Z'
    };
    let bootstrapCount = 0;
    const tauri = globalThis as typeof globalThis & {
      isTauri: boolean;
      __TAURI_INTERNALS__: {
        invoke: (command: string) => Promise<unknown>;
      };
    };

    tauri.isTauri = true;
    tauri.__TAURI_INTERNALS__ = {
      invoke: async (command) => {
        if (command === 'get_current_workspace') return workspaceA;
        if (command === 'get_brain_settings') return brainSettings;
        if (command === 'open_workspace_picker') return workspaceB;
        if (command === 'load_workspace_bootstrap') {
          bootstrapCount += 1;
          if (bootstrapCount === 1) {
            await new Promise((resolve) => window.setTimeout(resolve, 250));
            return bootstrap('node-a', 'Stale A');
          }
          return bootstrap('node-b', 'Current B');
        }
        throw new Error(`Unexpected Tauri command in workspace switch test: ${command}`);
      }
    };
  });
}

async function selectText(text: Locator) {
  await text.evaluate((element) => {
    const selection = window.getSelection();
    const range = document.createRange();
    range.selectNodeContents(element);
    selection?.removeAllRanges();
    selection?.addRange(range);
    document.dispatchEvent(new Event('selectionchange'));
  });
}

async function selectAcrossPaperAndChat(page: Page) {
  await page.evaluate(() => {
    const start = document.querySelector('.textLayer span')?.firstChild;
    const end = document.querySelector('[aria-label="Paper context"] strong')?.firstChild;
    if (!start || !end) throw new Error('Expected paper and chat text for cross-surface selection');
    const range = document.createRange();
    range.setStart(start, 0);
    range.setEnd(end, end.textContent?.length ?? 0);
    const selection = window.getSelection();
    selection?.removeAllRanges();
    selection?.addRange(range);
    document.dispatchEvent(new Event('selectionchange'));
  });
}

async function expectOpaqueSurface(surface: Locator) {
  const color = await surface.evaluate((element) => getComputedStyle(element).backgroundColor);
  expect(isOpaqueColor(color), `expected an opaque background, received ${color}`).toBe(true);
}

function isOpaqueColor(color: string) {
  if (color.startsWith('rgb(')) return true;
  const alpha = color.match(/^rgba\(.+,\s*([0-9.]+)\)$/)?.[1];
  return alpha !== undefined && Number(alpha) === 1;
}

async function requiredBox(locator: Locator) {
  const box = await locator.boundingBox();
  expect(box).not.toBeNull();
  return box!;
}

function minimalPdf() {
  const firstPage = 'BT /F1 24 Tf 72 720 Td (Soma paper selection on page one.) Tj ET';
  const secondPage = 'BT /F1 24 Tf 72 720 Td (Second page reading context.) Tj ET';
  const objects = [
    '<< /Type /Catalog /Pages 2 0 R >>',
    '<< /Type /Pages /Kids [3 0 R 4 0 R] /Count 2 >>',
    pageObject(6),
    pageObject(7),
    '<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>',
    streamObject(firstPage),
    streamObject(secondPage)
  ];
  const offsets = [0];
  let pdf = '%PDF-1.4\n';

  objects.forEach((object, index) => {
    offsets.push(Buffer.byteLength(pdf, 'ascii'));
    pdf += `${index + 1} 0 obj\n${object}\nendobj\n`;
  });

  const xrefOffset = Buffer.byteLength(pdf, 'ascii');
  pdf += `xref\n0 ${objects.length + 1}\n0000000000 65535 f \n`;
  pdf += offsets.slice(1).map((offset) => `${String(offset).padStart(10, '0')} 00000 n \n`).join('');
  pdf += `trailer\n<< /Size ${objects.length + 1} /Root 1 0 R >>\nstartxref\n${xrefOffset}\n%%EOF\n`;
  return Buffer.from(pdf, 'ascii');
}

function pageObject(contentsId: number) {
  return [
    '<< /Type /Page /Parent 2 0 R',
    '/MediaBox [0 0 612 792]',
    '/Resources << /Font << /F1 5 0 R >> >>',
    `/Contents ${contentsId} 0 R >>`
  ].join(' ');
}

function streamObject(content: string) {
  return `<< /Length ${Buffer.byteLength(content, 'ascii')} >>\nstream\n${content}\nendstream`;
}
