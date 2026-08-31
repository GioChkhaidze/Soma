import { expect, test, type Page } from '@playwright/test';

test('import, compile, review, graph, pin, and job cleanup work as one user flow', async ({ page }) => {
  const pageErrors: string[] = [];
  const consoleErrors: string[] = [];
  page.on('pageerror', (error) => pageErrors.push(error.message));
  page.on('console', (message) => {
    if (message.type() === 'error') consoleErrors.push(message.text());
  });
  await page.addInitScript(() => {
    const state = globalThis as typeof globalThis & { __windowErrors?: string[] };
    state.__windowErrors = [];
    window.addEventListener('error', (event) => {
      state.__windowErrors?.push(event.message);
    });
  });
  await installCoreWorkflowMock(page);
  await page.goto('/');

  await page.getByRole('button', { name: 'Sources' }).click();
  const sources = page.getByLabel('Workspace import');
  await sources.getByPlaceholder('Source file').fill('C:\\Research\\conversation.md');
  await sources.getByRole('button', { name: 'Import source' }).click();
  await expect(sources.getByText('Imported 3 messages into 2 chunks.')).toBeVisible();
  await expect(sources.getByText(/1 source imported.*2 chunks/)).toBeVisible();

  await sources.getByRole('button', { name: 'Compile Graph' }).click();
  const review = page.getByLabel('Graph review tray');
  await expect(review).toBeVisible();
  await expect(review.getByText('Proposed node body: A compiled source-backed topic.')).toBeVisible();
  await review.getByRole('button', { name: 'Accept', exact: true }).click();
  await expect(review.getByText('No updates in this filter.')).toBeVisible();
  await review.getByRole('button', { name: /Accepted/ }).click();
  await expect(review.getByRole('button', { name: /Compiled Topic/ })).toBeVisible();

  await page.getByRole('button', { name: 'Close panel' }).click();
  const graph = page.getByLabel('Conversation graph');
  const compiledNode = page.getByLabel('Compiled Topic, concept');
  await expect(compiledNode).toBeVisible();
  await expectNoWindowErrors(page);
  await graph.getByRole('button', { name: 'Tree' }).click();
  await expect(graph.getByRole('button', { name: 'Tree' })).toHaveAttribute('aria-pressed', 'true');
  await graph.getByRole('button', { name: 'Hybrid' }).click();
  await expect(graph.getByRole('button', { name: 'Hybrid' })).toHaveAttribute('aria-pressed', 'true');
  await graph.getByRole('button', { name: 'Graph', exact: true }).click();
  await expect(graph.getByRole('button', { name: 'Graph', exact: true })).toHaveAttribute('aria-pressed', 'true');
  await expectNoWindowErrors(page);

  await compiledNode.click();
  await expect(page.getByLabel('Node detail').getByRole('heading', { name: 'Compiled Topic' })).toBeVisible();
  await expectNoWindowErrors(page);
  await graph.getByRole('button', { name: 'Pin' }).click();
  await expect.poll(() => lastPinnedValue(page)).toBe(true);
  await expectNoWindowErrors(page);

  await page.getByRole('button', { name: 'Compile Graph' }).click();
  const jobs = page.getByLabel('Compile Graph');
  await jobs.getByText('Advanced').click();
  await jobs.getByRole('button', { name: 'Open folder' }).click();
  await expect.poll(() => openedJobId(page)).toBe('job-core-flow');

  page.once('dialog', (dialog) => dialog.accept());
  await jobs.getByRole('button', { name: 'Clear history' }).click();
  await expect(jobs.getByText('No compile runs yet.')).toBeVisible();
  await expect(jobs.getByText('Cleared 1 compile run.')).toBeVisible();
  await page.waitForTimeout(200);
  expect(pageErrors).toEqual([]);
  expect(consoleErrors).toEqual([]);
  expect(await page.evaluate(() => (
    (globalThis as typeof globalThis & { __windowErrors?: string[] }).__windowErrors ?? []
  ))).toEqual([]);
});

async function installCoreWorkflowMock(page: Page) {
  await page.addInitScript(() => {
    const state = globalThis as typeof globalThis & CoreWorkflowTestState;
    let imported = false;
    let compiled = false;
    let accepted = false;
    let jobsCleared = false;

    const workspace = () => ({
      has_workspace: true,
      workspace_dir: 'C:\\Soma\\core-flow-test',
      database_path: 'C:\\Soma\\core-flow-test\\soma.db',
      stats: {
        sources: imported ? 1 : 0,
        conversations: imported ? 1 : 0,
        messages: imported ? 3 : 0,
        chunks: imported ? 2 : 0,
        ftsRows: imported ? 2 : 0
      }
    });
    const canvasNode = () => ({
      id: 'node-core-flow',
      type: 'concept',
      title: 'Compiled Topic',
      preview: 'A compiled source-backed topic.',
      source_chunk_ids: ['chunk-core-flow'],
      body_version: 1,
      body_version_id: 'body-core-flow-1',
      status: 'active',
      markers: ['source_backed', 'ai_compiled']
    });
    const canvas = () => ({
      schema_version: 1,
      nodes: accepted ? [canvasNode()] : [],
      edges: [],
      paths: []
    });
    const proposal = () => ({
      id: 'proposal-core-flow',
      patch_id: 'patch-core-flow',
      job_id: 'job-core-flow',
      source_message_id: null,
      type: 'node',
      status: accepted ? 'accepted' : 'proposed',
      temp_id: 'temp-core-flow',
      title: 'Compiled Topic',
      target: 'temp-core-flow',
      reason: 'Supported by the imported conversation.',
      mutation_payload: { compiled_body: 'A compiled source-backed topic.' },
      related_node_ids: [],
      evidence_count: 1,
      evidence_refs: [{ type: 'chunk', id: 'chunk-core-flow' }],
      risk_markers: [],
      source: {
        kind: 'job',
        id: 'job-core-flow',
        source_message_id: null,
        job_id: 'job-core-flow',
        label: 'Compile Graph'
      },
      created_at: '2026-08-31T00:00:00.000Z',
      decided_at: accepted ? '2026-08-31T00:01:00.000Z' : null,
      decision_reason: null
    });
    const reviewQueue = () => {
      const items = compiled ? [proposal()] : [];
      const group = (status: string, title: string) => {
        const groupItems = items.filter((item) => item.status === status);
        return { status, title, count: groupItems.length, items: groupItems };
      };
      return {
        generated_at: '2026-08-31T00:00:00.000Z',
        total_count: items.length,
        counts_by_status: accepted ? { accepted: 1 } : compiled ? { proposed: 1 } : {},
        groups: {
          draft: group('draft', 'Draft'),
          proposed: group('proposed', 'Needs review'),
          deferred: group('deferred', 'Deferred'),
          superseded: group('superseded', 'Superseded'),
          rejected: group('rejected', 'Rejected')
        },
        items,
        latest_undoable_patch: null
      };
    };
    const files = {
      metadata: 'metadata.json',
      instructions: 'instructions.md',
      runtime: 'runtime.json',
      runtimeResult: 'runtime_result.json',
      chunks: 'chunks.json',
      currentGraphSnapshot: 'current_graph_snapshot.json',
      graphPatchSchema: 'graph_patch.schema.json',
      outputPatch: 'output_patch.json'
    };
    const job = () => ({
      jobId: 'job-core-flow',
      jobDir: 'jobs/job-core-flow',
      jobKind: 'graph_extraction',
      createdAt: '2026-08-31T00:00:00.000Z',
      schemaVersion: 1,
      chunkCount: 2,
      includedChunkCount: 2,
      totalChunkCount: 2,
      truncated: false,
      sourceCount: 1,
      sourceMessageId: null,
      sourceNodeId: null,
      files,
      metadataExists: true,
      outputPatchExists: true,
      outputPatchStatus: 'ready',
      outputPatchProposalCount: 1,
      outputPatchImportable: false,
      importedProposalCount: 1,
      acceptedProposalCount: accepted ? 1 : 0,
      runtimeStatus: 'completed',
      runtimeFailureKind: null,
      runtimeMessage: 'Runtime command completed.',
      runtimeAdapterKind: 'mock',
      runtimeRanAt: '2026-08-31T00:00:30.000Z'
    });
    const compileResult = () => ({
      status: 'review_ready',
      message: 'Review Updates has 1 new proposal.',
      proposalCount: 1,
      job: job(),
      createdJob: {
        jobId: 'job-core-flow',
        jobDir: 'jobs/job-core-flow',
        files,
        chunkCount: 2,
        includedChunkCount: 2,
        totalChunkCount: 2,
        truncated: false
      },
      run: {
        jobId: 'job-core-flow',
        jobDir: 'jobs/job-core-flow',
        adapterKind: 'mock',
        status: 'completed',
        failureKind: null,
        message: 'Runtime command completed.',
        outputPatchStatus: 'ready',
        outputPatchProposalCount: 1,
        outputPatchImportable: true
      },
      importResult: {
        jobId: 'job-core-flow',
        valid: true,
        imported: true,
        trusted: false,
        proposalCount: 1,
        proposals: [{ id: 'proposal-core-flow' }],
        errors: [],
        warnings: []
      }
    });

    state.isTauri = true;
    state.__TAURI_INTERNALS__ = {
      invoke: async (command, args) => {
        if (command === 'get_current_workspace') return workspace();
        if (command === 'get_current_workspace_with_stats') return workspace();
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
            canvas: canvas(),
            layout: { layoutOverrides: {}, pinnedNodeIds: [] }
          };
        }
        if (command === 'load_graph_canvas_snapshot') return canvas();
        if (command === 'load_review_queue') return reviewQueue();
        if (command === 'import_source_file') {
          imported = true;
          return {
            sourceId: 'source-core-flow',
            rawPath: 'sources/source-core-flow.md',
            conversations: [{ id: 'conversation-core-flow', title: 'Research', messageCount: 3 }],
            messageCount: 3,
            chunkCount: 2
          };
        }
        if (command === 'compile_graph_workspace') {
          compiled = true;
          return compileResult();
        }
        if (command === 'list_jobs') return { jobs: compiled && !jobsCleared ? [job()] : [] };
        if (command === 'accept_graph_proposal') {
          accepted = true;
          return {
            proposalId: String(args?.proposal_id ?? ''),
            status: 'accepted',
            entityType: 'node',
            entityId: 'node-core-flow'
          };
        }
        if (command === 'load_graph_node_detail') {
          return {
            ...canvasNode(),
            compiled_body: 'A compiled source-backed topic.',
            evidence: [],
            update_history: [],
            relations: { items: [], is_partial: false }
          };
        }
        if (command === 'list_node_messages') return [];
        if (command === 'persist_node_position') {
          state.__lastPinnedValue = args?.pinned;
          return {
            node_id: String(args?.node_id ?? ''),
            x: Number(args?.x ?? 0),
            y: Number(args?.y ?? 0),
            left: Number(args?.x ?? 0),
            top: Number(args?.y ?? 0),
            pinned: args?.pinned === true,
            updated_at: '2026-08-31T00:02:00.000Z'
          };
        }
        if (command === 'open_job_folder') {
          state.__openedJobId = String(args?.job_id ?? '');
          return { jobId: state.__openedJobId, jobDir: 'jobs/job-core-flow', opened: true };
        }
        if (command === 'clear_job_history') {
          jobsCleared = true;
          return { removed: 1 };
        }
        throw new Error(`Unexpected Tauri command in core workflow test: ${command}`);
      }
    };
  });
}

async function lastPinnedValue(page: Page) {
  return page.evaluate(() => (
    (globalThis as typeof globalThis & CoreWorkflowTestState).__lastPinnedValue
  ));
}

async function openedJobId(page: Page) {
  return page.evaluate(() => (
    (globalThis as typeof globalThis & CoreWorkflowTestState).__openedJobId
  ));
}

async function expectNoWindowErrors(page: Page) {
  await page.waitForTimeout(200);
  expect(await page.evaluate(() => (
    (globalThis as typeof globalThis & { __windowErrors?: string[] }).__windowErrors ?? []
  ))).toEqual([]);
}

type CoreWorkflowTestState = {
  isTauri: boolean;
  __lastPinnedValue?: unknown;
  __openedJobId?: string;
  __TAURI_INTERNALS__: {
    invoke: (command: string, args?: Record<string, unknown>) => Promise<unknown>;
  };
};
