import assert from 'node:assert/strict';
import test from 'node:test';

import { clearMocks, mockIPC } from '@tauri-apps/api/mocks';

import { projectGraphView } from '../apps/desktop/src/shared/graph/projection.ts';
import {
  contextAreasForMessage,
  displayGraphChatError,
  latestUndoableGraphPatch
} from '../apps/desktop/src/features/graph-chat/graphChatViewModel.ts';
import {
  chatUpdateSummaryForMessage,
  displayChatMessageContent,
  proposalLinesForMessage,
  proposalTypeLabel
} from '../apps/desktop/src/shared/data/chatReview.ts';
import {
  graphDetailLevelForZoom,
  toReactFlowGraph
} from '../apps/desktop/src/features/graph-workspace/reactFlowAdapter.ts';
import {
  latestUndoableNodePatch
} from '../apps/desktop/src/features/node-chat/nodeChatViewModel.ts';
import {
  mergeMessagesById,
  settleMessagesById
} from '../apps/desktop/src/shared/data/messageMerge.ts';
import {
  clampSearchIndex,
  highlightedTextParts,
  nextSearchIndex,
  resultCountLabel
} from '../apps/desktop/src/features/search/searchViewModel.ts';
import {
  activeCredentialCue,
  aiSettingsSummary,
  filterModelOptions,
  modelOptionsForSettings
} from '../apps/desktop/src/features/settings/aiSettingsViewModel.ts';
import {
  aiProviderGroups,
  providerById
} from '../apps/desktop/src/features/settings/aiProviderCatalog.ts';
import {
  brainSetupIssue,
  credentialConfiguredForProvider,
  defaultAiSettings,
  mergePersistedAiSettings,
  rememberProviderCredential,
  sanitizeAiSettings
} from '../apps/desktop/src/features/settings/aiSettingsPolicy.ts';
import {
  chatTurnErrorMessage,
  chatUpdateNotice,
  compileFailureMessage,
  formatError,
  reviewReadyNotice
} from '../apps/desktop/src/app/controllers/controllerUtils.ts';
import {
  activateWorkspaceRequestOwner,
  initialWorkspaceRequestOwner,
  ownsWorkspaceRequest
} from '../apps/desktop/src/app/controllers/workspaceRequestOwnership.ts';
import { isStorageBusyMessage, STORAGE_BUSY_MESSAGE } from '../apps/desktop/src/shared/data/storageBusy.ts';
import { jobRunFailureMessage } from '../apps/desktop/src/features/job-runs/jobFlow.ts';
import {
  compactJobStatusItems,
  jobRunPrimaryActionState,
  jobRunsPanelNotice,
  primaryJobRun
} from '../apps/desktop/src/features/job-runs/jobRunsViewModel.ts';
import { layoutNodeFromPosition } from '../apps/desktop/src/shared/data/layoutState.ts';
import {
  pendingReviewCount as appPendingReviewCount
} from '../apps/desktop/src/shared/data/reviewQueue.ts';
import {
  reviewFilterCount,
  reviewItemActions,
  reviewMutationPreview,
  reviewNoticeItems,
  noticeText,
  visibleReviewItems
} from '../apps/desktop/src/features/merge-review/reviewTrayViewModel.ts';
import {
  brainModelListResultSchema,
  compileGraphWorkspaceResultSchema,
  graphChatTurnArgsSchema,
  graphReviewQueueReadModelSchema,
  graphNodeSchema,
  graphChatTurnResultSchema,
  listJobsResultSchema,
  nodeMessageArgsSchema,
  nodeChatTurnResultSchema,
  updateNodeBodyArgsSchema
} from '../packages/contracts/src/schemas.ts';
import {
  BRAIN_PROVIDER_IDS,
  CHAT_MESSAGE_MAX_CHARACTERS
} from '../packages/contracts/src/appCommands.ts';
import { NODE_BODY_MAX_CHARACTERS } from '../packages/contracts/src/graph.ts';
import {
  compileGraphWorkspace,
  runCompileJob,
  sendGraphWorkspaceChatTurn
} from '../apps/desktop/src/shared/commands/graphWorkspaceCommands.ts';
import { sendNodeWorkspaceChatTurn } from '../apps/desktop/src/shared/commands/nodeChatCommands.ts';
import { contractSchema, invokeRequired } from '../apps/desktop/src/shared/commands/tauriCommandClient.ts';

test('Tauri command boundary validates lazy contract schemas', async (context) => {
  const originalWindow = globalThis.window;
  const originalIsTauri = globalThis.isTauri;
  globalThis.window = globalThis;
  globalThis.isTauri = true;
  context.after(() => {
    clearMocks();
    if (originalWindow === undefined) delete globalThis.window;
    else globalThis.window = originalWindow;
    if (originalIsTauri === undefined) delete globalThis.isTauri;
    else globalThis.isTauri = originalIsTauri;
  });

  const validInput = {
    providerId: 'openai',
    model: 'gpt-5',
    endpoint: '',
    authProfile: '',
    apiKey: 'test-key'
  };
  const validSettings = {
    providerId: 'openai',
    model: 'gpt-5',
    endpoint: '',
    authProfile: '',
    credentialConfigured: true
  };
  const calls = [];
  mockIPC((command, args) => {
    calls.push({ command, args });
    return validSettings;
  });

  const result = await invokeRequired(
    'save_brain_settings',
    contractSchema('brainSettingsSchema'),
    contractSchema('saveBrainSettingsArgsSchema'),
    { settings: validInput }
  );
  assert.deepEqual(result, validSettings);
  assert.deepEqual(calls, [{ command: 'save_brain_settings', args: { settings: validInput } }]);

  calls.length = 0;
  await assert.rejects(
    invokeRequired(
      'save_brain_settings',
      contractSchema('brainSettingsSchema'),
      contractSchema('saveBrainSettingsArgsSchema'),
      { settings: {} }
    ),
    /save_brain_settings args failed contract validation: settings\.providerId/
  );
  assert.equal(calls.length, 0);

  mockIPC((command, args) => {
    calls.push({ command, args });
    return { ...validSettings, credentialConfigured: 'yes' };
  });
  await assert.rejects(
    invokeRequired('get_brain_settings', contractSchema('brainSettingsSchema')),
    /get_brain_settings result failed contract validation: credentialConfigured/
  );
  assert.deepEqual(calls, [{ command: 'get_brain_settings', args: {} }]);
});

test('chat command contracts share one Unicode character bound', () => {
  const maximum = '🧠'.repeat(CHAT_MESSAGE_MAX_CHARACTERS);
  const graphArgs = graphChatTurnArgsSchema.parse({ request_id: 'graph-test', content: `  ${maximum}  ` });
  const nodeArgs = nodeMessageArgsSchema.parse({
    node_id: 'node-1',
    content: maximum,
    request_id: 'node-test',
    capture_graph_changes: false
  });

  assert.equal(Array.from(graphArgs.content).length, CHAT_MESSAGE_MAX_CHARACTERS);
  assert.equal(Array.from(nodeArgs.content).length, CHAT_MESSAGE_MAX_CHARACTERS);
  for (const parse of [
    () => graphChatTurnArgsSchema.parse({ request_id: 'graph-test', content: `${maximum}x` }),
    () => nodeMessageArgsSchema.parse({
      node_id: 'node-1',
      content: `${maximum}x`,
      request_id: 'node-test',
      capture_graph_changes: false
    })
  ]) {
    assert.throws(parse, /Chat messages are limited to 4,000 characters/);
  }
});

test('direct node body updates share the Unicode character bound', () => {
  for (const character of ['x', '\u{1F9E0}']) {
    const maximum = character.repeat(NODE_BODY_MAX_CHARACTERS);
    const parsed = updateNodeBodyArgsSchema.parse({
      node_id: 'node-1',
      compiled_body: maximum
    });
    assert.equal(Array.from(parsed.compiled_body).length, NODE_BODY_MAX_CHARACTERS);
    assert.throws(
      () => updateNodeBodyArgsSchema.parse({
        node_id: 'node-1',
        compiled_body: `${maximum}${character}`
      }),
      /Node bodies are limited to 32,000 characters/
    );
  }
});

test('mutating graph and chat commands remain pending until Tauri invoke settles', async (context) => {
  const originalWindow = globalThis.window;
  const originalIsTauri = globalThis.isTauri;
  const originalSetTimeout = globalThis.setTimeout;
  globalThis.window = globalThis;
  globalThis.isTauri = true;
  globalThis.setTimeout = (callback, _delay, ...args) => {
    queueMicrotask(() => callback(...args));
    return 0;
  };
  context.after(() => {
    clearMocks();
    globalThis.setTimeout = originalSetTimeout;
    if (originalWindow === undefined) delete globalThis.window;
    else globalThis.window = originalWindow;
    if (originalIsTauri === undefined) delete globalThis.isTauri;
    else globalThis.isTauri = originalIsTauri;
  });

  const pendingInvokes = [];
  mockIPC((command) => new Promise((resolve, reject) => {
    pendingInvokes.push({ command, resolve, reject });
  }));
  const cases = [
    ['compile_graph_workspace', () => compileGraphWorkspace()],
    ['run_compile_job', () => runCompileJob('job-1')],
    ['send_graph_chat_turn', () => sendGraphWorkspaceChatTurn('Explain this graph.', [], { requestId: 'graph-test' })],
    ['send_node_chat_turn', () => sendNodeWorkspaceChatTurn('node-1', 'Explain this node.', 'node-test', false)]
  ];

  for (const [expectedCommand, start] of cases) {
    const operation = start();
    let settled = false;
    const observed = operation.then(
      (value) => ({ status: 'fulfilled', value }),
      (error) => ({ status: 'rejected', error })
    );
    void observed.then(() => {
      settled = true;
    });

    while (pendingInvokes.length === 0) {
      await new Promise(setImmediate);
    }
    const invoke = pendingInvokes.shift();
    assert.equal(invoke.command, expectedCommand);
    await Promise.resolve();
    assert.equal(settled, false, `${expectedCommand} settled before its invoke`);

    invoke.reject(new Error(`${expectedCommand} backend settled`));
    const outcome = await observed;
    assert.equal(outcome.status, 'rejected');
    assert.match(String(outcome.error), new RegExp(`${expectedCommand} backend settled`));
    assert.equal(settled, true);
  }
});

test('graph workspace fixture matches the accepted graph snapshot shape', () => {
  const snapshot = graphSnapshotFixture();

  assert.equal(snapshot.schema_version, 1);
  assert.equal(snapshot.nodes.length, 4);
  assert.equal(snapshot.edges.length, 5);

  for (const node of snapshot.nodes) {
    assert.equal(node.status, 'active');
    assert.equal(typeof node.title, 'string');
    assert.equal(typeof node.compiled_body, 'string');
    assert.equal(Number.isInteger(node.body_version), true);
    assert.equal(Array.isArray(node.source_chunk_ids), true);
    assert.equal(Array.isArray(node.evidence), true);
    assert.equal(Array.isArray(node.update_history), true);
  }
});

test('node detail contract requires bounded semantic relations', () => {
  const detail = {
    id: 'node-detail',
    type: 'concept',
    title: 'Node detail',
    preview: null,
    compiled_body: 'A complete body.',
    source_chunk_ids: [],
    body_version: 1,
    status: 'active',
    markers: [],
    evidence: [],
    update_history: [],
    relations: {
      items: [{
        edge_id: 'edge-detail',
        type: 'depends_on',
        direction: 'outgoing',
        bridge_text: null,
        neighbor: { id: 'node-neighbor', title: 'Neighbor' }
      }],
      is_partial: true
    }
  };

  const parsed = graphNodeSchema.parse(detail);
  assert.equal(parsed.relations.items[0].bridge_text, '');
  assert.equal(parsed.relations.items[0].neighbor.title, 'Neighbor');
  assert.equal(parsed.relations.is_partial, true);
  const { relations: _relations, ...withoutRelations } = detail;
  assert.equal(graphNodeSchema.safeParse(withoutRelations).success, false);
  assert.equal('body_sections' in parsed, false);
  assert.equal('body_max_words' in parsed, false);
});


test('graph workspace projection changes visible edges without changing graph truth', () => {
  const snapshot = graphSnapshotFixture();
  const tree = projectGraphView(snapshot, { connectedness: 0 });
  const hybrid = projectGraphView(snapshot, { connectedness: 50 });
  const graph = projectGraphView(snapshot, { connectedness: 100 });

  assert.equal(tree.projection.mode, 'tree');
  assert.equal(hybrid.projection.mode, 'hybrid');
  assert.equal(graph.projection.mode, 'graph');
  assert.equal(tree.edges.length < hybrid.edges.length, true);
  assert.equal(hybrid.edges.length < graph.edges.length, true);
  assert.equal(snapshot.edges.length, 5);
  assert.equal(tree.nodes.every((node) => node.layout), true);
});

test('React Flow adapter maps projected graph state without owning graph truth', () => {
  const snapshot = graphSnapshotFixture();
  const pinnedNodeId = 'node_graph_chat_context';
  const selectedNodeId = 'node_graph_workspace';
  const projected = projectGraphView(snapshot, {
    connectedness: 50,
    pinnedNodeIds: [pinnedNodeId]
  });
  const beforeAdapter = JSON.stringify(projected);

  const flowGraph = toReactFlowGraph(projected, selectedNodeId, [pinnedNodeId]);

  assert.equal(JSON.stringify(projected), beforeAdapter);
  assert.deepEqual(flowGraph.nodes.map((node) => node.id), projected.nodes.map((node) => node.id));
  assert.equal(flowGraph.edges.length, projected.edges.length);

  const selectedNode = flowGraph.nodes.find((node) => node.id === selectedNodeId);
  assert.equal(selectedNode?.selected, true);
  assert.equal(selectedNode?.data.selected, true);
  assert.equal(selectedNode?.draggable, true);
  assert.equal(selectedNode?.deletable, false);

  const projectedSelected = projected.nodes.find((node) => node.id === selectedNodeId);
  assert.deepEqual(selectedNode?.position, {
    x: projectedSelected?.layout.x,
    y: projectedSelected?.layout.y
  });

  const pinnedNode = flowGraph.nodes.find((node) => node.id === pinnedNodeId);
  assert.equal(pinnedNode?.data.pinned, true);
  assert.equal(flowGraph.edges.every((edge) => edge.selectable === false), true);
  assert.equal(flowGraph.edges.every((edge) => edge.type === 'default'), true);
  assert.equal(flowGraph.edges.every((edge) => edge.sourceHandle?.startsWith('source-')), true);
  assert.equal(flowGraph.edges.every((edge) => edge.targetHandle?.startsWith('target-')), true);
});

test('React Flow adapter supports zoom-based graph detail without mutating projection', () => {
  const snapshot = graphSnapshotFixture();
  const projected = projectGraphView(snapshot, { connectedness: 100 });
  const beforeAdapter = JSON.stringify(projected);

  const far = toReactFlowGraph(projected, null, [], [], undefined, { detailLevel: 'far' });
  const mid = toReactFlowGraph(projected, null, [], [], undefined, { detailLevel: 'mid' });
  const near = toReactFlowGraph(projected, 'node_graph_workspace', [], [], undefined, { detailLevel: 'near' });

  assert.equal(JSON.stringify(projected), beforeAdapter);
  assert.equal(graphDetailLevelForZoom(0.4), 'far');
  assert.equal(graphDetailLevelForZoom(0.85), 'mid');
  assert.equal(graphDetailLevelForZoom(1.2), 'near');
  assert.equal(far.nodes.every((node) => node.data.detailLevel === 'far'), true);
  assert.equal(mid.nodes.every((node) => node.data.detailLevel === 'mid'), true);
  assert.equal(near.nodes.every((node) => node.data.detailLevel === 'near'), true);
  assert.equal(far.edges.every((edge) => edge.markerEnd === undefined), true);
  assert.equal(mid.edges.every((edge) => edge.markerEnd !== undefined), true);
  assert.equal(near.edges.every((edge) => edge.className?.includes('somaEdge--near')), true);
  assert.equal(near.edges.every((edge) => edge.pathOptions?.curvature === 0.28), true);
  assert.equal(near.nodes.find((node) => node.id === 'node_graph_workspace')?.selected, true);
  assert.equal(far.nodes.some((node) => node.data.prominent), true);
});

test('React Flow adapter marks selected neighborhoods without mutating graph truth', () => {
  const snapshot = graphSnapshotFixture();
  const selectedNodeId = 'node_edge_bridge_text';
  const projected = projectGraphView(snapshot, { connectedness: 100 });
  const beforeAdapter = JSON.stringify(projected);
  const flowGraph = toReactFlowGraph(projected, selectedNodeId, [], [], undefined, { detailLevel: 'near' });
  const connectedEdges = flowGraph.edges.filter((edge) => edge.data?.selectionRole === 'connected');
  const dimmedEdges = flowGraph.edges.filter((edge) => edge.data?.selectionRole === 'dimmed');
  const neighborIds = new Set(
    connectedEdges.flatMap((edge) => [edge.source, edge.target].filter((nodeId) => nodeId !== selectedNodeId))
  );

  assert.equal(JSON.stringify(projected), beforeAdapter);
  assert.equal(flowGraph.nodes.find((node) => node.id === selectedNodeId)?.data.selectionRole, 'selected');
  assert.equal(connectedEdges.length > 0, true);
  assert.equal(dimmedEdges.length > 0, true);
  assert.equal(connectedEdges.every((edge) => edge.source === selectedNodeId || edge.target === selectedNodeId), true);
  assert.equal(dimmedEdges.every((edge) => edge.source !== selectedNodeId && edge.target !== selectedNodeId), true);
  assert.equal(
    flowGraph.edges.every((edge) => edge.className?.includes(`somaEdge--${edge.data?.selectionRole}`)),
    true
  );
  assert.equal(flowGraph.nodes.every((node) => {
    if (node.id === selectedNodeId) return node.data.selectionRole === 'selected';
    if (neighborIds.has(node.id)) return node.data.selectionRole === 'neighbor';
    return node.data.selectionRole === 'dimmed';
  }), true);
});

test('search view model supports keyboard navigation and matched text highlights', () => {
  assert.equal(clampSearchIndex(8, 3), 2);
  assert.equal(clampSearchIndex(-2, 3), 0);
  assert.equal(nextSearchIndex(2, 3, 1), 0);
  assert.equal(nextSearchIndex(0, 3, -1), 2);
  assert.equal(nextSearchIndex(0, 0, 1), 0);
  assert.equal(resultCountLabel(1), '1 result');
  assert.equal(resultCountLabel(4), '4 results');
  assert.deepEqual(highlightedTextParts('Graph Chat Context', 'chat'), [
    { text: 'Graph ', match: false },
    { text: 'Chat', match: true },
    { text: ' Context', match: false }
  ]);
  assert.deepEqual(highlightedTextParts('Graph graph', 'graph'), [
    { text: 'Graph', match: true },
    { text: ' ', match: false },
    { text: 'graph', match: true }
  ]);
});

test('AI settings model exposes supported local, API, and agent runtime options', () => {
  const providerIds = aiProviderGroups.flatMap((group) => group.providers.map((provider) => provider.id));
  assert.deepEqual(aiProviderGroups.map((group) => group.id), ['local', 'provider', 'agent']);
  assert.deepEqual(providerIds, [
    'ollama',
    'lm_studio',
    'vllm',
    'local_llm',
    'openrouter',
    'vercel_ai_gateway',
    'openai',
    'claude',
    'gemini',
    'deepseek',
    'zai',
    'moonshot',
    'minimax',
    'mistral',
    'groq',
    'xai',
    'together',
    'fireworks',
    'cerebras',
    'openai_compatible',
    'codex_sdk',
    'claude_code'
  ]);
  assert.ok(BRAIN_PROVIDER_IDS.includes('soma_cloud'));
  assert.ok(!providerIds.includes('soma_cloud'));
  assert.equal(providerById('soma_cloud').id, 'codex_sdk');
  assert.equal(providerById('vercel_ai_gateway').endpointDefault, 'https://ai-gateway.vercel.sh/v1');
  assert.equal(providerById('vercel_ai_gateway').endpointPlaceholder, 'https://ai-gateway.vercel.sh/v1');
  assert.equal(defaultAiSettings().providerId, 'codex_sdk');
  assert.equal(defaultAiSettings().credentialConfigured, false);
  assert.equal(
    sanitizeAiSettings({
      providerId: 'openai',
      model: 'gpt-test',
      endpoint: 42,
      credential: 'sk-secret'
    }).endpoint,
    ''
  );
  assert.equal(Object.hasOwn(sanitizeAiSettings({ credential: 'sk-secret' }), 'credential'), false);
  assert.equal(Object.hasOwn(sanitizeAiSettings({ useJobFolderCompiler: false }), 'useJobFolderCompiler'), false);
  assert.equal(sanitizeAiSettings({
    providerId: 'codex_sdk',
    authProfile: 'work'
  }).authProfile, 'work');
  assert.equal(sanitizeAiSettings({
    providerId: 'claude_code',
    authProfile: 'legacy-profile'
  }).authProfile, '');
  const sanitizedUnsupportedProvider = sanitizeAiSettings({
    providerId: 'soma_cloud',
    model: 'managed-default',
    endpoint: 'https://cloud.soma.invalid',
    authProfile: 'managed',
    credentialConfigured: true
  });
  assert.equal(sanitizedUnsupportedProvider.providerId, 'codex_sdk');
  assert.equal(sanitizedUnsupportedProvider.model, '');
  assert.equal(sanitizedUnsupportedProvider.endpoint, '');
  assert.equal(sanitizedUnsupportedProvider.authProfile, '');
  assert.equal(sanitizedUnsupportedProvider.credentialConfigured, false);
  assert.match(providerById('claude_code').description, /active local login/);
  assert.match(providerById('claude_code').modelPlaceholder, /model alias/);
  assert.match(aiSettingsSummary({
    providerId: 'claude_code',
    model: '',
    endpoint: '',
    authProfile: '',
    credentialConfigured: false,
    updatedAt: null
  }), /Claude Code/);
  assert.deepEqual(activeCredentialCue(defaultAiSettings()), {
    label: 'Auth: local Codex login',
    tone: 'neutral'
  });
  assert.deepEqual(activeCredentialCue({
    ...defaultAiSettings(),
    providerId: 'ollama',
    model: 'llama3.3',
    endpoint: 'http://localhost:11434/v1'
  }), {
    label: 'Credential: none required',
    tone: 'neutral'
  });
  assert.deepEqual(activeCredentialCue({
    ...defaultAiSettings(),
    providerId: 'openrouter',
    model: 'openai/gpt-5.5',
    endpoint: 'https://openrouter.ai/api/v1',
    credentialConfigured: true
  }, 'OpenRouter API key'), {
    label: 'Credential: stored OpenRouter API key',
    tone: 'ready'
  });
  assert.deepEqual(activeCredentialCue({
    ...defaultAiSettings(),
    providerId: 'openrouter',
    model: 'openai/gpt-5.5',
    endpoint: 'https://openrouter.ai/api/v1',
    credentialConfigured: false
  }, 'OpenRouter API key'), {
    label: 'Credential: missing OpenRouter API key',
    tone: 'missing'
  });
  const savedOpenRouter = sanitizeAiSettings({
    providerId: 'openrouter',
    model: 'openai/gpt-5.5',
    endpoint: 'https://openrouter.ai/api/v1',
    credentialConfigured: true
  });
  const switchedToLocal = rememberProviderCredential({
    ...savedOpenRouter,
    providerId: 'ollama',
    model: 'llama3.3',
    endpoint: 'http://localhost:11434/v1',
    credentialConfigured: false
  });
  assert.equal(credentialConfiguredForProvider(switchedToLocal, 'openrouter'), true);
  assert.deepEqual(activeCredentialCue({
    ...switchedToLocal,
    providerId: 'openrouter',
    model: 'openai/gpt-5.5',
    endpoint: 'https://openrouter.ai/api/v1',
    credentialConfigured: credentialConfiguredForProvider(switchedToLocal, 'openrouter')
  }, 'OpenRouter API key'), {
    label: 'Credential: stored OpenRouter API key',
    tone: 'ready'
  });
  assert.equal(
    brainSetupIssue(defaultAiSettings())?.message,
    'Set up Brain first. Open Brain Settings, authorize Codex if needed, then click Enable Codex.'
  );
  assert.equal(brainSetupIssue({
    ...defaultAiSettings(),
    updatedAt: '2026-06-28T00:00:00Z'
  }), null);
  assert.match(brainSetupIssue({
    ...defaultAiSettings(),
    providerId: 'local_llm',
    updatedAt: '2026-06-28T00:00:00Z'
  })?.message ?? '', /model/);
  assert.match(brainSetupIssue({
    ...defaultAiSettings(),
    providerId: 'openai',
    updatedAt: '2026-06-28T00:00:00Z'
  })?.message ?? '', /model/);
  assert.match(brainSetupIssue({
    ...defaultAiSettings(),
    providerId: 'openai',
    model: 'gpt-5.5',
    endpoint: 'https://api.openai.com/v1',
    updatedAt: '2026-06-28T00:00:00Z'
  })?.message ?? '', /stored key/);
});

test('AI settings session keeps active readiness and provider credentials while a draft changes', () => {
  const priorDraft = {
    ...defaultAiSettings(),
    providerId: 'ollama',
    model: 'llama3.3',
    endpoint: 'http://localhost:11434/v1',
    credentialConfiguredByProvider: {
      openrouter: true
    }
  };
  const active = mergePersistedAiSettings({
    providerId: 'openai',
    model: 'gpt-5.5',
    endpoint: 'https://api.openai.com/v1',
    authProfile: '',
    credentialConfigured: true,
    updatedAt: '2026-07-25T00:00:00Z'
  }, priorDraft);
  const editedDraft = {
    ...active,
    model: '',
    updatedAt: null
  };

  assert.equal(brainSetupIssue(active), null);
  assert.match(brainSetupIssue(editedDraft)?.message ?? '', /model/);
  assert.equal(credentialConfiguredForProvider(active, 'openrouter'), true);
  assert.equal(credentialConfiguredForProvider(active, 'openai'), true);
});

test('AI settings selector includes registered, live, and saved model options', () => {
  const openRouterModels = modelOptionsForSettings({
    ...defaultAiSettings(),
    providerId: 'openrouter',
    model: '',
    endpoint: 'https://openrouter.ai/api/v1'
  }).map((model) => model.id);
  assert.ok(openRouterModels.includes('zhipuai/glm-5.2'));
  assert.ok(openRouterModels.includes('moonshotai/kimi-k2.6'));
  assert.ok(openRouterModels.includes('minimax/MiniMax-M3'));
  assert.ok(openRouterModels.includes('meta/llama-4-maverick'));
  assert.ok(openRouterModels.includes('google/gemma-4-31b-it'));

  const liveModels = modelOptionsForSettings({
    ...defaultAiSettings(),
    providerId: 'ollama',
    model: 'custom-local-model',
    endpoint: 'http://localhost:11434/v1'
  }, ['llama3.3', 'gemma4']);
  assert.deepEqual(liveModels.slice(0, 2).map((model) => model.id), ['gemma4', 'llama3.3']);
  assert.ok(liveModels.some((model) => model.id === 'custom-local-model'));
});

test('AI settings model search ranks readable matches without exact IDs', () => {
  const models = [
    { id: 'zhipuai/glm-5.2', label: 'GLM-5.2', note: 'Z.AI' },
    { id: 'moonshotai/kimi-k2.6', label: 'Kimi K2.6', note: 'Moonshot' },
    { id: 'google/gemma-4-31b-it', label: 'Gemma 4 31B', note: 'Google' },
    { id: 'deepseek/deepseek-chat', label: 'DeepSeek Chat', note: 'DeepSeek' }
  ];

  assert.deepEqual(filterModelOptions(models, 'glm').map((model) => model.id), ['zhipuai/glm-5.2']);
  assert.deepEqual(filterModelOptions(models, 'kimi').map((model) => model.id), ['moonshotai/kimi-k2.6']);
  assert.deepEqual(filterModelOptions(models, 'gemma 31b').map((model) => model.id), ['google/gemma-4-31b-it']);
  assert.deepEqual(filterModelOptions(models, 'moonshot').map((model) => model.id), ['moonshotai/kimi-k2.6']);
});

test('brain model list contract accepts live selector rows without constraining model ids', () => {
  const parsed = brainModelListResultSchema.parse({
    providerId: 'openrouter',
    status: 'ready',
    message: 'Loaded 3 models.',
    models: ['z-ai/glm-5.2', 'moonshotai/kimi-k2.6', 'google/gemma-4']
  });

  assert.deepEqual(parsed.models, ['z-ai/glm-5.2', 'moonshotai/kimi-k2.6', 'google/gemma-4']);
});

test('list jobs contract accepts pre-runtime compile rows with null runtime fields', () => {
  const parsed = listJobsResultSchema.parse({
    jobs: [{
      jobId: 'job_pre_runtime',
      jobDir: 'Soma Workspace/jobs/job_pre_runtime',
      jobKind: 'graph_extraction',
      createdAt: '2026-06-28T00:00:00Z',
      schemaVersion: 1,
      chunkCount: 32,
      sourceCount: 1,
      sourceMessageId: null,
      sourceNodeId: null,
      files: {
        metadata: 'metadata.json',
        runtime: 'runtime.json',
        runtimeResult: 'runtime_result.json',
        outputPatch: 'output_patch.json'
      },
      metadataExists: true,
      outputPatchExists: true,
      outputPatchStatus: 'empty',
      outputPatchProposalCount: 0,
      outputPatchImportable: false,
      runtimeStatus: null,
      runtimeMessage: null,
      runtimeAdapterKind: null,
      runtimeRanAt: null
    }]
  });

  assert.equal(parsed.jobs.length, 1);
  assert.equal(parsed.jobs[0].runtimeStatus, null);
  assert.equal(parsed.jobs[0].runtimeMessage, null);
});

test('compile graph workspace contract carries review-ready import result', () => {
  const parsed = compileGraphWorkspaceResultSchema.parse({
    status: 'review_ready',
    message: 'Review Updates has 2 new proposals.',
    proposalCount: 2,
    job: {
      jobId: 'job_compile',
      jobDir: 'jobs/job_compile',
      jobKind: 'graph_extraction',
      createdAt: '2026-06-28T00:00:00Z',
      schemaVersion: 1,
      chunkCount: 4,
      sourceCount: 1,
      sourceMessageId: null,
      sourceNodeId: null,
      files: {
        metadata: 'metadata.json',
        runtime: 'runtime.json',
        runtimeResult: 'runtime_result.json',
        outputPatch: 'output_patch.json'
      },
      metadataExists: true,
      outputPatchExists: true,
      outputPatchStatus: 'ready',
      outputPatchProposalCount: 2,
      outputPatchImportable: false,
      importedProposalCount: 2,
      acceptedProposalCount: 0,
      runtimeStatus: 'completed',
      runtimeMessage: 'Runtime command completed.',
      runtimeAdapterKind: 'codex_sdk_profile',
      runtimeRanAt: '2026-06-28T00:01:00Z'
    },
    createdJob: {
      jobId: 'job_compile',
      jobDir: 'jobs/job_compile',
      files: {
        metadata: 'metadata.json',
        instructions: 'instructions.md',
        runtime: 'runtime.json',
        chunks: 'chunks.json',
        currentGraphSnapshot: 'current_graph_snapshot.json',
        graphPatchSchema: 'graph_patch.schema.json',
        outputPatch: 'output_patch.json'
      },
      chunkCount: 4,
      includedChunkCount: 4,
      totalChunkCount: 4,
      truncated: false
    },
    run: {
      jobId: 'job_compile',
      jobDir: 'jobs/job_compile',
      adapterKind: 'codex_sdk_profile',
      status: 'completed',
      failureKind: null,
      message: 'Runtime command completed.',
      outputPatchStatus: 'ready',
      outputPatchProposalCount: 2,
      outputPatchImportable: true
    },
    importResult: {
      jobId: 'job_compile',
      valid: true,
      imported: true,
      trusted: false,
      proposalCount: 2,
      proposals: [{ id: 'proposal_1' }, { id: 'proposal_2' }],
      errors: [],
      warnings: []
    }
  });

  assert.equal(parsed.status, 'review_ready');
  assert.equal(parsed.importResult.imported, true);
  assert.equal(parsed.job.outputPatchImportable, false);
  assert.equal(parsed.job.importedProposalCount, 2);
});

test('direct chat turn contracts carry assistant answers and review imports', () => {
  const createdAt = '2026-06-28T00:00:00Z';
  const graphContext = {
    mode: 'graph_chat',
    user_message: 'How should graph chat behave?',
    reading_context: {
      kind: 'pdf',
      document_name: 'paper.pdf',
      page_number: 2,
      page_count: 8,
      page_text: 'Visible page text.',
      selected_text: null,
      selection_page_number: null
    },
    focus_node_ids: [],
    focus_set_node_bodies: [],
    top_matching_nodes: [{ id: 'node_direct_chat', title: 'Direct Chat', type: 'concept' }],
    top_matching_node_bodies: [],
    relevant_path_fragments: [],
    unresolved_questions: [],
    open_tasks: [],
    recent_graph_thread_messages: [],
    source_evidence_excerpts: [],
    used_graph_areas: [{ id: 'node_direct_chat', title: 'Direct Chat', type: 'concept' }]
  };
  const graphParsed = graphChatTurnResultSchema.parse({
    user_message_id: 'graph_user_1',
    user_message: {
      id: 'graph_user_1',
      role: 'user',
      content: graphContext.user_message,
      created_at: createdAt
    },
    assistant_message: {
      id: 'graph_assistant_1',
      role: 'assistant',
      content: 'Graph chat answers first, then imports reviewable updates.',
      created_at: createdAt,
      context_packet: graphContext
    },
    context_packet: graphContext,
    used_graph_areas: graphContext.used_graph_areas,
    proposal_count: 1,
    patch_import_status: 'imported_to_review',
    patch_import_result: {
      messageId: 'graph_assistant_1',
      patchId: 'patch_direct_chat',
      valid: true,
      imported: true,
      trusted: false,
      proposal_status: 'draft',
      proposalCount: 1,
      proposals: [{ id: 'proposal_direct_chat' }],
      errors: [],
      warnings: []
    },
    runtime_status: 'completed',
    runtime_adapter_kind: 'local_offline_endpoint',
    runtime_failure_kind: null,
    runtime_message: 'Chat runtime returned an assistant answer.'
  });

  assert.equal(graphParsed.assistant_message?.id, 'graph_assistant_1');
  assert.equal(graphParsed.patch_import_status, 'imported_to_review');
  assert.equal(graphParsed.patch_import_result.trusted, false);
  assert.equal(graphParsed.context_packet.reading_context?.selected_text, undefined);
  assert.equal(graphParsed.context_packet.reading_context?.selection_page_number, undefined);

  const nodeContext = {
    mode: 'node_chat',
    focused_node_id: 'node_direct_chat',
    user_message: 'Add this to the selected node.',
    focused_node_body: {
      id: 'node_direct_chat',
      title: 'Direct Chat',
      type: 'concept',
      compiled_body: 'Direct chat has a focused node body.',
      body_version: 1,
      source_chunk_ids: ['chunk_1']
    },
    neighbor_bodies: [],
    bridge_texts: [],
    node_thread_recent_messages: [],
    source_evidence_excerpts: []
  };
  const nodeParsed = nodeChatTurnResultSchema.parse({
    user_message_id: 'node_user_1',
    user_message: {
      id: 'node_user_1',
      node_id: 'node_direct_chat',
      role: 'user',
      content: nodeContext.user_message,
      created_at: createdAt,
      context_packet: nodeContext
    },
    assistant_message: {
      id: 'node_assistant_1',
      node_id: 'node_direct_chat',
      role: 'assistant',
      content: 'The answer remains visible even if proposed body updates fail validation.',
      created_at: createdAt,
      context_packet: nodeContext
    },
    context_packet: nodeContext,
    used_graph_areas: [],
    proposal_count: 0,
    patch_import_status: 'invalid',
    patch_import_result: {
      messageId: 'node_assistant_1',
      valid: false,
      imported: false,
      trusted: false,
      proposalCount: 0,
      proposals: [],
      errors: [{ path: '$.proposed_node_body_updates[0]', message: 'source_chunk_ids is required.' }],
      warnings: []
    },
    runtime_status: 'completed',
    runtime_adapter_kind: 'codex_sdk_profile',
    runtime_failure_kind: null,
    runtime_message: 'Chat runtime returned an assistant answer.',
    error: 'Graph updates need regeneration; the assistant answer was kept.'
  });

  assert.equal(nodeParsed.assistant_message?.node_id, 'node_direct_chat');
  assert.equal(nodeParsed.patch_import_status, 'invalid');
  assert.match(nodeParsed.error ?? '', /answer was kept/);
});

test('compile failure messages give exact runtime fixes', () => {
  assert.match(compileFailureMessage({
    jobId: 'job_typed_local',
    jobDir: 'jobs/job_typed_local',
    adapterKind: 'local_offline_endpoint',
    status: 'failed',
    failureKind: 'unavailable',
    message: 'Provider transport changed its prose.',
    outputPatchStatus: 'empty',
    outputPatchProposalCount: 0,
    outputPatchImportable: false
  }), /Start the OpenAI-compatible endpoint or update Brain Settings/);

  assert.match(compileFailureMessage({
    jobId: 'job_api',
    jobDir: 'jobs/job_api',
    adapterKind: 'api_provider',
    status: 'unsupported',
    message: 'Hosted API adapters are planned later.',
    outputPatchStatus: 'empty',
    outputPatchProposalCount: 0,
    outputPatchImportable: false
  }), /Choose Codex, Claude Code, or Local LLM/);

  assert.match(compileFailureMessage({
    jobId: 'job_local',
    jobDir: 'jobs/job_local',
    adapterKind: 'local_offline_endpoint',
    status: 'failed',
    message: 'Local runtime needs an HTTP endpoint.',
    outputPatchStatus: 'empty',
    outputPatchProposalCount: 0,
    outputPatchImportable: false
  }), /Start the OpenAI-compatible endpoint or update Brain Settings/);

  assert.match(compileFailureMessage({
    jobId: 'job_codex',
    jobDir: 'jobs/job_codex',
    adapterKind: 'codex_sdk_profile',
    status: 'failed',
    message: 'Could not start runtime command `codex`: not found',
    outputPatchStatus: 'empty',
    outputPatchProposalCount: 0,
    outputPatchImportable: false
  }), /enable Codex/);

  assert.match(compileFailureMessage({
    jobId: 'job_claude',
    jobDir: 'jobs/job_claude',
    adapterKind: 'claude_code_profile',
    status: 'failed',
    message: 'Could not start runtime command `claude`: not found',
    outputPatchStatus: 'empty',
    outputPatchProposalCount: 0,
    outputPatchImportable: false
  }), /Install Claude Code or set SOMA_CLAUDE_COMMAND/);

  assert.match(compileFailureMessage({
    jobId: 'job_invalid',
    jobDir: 'jobs/job_invalid',
    adapterKind: 'claude_code_profile',
    status: 'completed',
    message: 'Runtime command completed.',
    outputPatchStatus: 'invalid',
    outputPatchProposalCount: 0,
    outputPatchImportable: false
  }), /malformed updates/);
});

test('chat and compile copy avoids backend jargon in normal UI states', () => {
  assert.equal(reviewReadyNotice(1), '1 update ready to review.');
  assert.equal(reviewReadyNotice(3), '3 updates ready to review.');
  assert.equal(chatUpdateNotice('accepted_to_graph', 1), '1 update saved to graph.');
  assert.equal(chatUpdateNotice('accepted_to_graph', 3), '3 updates saved to graph.');
  assert.equal(chatUpdateNotice('imported_to_review', 2), '2 updates ready to review.');
  assert.equal(chatUpdateNotice('none', 0), null);
  assert.equal(
    chatTurnErrorMessage('Provider wording changed.', 'credential', 'api_provider'),
    'API brain is not reachable or needs a valid key. Check Brain Settings.'
  );
  assert.equal(
    chatTurnErrorMessage('missing credential, but the typed kind is authoritative', 'invalid_response', 'api_provider'),
    'The brain answered in a format Soma could not read. Try again or choose another brain.'
  );
  assert.equal(
    chatTurnErrorMessage('missing credential `api_key:openrouter/default`'),
    'API brain is not reachable or needs a valid key. Check Brain Settings.'
  );
  assert.equal(
    chatTurnErrorMessage('Runtime command exited with status 1. Error: database is locked'),
    STORAGE_BUSY_MESSAGE
  );
  assert.equal(
    chatTurnErrorMessage(
      'Chat runtime answered as Codex instead of answering the current user message. No graph updates were imported.'
    ),
    'Codex returned its identity instead of a Soma answer. Try again in a moment.'
  );
  assert.equal(
    chatTurnErrorMessage('Codex runtime could not start. Details: Codex profile storage is busy.'),
    'Codex is busy with another run. Try again in a moment.'
  );
  assert.equal(
    compileFailureMessage({
      jobId: 'job_codex_locked',
      jobDir: 'jobs/job_codex_locked',
      adapterKind: 'codex_sdk_profile',
      status: 'failed',
      message: 'Error: database is locked',
      outputPatchStatus: 'empty',
      outputPatchProposalCount: 0,
      outputPatchImportable: false
    }),
    STORAGE_BUSY_MESSAGE
  );
  assert.equal(
    chatTurnErrorMessage('Graph updates need regeneration; the assistant answer was kept.'),
    'Answer saved. Suggested updates need to be regenerated.'
  );
  assert.equal(
    formatError(new Error(
      'send_graph_chat_turn result failed contract validation: '
        + 'context_packet.reading_context.selected_text: Invalid input'
    )),
    'Soma could not read the latest response. Try again.'
  );
  assert.equal(
    formatError('Accepting warning proposals is not implemented.'),
    'That output only contained compiler notices. No graph update needs review.'
  );
  assert.equal(
    formatError(new Error('Error: database is locked')),
    STORAGE_BUSY_MESSAGE
  );
  assert.equal(
    formatError({ message: 'SQLITE_LOCKED: database schema is locked' }),
    STORAGE_BUSY_MESSAGE
  );
  assert.equal(
    formatError('SQLite write lock was poisoned.'),
    STORAGE_BUSY_MESSAGE
  );
  assert.equal(isStorageBusyMessage(STORAGE_BUSY_MESSAGE), true);
  assert.equal(isStorageBusyMessage('SQLite write lock was poisoned.'), true);
  assert.equal(
    chatTurnErrorMessage(formatError(new Error('Error: database is locked'))),
    STORAGE_BUSY_MESSAGE
  );
  assert.equal(
    chatTurnErrorMessage('Runtime command exited with SQLITE_BUSY'),
    STORAGE_BUSY_MESSAGE
  );
  assert.equal(
    jobRunFailureMessage(jobRunFixture({
      runtimeStatus: 'failed',
      runtimeAdapterKind: 'codex_sdk_profile',
      runtimeMessage: 'Error: database is locked'
    })),
    STORAGE_BUSY_MESSAGE
  );
  assert.equal(
    displayChatMessageContent({
      id: 'assistant_locked',
      role: 'assistant',
      content: 'Runtime command exited with status 1. Error: database is locked',
      created_at: '2026-07-06T00:00:00.000Z'
    }),
    STORAGE_BUSY_MESSAGE
  );
  assert.equal(
    displayChatMessageContent({
      id: 'assistant_locked_long',
      role: 'assistant',
      content: `Runtime failed while preparing the answer. ${
        'Details repeated. '.repeat(20)
      }SQLITE_LOCKED: database schema is locked`,
      created_at: '2026-07-06T00:00:00.000Z'
    }),
    STORAGE_BUSY_MESSAGE
  );
  assert.equal(
    displayChatMessageContent({
      id: 'user_locked',
      role: 'user',
      content: 'Why did it say database is locked?',
      created_at: '2026-07-06T00:00:00.000Z'
    }),
    'Why did it say database is locked?'
  );
  assert.equal(
    displayGraphChatError('Runtime failed before answer: Error: database is locked'),
    STORAGE_BUSY_MESSAGE
  );

  const readModel = reviewQueueFixture([
    reviewItemFixture({
      id: 'proposal_chat_body',
      patch_id: 'patch_chat_body',
      source_message_id: 'assistant_1',
      type: 'node_body_update',
      status: 'proposed',
      title: 'Append node section',
      target: 'Graph workspace',
      reason: 'Useful clarification.',
      related_node_ids: ['node_graph_workspace'],
      evidence_count: 1,
      evidence_refs: [{ type: 'chunk', id: 'chunk_1' }],
      risk_markers: [],
      source: graphMessageSource('assistant_1'),
      created_at: '2026-06-28T00:00:00.000Z'
    })
  ]);

  assert.deepEqual(chatUpdateSummaryForMessage(readModel, 'assistant_1'), {
    label: '1 update ready',
    tone: 'ready',
    visible: true
  });
  assert.equal(proposalTypeLabel('node_body_update'), 'Node body');
  assert.equal(proposalTypeLabel('message_evidence_attachment'), 'Evidence');

  const acceptedReadModel = reviewQueueFixture(
    [reviewItemFixture({
      id: 'proposal_chat_accepted',
      patch_id: 'patch_chat_accepted',
      source_message_id: 'assistant_accepted',
      type: 'node',
      status: 'accepted',
      temp_id: 'new_node',
      title: 'Accepted node',
      target: 'new_node',
      evidence_count: 1,
      evidence_refs: [{ type: 'chunk', id: 'chunk_1' }],
      risk_markers: [],
      source: graphMessageSource('assistant_accepted'),
      created_at: '2026-07-24T00:00:00.000Z'
    })],
    {
      patch_id: 'patch_chat_accepted',
      source: 'graph_thread_message',
      source_message_id: 'assistant_accepted',
      change_count: 1
    }
  );

  assert.deepEqual(latestUndoableGraphPatch(acceptedReadModel), {
    messageId: 'assistant_accepted',
    patchId: 'patch_chat_accepted'
  });
  assert.equal(graphReviewQueueReadModelSchema.safeParse(acceptedReadModel).success, true);
  assert.equal(latestUndoableGraphPatch({
    ...acceptedReadModel,
    latest_undoable_patch: null
  }), null);
});

test('graph chat history merge keeps current and optimistic messages by id', () => {
  const loadedMessages = [
    {
      id: 'shared_assistant',
      role: 'assistant',
      content: 'Stale persisted copy.',
      created_at: '2026-07-25T00:00:00.000Z'
    },
    {
      id: 'loaded_user',
      role: 'user',
      content: 'Loaded history.',
      created_at: '2026-07-25T00:01:00.000Z'
    }
  ];
  const currentMessages = [
    {
      id: 'shared_assistant',
      role: 'assistant',
      content: 'Current enriched copy.',
      created_at: '2026-07-25T00:00:00.000Z'
    },
    {
      id: 'pending_user',
      role: 'user',
      content: 'New question.',
      created_at: '2026-07-25T00:02:00.000Z'
    },
    {
      id: 'pending_assistant',
      role: 'assistant',
      content: 'Thinking',
      created_at: '2026-07-25T00:02:00.000Z'
    }
  ];

  const merged = mergeMessagesById(loadedMessages, currentMessages);

  assert.deepEqual(merged.map((message) => message.id), [
    'loaded_user',
    'shared_assistant',
    'pending_user',
    'pending_assistant'
  ]);
  assert.equal(merged.find((message) => message.id === 'shared_assistant')?.content, 'Current enriched copy.');
});

test('workspace request ownership rejects stale completions across A to B to A switches', () => {
  const workspaceAFirst = initialWorkspaceRequestOwner('workspace-a');
  const unchangedWorkspaceA = activateWorkspaceRequestOwner(workspaceAFirst, 'workspace-a');
  const workspaceB = activateWorkspaceRequestOwner(workspaceAFirst, 'workspace-b');
  const workspaceASecond = activateWorkspaceRequestOwner(workspaceB, 'workspace-a');

  assert.equal(unchangedWorkspaceA, workspaceAFirst);
  assert.equal(workspaceB.generation, 1);
  assert.equal(workspaceASecond.generation, 2);
  assert.equal(ownsWorkspaceRequest(workspaceB, workspaceAFirst), false);
  assert.equal(ownsWorkspaceRequest(workspaceASecond, workspaceAFirst), false);
  assert.equal(ownsWorkspaceRequest(workspaceASecond, workspaceASecond), true);
});

test('node chat history merge preserves optimistic turns while delayed history loads', () => {
  const loadedMessages = [{
    id: 'persisted_user',
    node_id: 'node-a',
    role: 'user',
    content: 'Earlier question',
    created_at: '2026-07-25T00:00:00.000Z'
  }];
  const pendingMessages = [{
    id: 'pending_user',
    node_id: 'node-a',
    role: 'user',
    content: 'Current question',
    created_at: '2026-07-25T00:01:00.000Z'
  }, {
    id: 'pending_assistant',
    node_id: 'node-a',
    role: 'assistant',
    content: 'Thinking',
    created_at: '2026-07-25T00:01:00.000Z'
  }];

  assert.deepEqual(
    mergeMessagesById(loadedMessages, pendingMessages).map((message) => message.id),
    ['persisted_user', 'pending_user', 'pending_assistant']
  );
});

test('node chat exposes only the backend undo record owned by its messages', () => {
  const readModel = reviewQueueFixture([], {
    patch_id: 'node-patch',
    source: 'node_thread_message',
    source_message_id: 'node-assistant',
    change_count: 1
  });
  const messages = [{
    id: 'node-assistant',
    node_id: 'node-a',
    role: 'assistant',
    content: 'Captured answer',
    created_at: '2026-07-25T00:00:00.000Z'
  }];

  assert.deepEqual(latestUndoableNodePatch(readModel, messages), {
    messageId: 'node-assistant',
    patchId: 'node-patch'
  });
  assert.equal(latestUndoableNodePatch(readModel, [{ ...messages[0], id: 'other-message' }]), null);
  assert.equal(latestUndoableNodePatch({
    ...readModel,
    latest_undoable_patch: {
      ...readModel.latest_undoable_patch,
      source: 'graph_thread_message'
    }
  }, messages), null);
  assert.equal(graphReviewQueueReadModelSchema.safeParse(readModel).success, true);
});

test('chat completion deduplicates server messages that arrived with delayed history', () => {
  const realUser = {
    id: 'real_user',
    node_id: 'node-a',
    role: 'user',
    content: 'Current question',
    created_at: '2026-07-25T00:01:00.000Z'
  };
  const realAssistant = {
    id: 'real_assistant',
    node_id: 'node-a',
    role: 'assistant',
    content: 'Current answer',
    created_at: '2026-07-25T00:01:01.000Z'
  };
  const pending = [{
    ...realUser,
    id: 'pending_user'
  }, {
    ...realAssistant,
    id: 'pending_assistant',
    content: 'Thinking'
  }];
  const historyWithPending = mergeMessagesById([realUser], pending);
  const settledNode = settleMessagesById(
    historyWithPending,
    ['pending_user', 'pending_assistant'],
    [realUser, realAssistant]
  );
  const settledGraph = settleMessagesById(
    historyWithPending,
    ['pending_user', 'pending_assistant'],
    [realUser, realAssistant]
  );

  assert.deepEqual(settledNode.map((message) => message.id), ['real_user', 'real_assistant']);
  assert.deepEqual(settledGraph.map((message) => message.id), ['real_user', 'real_assistant']);
});

test('compile panel keeps ready updates usable while another compile runs', () => {
  const readyJob = jobRunFixture({
    jobId: 'job_ready',
    outputPatchStatus: 'ready',
    outputPatchProposalCount: 5,
    outputPatchImportable: true
  });
  const waitingJob = jobRunFixture({ jobId: 'job_waiting' });
  const runningJob = jobRunFixture({ jobId: 'job_running' });
  const importedJob = jobRunFixture({
    jobId: 'job_imported',
    outputPatchStatus: 'ready',
    outputPatchProposalCount: 5,
    outputPatchImportable: false,
    importedProposalCount: 5
  });

  assert.deepEqual(jobRunPrimaryActionState(readyJob, { busyJobId: 'job_running' }), {
    kind: 'import_patch',
    label: 'Review Updates',
    disabled: false
  });
  assert.deepEqual(jobRunPrimaryActionState(waitingJob, { busyJobId: 'job_running' }), {
    kind: 'run_compile',
    label: 'Compile Graph',
    disabled: true,
    disabledReason: 'Another compile is running.'
  });
  assert.deepEqual(jobRunPrimaryActionState(runningJob, { busyJobId: 'job_running' }), {
    kind: 'none',
    label: 'Compiling',
    disabled: true,
    disabledReason: 'This compile is running.'
  });
  assert.deepEqual(jobRunPrimaryActionState(importedJob), {
    kind: 'open_review',
    label: 'Review Updates',
    disabled: false
  });
  assert.equal(primaryJobRun([waitingJob, importedJob, readyJob], null)?.jobId, 'job_ready');
  assert.deepEqual(compactJobStatusItems({
    waiting_for_compiler: 1,
    proposals_ready: 0,
    imported_to_review: 1,
    accepted_to_graph: 0,
    failed: 0,
    running: 0
  }), [
    { id: 'review', label: 'In review', value: 1 },
    { id: 'compile', label: 'To compile', value: 1 }
  ]);
  assert.equal(
    jobRunsPanelNotice('Compile started.', { busyJobId: 'job_running', readyCount: 2, runningCount: 1 }),
    'Compile running. Review-ready updates stay available.'
  );
  assert.equal(jobRunsPanelNotice('1 update ready to review.', { readyCount: 1, runningCount: 0 }), null);
});

test('dragged node positions are layout state, not canonical graph truth', () => {
  const snapshot = graphSnapshotFixture();
  const nodeId = 'node_graph_workspace';
  const layoutNode = layoutNodeFromPosition(nodeId, { x: 240, y: 330 }, true);
  const beforeProjection = JSON.stringify(snapshot);

  const projected = projectGraphView(snapshot, {
    connectedness: 100,
    pinnedNodeIds: [nodeId],
    layoutOverrides: {
      [nodeId]: layoutNode
    }
  });
  const projectedNode = projected.nodes.find((node) => node.id === nodeId);
  const canonicalNode = snapshot.nodes.find((node) => node.id === nodeId);

  assert.equal(JSON.stringify(snapshot), beforeProjection);
  assert.equal(projectedNode?.layout.x, layoutNode.x);
  assert.equal(projectedNode?.layout.y, layoutNode.y);
  assert.equal(projectedNode?.layout.pinned, true);
  assert.equal(canonicalNode?.layout, undefined);
  assert.equal(snapshot.edges.length, 5);
});

test('graph chat view model consumes the server review queue contract', () => {
  const message = {
    id: 'local_graph_message_test',
    role: 'user',
    content: 'Graph chat should enrich compiled sections.',
    created_at: '2026-06-27T00:00:00.000Z'
  };
  const draftItem = reviewItemFixture({
    id: 'proposal_graph_message_body',
    patch_id: 'patch_graph_message_body',
    source_message_id: message.id,
    type: 'node_body_update',
    status: 'draft',
    title: 'Append node section',
    target: 'Graph workspace',
    reason: 'Graph chat answer may enrich this node.',
    related_node_ids: ['node_graph_workspace'],
    evidence_count: 1,
    evidence_refs: [{ type: 'message', id: message.id }],
    risk_markers: ['message_backed'],
    source: graphMessageSource(message.id),
    created_at: message.created_at
  });
  const readModel = reviewQueueFixture([draftItem]);
  const draft = readModel.groups.draft.items[0];

  assert.equal(graphReviewQueueReadModelSchema.safeParse(readModel).success, true);
  assert.equal(readModel.groups.draft.count, 1);
  assert.equal(draft.type, 'node_body_update');
  assert.equal(draft.source.source_message_id, message.id);
  assert.equal(draft.evidence_count, 1);
  assert.equal(draft.risk_markers.includes('message_backed'), true);
  assert.equal(Object.hasOwn(draft, 'payload'), false);
  assert.equal(JSON.stringify(readModel).includes(message.content), false);

  const deferred = reviewQueueFixture([{
    ...draftItem,
    status: 'deferred',
    decided_at: '2026-06-27T00:05:00.000Z',
    decision_reason: 'not now'
  }]);
  assert.equal(deferred.groups.draft.count, 0);
  assert.equal(deferred.groups.deferred.count, 1);
  assert.equal(deferred.groups.deferred.items[0].title, 'Append node section');
  assert.equal(deferred.groups.deferred.items[0].source.kind, 'graph_message');
  assert.equal(deferred.groups.deferred.items[0].source.source_message_id, message.id);
  assert.equal(deferred.groups.deferred.items[0].decision_reason, 'not now');
});

test('review tray keeps compiler warnings out of reviewable updates', () => {
  const readModel = reviewQueueFixture([
    reviewItemFixture({
      id: 'patch_warning',
      patch_id: 'patch_warning',
      type: 'warning',
      title: 'Patch warning',
      target: 'patch',
      reason: 'Repeated source material was skipped to avoid duplicating graph state.',
      evidence_count: 1,
      evidence_refs: [{ type: 'chunk', id: 'chunk_1' }],
      risk_markers: ['warning'],
      created_at: '2026-06-28T00:00:00.000Z'
    }),
    reviewItemFixture({
      id: 'node_reviewable',
      patch_id: 'patch_warning',
      type: 'node',
      temp_id: 'node_reviewable',
      title: 'Reviewable Node',
      target: 'node_reviewable',
      evidence_count: 1,
      evidence_refs: [{ type: 'chunk', id: 'chunk_2' }],
      risk_markers: [],
      created_at: '2026-06-28T00:00:00.000Z'
    })
  ]);
  const warning = readModel.items.find((item) => item.id === 'patch_warning');
  const reviewableNode = readModel.items.find((item) => item.id === 'node_reviewable');

  assert.ok(warning);
  assert.ok(reviewableNode);
  assert.equal(appPendingReviewCount(readModel), 1);
  assert.equal(reviewFilterCount(readModel, 'needs_review'), 1);
  assert.deepEqual(reviewNoticeItems(readModel).map((item) => item.id), ['patch_warning']);
  assert.equal(
    noticeText(reviewNoticeItems(readModel)),
    'No new update was created; the source already looks covered.'
  );
  assert.deepEqual(visibleReviewItems(readModel, 'needs_review').map((item) => item.id), ['node_reviewable']);
  assert.deepEqual(reviewItemActions(warning), []);
  assert.deepEqual(reviewItemActions(reviewableNode), ['accept', 'reject', 'defer']);
});

test('review tray never offers unsupported Accept actions', () => {
  for (const type of ['path', 'ambiguity', 'merge_candidate']) {
    const item = reviewItemFixture({
      id: `proposal_${type}`,
      type,
      status: 'proposed',
      title: `Review ${type}`,
      risk_markers: type === 'ambiguity' ? ['ambiguity'] : ['new_graph_object']
    });
    assert.deepEqual(reviewItemActions(item), ['reject', 'defer']);
    assert.deepEqual(reviewItemActions({ ...item, status: 'deferred' }), ['reject']);
  }
});

test('review queue boundary preserves exact mutation payloads and rejects payload expansion', () => {
  const exactBody = '\nExact compiled body with deliberate whitespace.\n';
  const readModel = reviewQueueFixture([
    reviewItemFixture({
      id: 'proposal_exact_body',
      type: 'node',
      mutation_payload: { compiled_body: exactBody }
    })
  ]);

  const parsed = graphReviewQueueReadModelSchema.safeParse(readModel);
  assert.equal(parsed.success, true);
  assert.equal(parsed.data.items[0].mutation_payload.compiled_body, exactBody);

  const expanded = structuredClone(readModel);
  expanded.items[0].mutation_payload.raw_payload = 'must not cross the boundary';
  assert.equal(graphReviewQueueReadModelSchema.safeParse(expanded).success, false);
});

test('review tray labels and preserves exact mutation text for informed review', () => {
  for (const [item, expected] of [
    [
      reviewItemFixture({
        type: 'node',
        mutation_payload: { compiled_body: 'Exact new node body.' }
      }),
      { label: 'Proposed node body', text: 'Exact new node body.' }
    ],
    [
      reviewItemFixture({
        type: 'node_body_update',
        mutation_payload: { section_text: 'Exact section to append.' }
      }),
      { label: 'Section to append', text: 'Exact section to append.' }
    ],
    [
      reviewItemFixture({
        type: 'node_body_update',
        mutation_payload: { compiled_body: 'Exact replacement body.' }
      }),
      { label: 'Replacement body', text: 'Exact replacement body.' }
    ],
    [
      reviewItemFixture({
        type: 'edge',
        mutation_payload: { bridge_text: 'Exact new bridge.' }
      }),
      { label: 'Proposed bridge', text: 'Exact new bridge.' }
    ],
    [
      reviewItemFixture({
        type: 'edge_bridge_update',
        mutation_payload: { bridge_text: 'Exact replacement bridge.' }
      }),
      { label: 'Replacement bridge', text: 'Exact replacement bridge.' }
    ]
  ]) {
    assert.deepEqual(reviewMutationPreview(item), expected);
  }
  assert.equal(reviewMutationPreview(reviewItemFixture()), null);
});

test('graph chat view model links messages to context areas and proposed updates', () => {
  const message = {
    id: 'local_graph_message_context',
    role: 'user',
    content: 'Graph chat should show the areas it used.',
    created_at: '2026-06-27T00:00:00.000Z'
  };
  const packet = graphContextPacketFixture(message.content);
  const readModel = reviewQueueFixture([
    reviewItemFixture({
      id: 'proposal_context_body',
      patch_id: 'patch_graph_message',
      source_message_id: message.id,
      type: 'node_body_update',
      status: 'draft',
      title: 'Append node section',
      target: 'Graph workspace',
      related_node_ids: ['node_graph_workspace'],
      evidence_count: 1,
      evidence_refs: [{ type: 'message', id: message.id }],
      risk_markers: ['message_backed'],
      source: graphMessageSource(message.id),
      created_at: message.created_at
    })
  ]);
  const messageWithPacket = {
    ...message,
    context_packet: packet
  };

  assert.deepEqual(contextAreasForMessage(messageWithPacket), packet.used_graph_areas);
  assert.deepEqual(proposalLinesForMessage(readModel, message.id).map((proposal) => proposal.status), ['draft']);

  const multiStatusModel = reviewQueueFixture([
    reviewItemFixture({
      id: 'proposal_message_evidence',
      patch_id: 'patch_graph_message',
      source_message_id: message.id,
      type: 'message_evidence_attachment',
      status: 'proposed',
      title: 'Attach message evidence',
      target: 'Graph workspace',
      reason: 'Message supports this node.',
      related_node_ids: ['node_graph_workspace'],
      evidence_count: 1,
      evidence_refs: [{ type: 'message', id: message.id }],
      risk_markers: ['message_backed'],
      source: graphMessageSource(message.id),
      created_at: message.created_at
    }),
    reviewItemFixture({
      id: 'proposal_body_update',
      patch_id: 'patch_graph_message',
      source_message_id: message.id,
      type: 'node_body_update',
      status: 'deferred',
      title: 'Append node section',
      target: 'Graph workspace',
      reason: 'Maybe enrich the node body.',
      related_node_ids: ['node_graph_workspace'],
      evidence_count: 1,
      evidence_refs: [{ type: 'message', id: message.id }],
      risk_markers: ['message_backed'],
      source: graphMessageSource(message.id),
      created_at: message.created_at
    }),
    reviewItemFixture({
      id: 'proposal_ambiguity',
      patch_id: 'patch_graph_message',
      source_message_id: message.id,
      type: 'ambiguity',
      status: 'rejected',
      title: 'Review target',
      target: 'Graph',
      reason: 'Maybe this belongs elsewhere.',
      evidence_count: 1,
      evidence_refs: [{ type: 'message', id: message.id }],
      risk_markers: ['ambiguity', 'message_backed'],
      source: graphMessageSource(message.id),
      created_at: message.created_at
    })
  ]);

  assert.deepEqual(
    proposalLinesForMessage(multiStatusModel, message.id).map((proposal) => proposal.status),
    ['proposed', 'deferred', 'rejected']
  );
});

function graphSnapshotFixture() {
  const nodes = [
    ['node_compiled_sections', 'Compiled Conversation Sections', 'concept'],
    ['node_edge_bridge_text', 'Readable Edge Bridges', 'decision'],
    ['node_graph_workspace', 'Graph Workspace Foundation', 'concept'],
    ['node_graph_chat_context', 'Graph Chat Context', 'concept']
  ].map(([id, title, type], index) => ({
    id,
    type,
    title,
    preview: `${title} preview.`,
    compiled_body: `${title} keeps the graph readable and evidence-backed.`,
    status: 'active',
    markers: ['source_backed'],
    source_chunk_ids: [`chunk_${index + 1}`],
    evidence: [],
    body_version: 1,
    update_history: []
  }));

  const edges = [
    ['edge_sections_to_bridges', 'node_compiled_sections', 'node_edge_bridge_text', 'depends_on'],
    ['edge_workspace_to_sections', 'node_graph_workspace', 'node_compiled_sections', 'implements'],
    ['edge_chat_to_workspace', 'node_graph_chat_context', 'node_graph_workspace', 'depends_on'],
    ['edge_chat_to_sections', 'node_graph_chat_context', 'node_compiled_sections', 'mentions'],
    ['edge_workspace_to_bridges', 'node_graph_workspace', 'node_edge_bridge_text', 'supports']
  ].map(([id, sourceNodeId, targetNodeId, type], index) => ({
    id,
    source_node_id: sourceNodeId,
    target_node_id: targetNodeId,
    type,
    bridge_text: `${sourceNodeId} connects to ${targetNodeId}.`,
    status: 'active',
    markers: ['source_backed'],
    source_chunk_ids: [`chunk_edge_${index + 1}`],
    evidence: []
  }));

  return {
    schema_version: 1,
    nodes,
    edges,
    paths: []
  };
}

const REVIEW_GROUPS = {
  draft: 'Draft',
  proposed: 'Needs review',
  deferred: 'Deferred',
  superseded: 'Superseded',
  rejected: 'Rejected'
};

function reviewQueueFixture(items, latestUndoablePatch = null) {
  const countsByStatus = {};
  for (const item of items) {
    countsByStatus[item.status] = (countsByStatus[item.status] ?? 0) + 1;
  }

  return {
    generated_at: '2026-07-24T00:00:00.000Z',
    total_count: items.length,
    counts_by_status: countsByStatus,
    groups: Object.fromEntries(
      Object.entries(REVIEW_GROUPS).map(([status, title]) => {
        const groupedItems = items.filter((item) => item.status === status);
        return [status, { status, title, count: groupedItems.length, items: groupedItems }];
      })
    ),
    items,
    latest_undoable_patch: latestUndoablePatch
  };
}

function reviewItemFixture(overrides = {}) {
  return {
    id: 'proposal_fixture',
    patch_id: 'patch_fixture',
    job_id: null,
    source_message_id: null,
    type: 'node',
    status: 'proposed',
    temp_id: null,
    title: 'Review update',
    target: 'Graph',
    reason: 'Fixture review update.',
    mutation_payload: null,
    related_node_ids: [],
    evidence_count: 0,
    evidence_refs: [],
    risk_markers: ['no_evidence'],
    source: {
      kind: 'patch',
      id: 'patch_fixture',
      source_message_id: null,
      job_id: null,
      label: 'Patch'
    },
    created_at: '2026-07-24T00:00:00.000Z',
    decided_at: null,
    decision_reason: null,
    ...overrides
  };
}

function graphMessageSource(messageId) {
  return {
    kind: 'graph_message',
    id: messageId,
    source_message_id: messageId,
    job_id: null,
    label: 'Graph chat'
  };
}

function graphContextPacketFixture(userMessage) {
  const area = {
    id: 'node_graph_workspace',
    title: 'Graph workspace',
    type: 'concept'
  };
  return {
    mode: 'graph_chat',
    user_message: userMessage,
    focus_node_ids: [],
    focus_set_node_bodies: [],
    top_matching_nodes: [area],
    top_matching_node_bodies: [],
    relevant_path_fragments: [],
    unresolved_questions: [],
    open_tasks: [],
    recent_graph_thread_messages: [],
    source_evidence_excerpts: [],
    used_graph_areas: [area]
  };
}

function jobRunFixture(overrides = {}) {
  return {
    jobId: 'job_fixture',
    jobDir: 'jobs/job_fixture',
    jobKind: 'graph_extraction',
    createdAt: '2026-06-28T00:00:00.000Z',
    schemaVersion: 1,
    chunkCount: 6,
    sourceCount: 1,
    files: {
      metadata: 'metadata.json',
      outputPatch: 'output_patch.json'
    },
    metadataExists: true,
    outputPatchExists: true,
    outputPatchStatus: 'empty',
    outputPatchProposalCount: 0,
    outputPatchImportable: false,
    runtimeStatus: null,
    runtimeMessage: null,
    runtimeAdapterKind: null,
    runtimeRanAt: null,
    ...overrides
  };
}
