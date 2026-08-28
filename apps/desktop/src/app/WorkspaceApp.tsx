import { useCallback, useEffect, lazy, useMemo, useRef, Suspense, useState } from 'react';

import type {
  GraphCanvasNode,
  GraphCanvasSnapshot,
  GraphReviewQueueReadModel,
  JobRun,
  LayoutNode,
  SourceReadingContext,
  WorkspaceState
} from '../../../../packages/contracts/src';

import { createWorkspaceAuto } from '../shared/commands/graphWorkspaceCommands';
import { loadGraphNodeDetail } from '../shared/commands/nodeDetailCommands';
import { layoutNodeFromPosition, pinnedNodeIdsWith, upsertLayoutOverride } from '../shared/data/layoutState';
import { emptyReviewQueue, pendingReviewCount } from '../shared/data/reviewQueue';
import { useGraphChatController } from './controllers/useGraphChatController';
import { useBrainSettingsController } from './controllers/useBrainSettingsController';
import {
  activeBrainEffort as effortForBrain,
  activeBrainLabel as labelForBrain
} from '../features/settings/aiSettingsViewModel';
import { useJobController } from './controllers/useJobController';
import { useGraphReadModelCoordinator } from './controllers/useGraphReadModelCoordinator';
import { useNodeLayoutPersistence } from './controllers/useNodeLayoutPersistence';
import { useReviewController } from './controllers/useReviewController';
import { useWorkspaceController } from './controllers/useWorkspaceController';
import { useWorkspaceRequestGuard } from './controllers/useWorkspaceRequestGuard';
import { formatError } from './controllers/controllerUtils';
import '../shared/styles/workspace.css';

const GraphWorkspace = lazy(() => import('../features/graph-workspace/GraphWorkspace')
  .then((module) => ({ default: module.GraphWorkspace })));
const GraphChatPanel = lazy(() => import('../features/graph-chat/GraphChatPanel')
  .then((module) => ({ default: module.GraphChatPanel })));
const SearchPanel = lazy(() => import('../features/search/SearchPanel')
  .then((module) => ({ default: module.SearchPanel })));
const WorkspaceImportPanel = lazy(() => import('../features/import/WorkspaceImportPanel')
  .then((module) => ({ default: module.WorkspaceImportPanel })));
const JobRunsPanel = lazy(() => import('../features/job-runs/JobRunsPanel')
  .then((module) => ({ default: module.JobRunsPanel })));
const ReviewTray = lazy(() => import('../features/merge-review/ReviewTray')
  .then((module) => ({ default: module.ReviewTray })));
const AiSettingsPanel = lazy(() => import('../features/settings/AiSettingsPanel')
  .then((module) => ({ default: module.AiSettingsPanel })));
const NodeInspectorHost = lazy(() => import('./NodeInspectorHost')
  .then((module) => ({ default: module.NodeInspectorHost })));
const PaperReader = lazy(() => import('../features/source-reader/PaperReader')
  .then((module) => ({ default: module.PaperReader })));

const emptyGraphSnapshot: GraphCanvasSnapshot = {
  schema_version: 1,
  nodes: [],
  edges: [],
  paths: []
};

type SidebarPanel = 'search' | 'sources' | 'jobs' | 'updates' | 'settings' | null;
type WorkspaceView = 'graph' | 'paper';

type WorkspaceAppProps = {
  initialWorkspace: WorkspaceState;
  initialWorkspaceError?: string | null;
};

export function WorkspaceApp({ initialWorkspace, initialWorkspaceError = null }: WorkspaceAppProps) {
  const hydratedWorkspaceRef = useRef<string | null>(null);
  const nodeSelectionRequestRef = useRef(0);
  const paperInputRef = useRef<HTMLInputElement | null>(null);
  const [workspace, setWorkspace] = useState<WorkspaceState | null>(initialWorkspace);
  const hasWorkspace = workspace?.has_workspace === true;
  const [sidebarOpen, setSidebarOpen] = useState(true);
  const [activeSidebarPanel, setActiveSidebarPanel] = useState<SidebarPanel>(null);
  const [sourcePathDraft, setSourcePathDraft] = useState('');
  const [workspaceBusy, setWorkspaceBusy] = useState(false);
  const [workspaceNotice, setWorkspaceNotice] = useState<string | null>(null);
  const [workspaceError, setWorkspaceError] = useState<string | null>(initialWorkspaceError);
  const {
    draft: aiSettingsDraft,
    activeSettings: activeAiSettings,
    notice: aiSettingsNotice,
    setupMessage: brainSetupMessage,
    updateDraft: updateAiSettingsDraft,
    reportNotice: reportAiSettingsNotice,
    save: saveAiSettings,
    authorizeCodex: authorizeCodexBrain,
    enableCodex: enableCodexBrain
  } = useBrainSettingsController();
  const [snapshot, setSnapshot] = useState<GraphCanvasSnapshot>(emptyGraphSnapshot);
  const visibleSnapshot = snapshot;
  const [connectedness, setConnectedness] = useState(70);
  const [pinnedNodeIds, setPinnedNodeIds] = useState<string[]>([]);
  const [layoutOverrides, setLayoutOverrides] = useState<Record<string, LayoutNode>>({});
  const projectedLayoutNodesRef = useRef<LayoutNode[]>([]);
  const [paperFile, setPaperFile] = useState<File | null>(null);
  const [activeWorkspaceView, setActiveWorkspaceView] = useState<WorkspaceView>('graph');
  const [readingContext, setReadingContext] = useState<SourceReadingContext | null>(null);
  const [captureGraphChanges, setCaptureGraphChanges] = useState(false);
  const brainLabel = labelForBrain(activeAiSettings);
  const brainEffort = effortForBrain(activeAiSettings, captureGraphChanges);
  const canStopBrain = ['codex_sdk', 'claude_code'].includes(activeAiSettings?.providerId ?? '');
  const [jobRuns, setJobRuns] = useState<JobRun[]>([]);
  const [reviewQueue, setReviewQueue] = useState<GraphReviewQueueReadModel>(emptyReviewQueue);
  const [selectedNodeId, setSelectedNodeId] = useState<string | null>(null);
  const [selectedNodeOverride, setSelectedNodeOverride] = useState<GraphCanvasNode | null>(null);
  const [focusNodeIds, setFocusNodeIds] = useState<string[]>([]);
  const graphIsEmpty = visibleSnapshot.nodes.length === 0;
  const [graphCanvasReady, setGraphCanvasReady] = useState(false);
  const workspaceKey = workspace?.workspace_dir ?? workspace?.database_path ?? 'no-workspace';
  const workspaceGuard = useWorkspaceRequestGuard(workspaceKey);
  const graphReadModels = useGraphReadModelCoordinator({
    workspaceKey,
    setSnapshot,
    setLayoutOverrides,
    setPinnedNodeIds,
    setReviewQueue
  });
  const saveNodeLayout = useNodeLayoutPersistence({
    hasWorkspace,
    workspaceGuard,
    graphReadModels,
    setWorkspaceError
  });
  const nodesById = useMemo(
    () => new Map(visibleSnapshot.nodes.map((node) => [node.id, node])),
    [visibleSnapshot.nodes]
  );
  const selectedNode = selectedNodeId
    ? nodesById.get(selectedNodeId)
      ?? (selectedNodeOverride?.id === selectedNodeId ? selectedNodeOverride : null)
    : null;
  const paperIsActive = activeWorkspaceView === 'paper' && paperFile !== null;
  const activeReadingContext = paperIsActive ? readingContext : null;
  const readingContextPending = paperIsActive && activeReadingContext === null;
  const visibleSelectedNode = paperIsActive ? null : selectedNode;

  useEffect(() => {
    if (graphIsEmpty) {
      setGraphCanvasReady(false);
      return undefined;
    }
    setGraphCanvasReady(false);
    const frame = window.requestAnimationFrame(() => setGraphCanvasReady(true));
    return () => window.cancelAnimationFrame(frame);
  }, [graphIsEmpty, workspaceKey]);
  const focusAreas = useMemo(() => focusNodeIds.flatMap((nodeId) => {
    const node = nodesById.get(nodeId);
    return node ? [{ id: node.id, title: node.title, type: node.type }] : [];
  }), [focusNodeIds, nodesById]);
  const pendingUpdates = pendingReviewCount(reviewQueue);
  const handleBrainSetupRequired = useCallback((message: string) => {
    setWorkspaceNotice(null);
    setWorkspaceError(message);
    reportAiSettingsNotice(message);
    setSidebarOpen(true);
    setActiveSidebarPanel('settings');
  }, [reportAiSettingsNotice]);
  const markWorkspaceHydrationStarted = useCallback((nextWorkspaceKey: string) => {
    hydratedWorkspaceRef.current = nextWorkspaceKey;
  }, []);
  const ensureWorkspaceForGraphChat = useCallback(async () => {
    if (workspace?.has_workspace) return workspaceKey;
    setWorkspaceBusy(true);
    setWorkspaceNotice(null);
    setWorkspaceError(null);
    try {
      const next = await createWorkspaceAuto();
      const nextWorkspaceKey = next.workspace_dir ?? next.database_path ?? 'workspace';
      graphReadModels.activateWorkspace(nextWorkspaceKey);
      hydratedWorkspaceRef.current = nextWorkspaceKey;
      setWorkspace(next);
      setWorkspaceNotice('New graph created.');
      return nextWorkspaceKey;
    } catch (error) {
      setWorkspaceError(formatError(error));
      return null;
    } finally {
      setWorkspaceBusy(false);
    }
  }, [graphReadModels, workspace?.has_workspace, workspaceKey]);

  const reviewController = useReviewController({
    workspaceGuard,
    reviewQueue,
    graphReadModels,
    setWorkspaceError
  });
  const jobController = useJobController({
    workspaceGuard,
    jobRuns,
    setJobRuns,
    graphReadModels,
    setWorkspaceNotice,
    setWorkspaceError,
    brainSetupMessage,
    onBrainSetupRequired: handleBrainSetupRequired,
    setActiveSidebarPanel
  });
  const graphChatController = useGraphChatController({
    workspaceKey,
    workspaceGuard,
    hasWorkspace,
    ensureWorkspace: ensureWorkspaceForGraphChat,
    focusNodeIds,
    readingContext: activeReadingContext,
    readingContextPending,
    captureGraphChanges,
    brainEffort,
    reviewQueue,
    graphReadModels,
    setWorkspaceNotice,
    setWorkspaceError,
    brainSetupMessage,
    onBrainSetupRequired: handleBrainSetupRequired
  });
  const activateWorkspace = useCallback((nextWorkspaceKey: string) => {
    graphReadModels.activateWorkspace(nextWorkspaceKey);
    return graphChatController.activateWorkspace(nextWorkspaceKey);
  }, [graphChatController.activateWorkspace, graphReadModels]);
  const workspaceController = useWorkspaceController({
    workspaceGuard,
    setWorkspace,
    onWorkspaceHydrationStarted: markWorkspaceHydrationStarted,
    sourcePathDraft,
    setWorkspaceBusy,
    setWorkspaceNotice,
    setWorkspaceError,
    emptyGraphSnapshot,
    setSnapshot,
    setReviewQueue,
    setLayoutOverrides,
    setPinnedNodeIds,
    activateWorkspace,
    graphReadModels,
    setJobRuns,
    setSelectedNodeId,
    setFocusNodeIds
  });
  const readyJobs = jobController.readyJobs;

  useEffect(() => {
    if (selectedNodeOverride && nodesById.has(selectedNodeOverride.id)) {
      setSelectedNodeOverride(null);
    }
    if (
      selectedNodeId !== null
      && !nodesById.has(selectedNodeId)
      && selectedNodeOverride?.id !== selectedNodeId
    ) {
      setSelectedNodeId(null);
    }
    setFocusNodeIds((ids) => {
      const existingIds = ids.filter((id) => nodesById.has(id));
      return existingIds.length === ids.length ? ids : existingIds;
    });
  }, [nodesById, selectedNodeId, selectedNodeOverride]);

  useEffect(() => {
    nodeSelectionRequestRef.current += 1;
    setSelectedNodeId(null);
    setSelectedNodeOverride(null);
  }, [workspaceKey]);

  useEffect(() => {
    if (!workspace?.has_workspace) return;
    const workspaceKey = workspace.workspace_dir ?? workspace.database_path ?? 'workspace';
    if (hydratedWorkspaceRef.current === workspaceKey) return;
    hydratedWorkspaceRef.current = workspaceKey;
    void workspaceController.refreshWorkspaceData(workspace).catch((error) => {
      if (hydratedWorkspaceRef.current !== workspaceKey) return;
      hydratedWorkspaceRef.current = null;
      setWorkspaceError(formatError(error));
    });
  }, [
    workspace?.has_workspace,
    workspace?.workspace_dir,
    workspace?.database_path,
    workspaceController.refreshWorkspaceData
  ]);

  useEffect(() => {
    if (!hasWorkspace || activeSidebarPanel !== 'jobs') return;
    const requestOwner = workspaceGuard.capture();
    void jobController.refreshJobs().catch((error) => {
      if (workspaceGuard.owns(requestOwner)) setWorkspaceError(formatError(error));
    });
  }, [activeSidebarPanel, hasWorkspace, jobController.refreshJobs, workspaceGuard]);

  useEffect(() => {
    if (!hasWorkspace || activeSidebarPanel !== 'sources' || workspace?.stats) return;
    const requestOwner = workspaceGuard.capture();
    void workspaceController.refreshWorkspaceStats().catch((error) => {
      if (workspaceGuard.owns(requestOwner)) setWorkspaceError(formatError(error));
    });
  }, [
    activeSidebarPanel,
    hasWorkspace,
    workspace?.stats,
    workspaceController.refreshWorkspaceStats,
    workspaceGuard
  ]);

  useEffect(() => {
    if (!hasWorkspace || activeSidebarPanel !== 'updates') return;
    const requestOwner = workspaceGuard.capture();
    void reviewController.refreshReviewQueue().catch((error) => {
      if (workspaceGuard.owns(requestOwner)) setWorkspaceError(formatError(error));
    });
  }, [activeSidebarPanel, hasWorkspace, reviewController.refreshReviewQueue, workspaceGuard]);

  const clearSelectedNode = useCallback(() => {
    nodeSelectionRequestRef.current += 1;
    setSelectedNodeId(null);
    setSelectedNodeOverride(null);
  }, []);

  const selectGraphNode = useCallback((nodeId: string, node?: GraphCanvasNode) => {
    const request = nodeSelectionRequestRef.current + 1;
    nodeSelectionRequestRef.current = request;
    setActiveWorkspaceView('graph');
    const availableNode = node ?? nodesById.get(nodeId)
      ?? (selectedNodeOverride?.id === nodeId ? selectedNodeOverride : null);
    if (availableNode) {
      setSelectedNodeOverride(nodesById.has(nodeId) ? null : availableNode);
      setSelectedNodeId(nodeId);
      return;
    }

    setSelectedNodeId(null);
    setSelectedNodeOverride(null);
    const requestOwner = workspaceGuard.capture();
    void loadGraphNodeDetail(nodeId)
      .then((detail) => {
        if (nodeSelectionRequestRef.current !== request || !workspaceGuard.owns(requestOwner)) return;
        setSelectedNodeOverride(detail);
        setSelectedNodeId(detail.id);
      })
      .catch((error) => {
        if (nodeSelectionRequestRef.current === request && workspaceGuard.owns(requestOwner)) {
          setWorkspaceError(formatError(error));
        }
      });
  }, [nodesById, selectedNodeOverride, workspaceGuard]);

  const toggleFocusNode = useCallback((nodeId: string) => {
    setFocusNodeIds((ids) => (
      ids.includes(nodeId)
        ? ids.filter((id) => id !== nodeId)
        : [...ids, nodeId]
    ));
  }, []);

  const handleProjectedLayoutChange = useCallback((nodes: LayoutNode[]) => {
    projectedLayoutNodesRef.current = nodes;
  }, []);

  const togglePinnedNode = useCallback((nodeId: string | null) => {
    if (!nodeId) return;
    const isPinned = pinnedNodeIds.includes(nodeId);
    const layout = projectedLayoutNodesRef.current.find((node) => node.node_id === nodeId);
    if (isPinned) {
      setPinnedNodeIds((ids) => ids.filter((id) => id !== nodeId));
      if (layout) {
        void saveNodeLayout({ ...layout, pinned: false });
      }
      return;
    }

    if (layout) {
      const pinnedLayout = { ...layout, pinned: true };
      setLayoutOverrides((overrides) => upsertLayoutOverride(overrides, pinnedLayout));
      void saveNodeLayout(pinnedLayout);
    }
    setPinnedNodeIds((ids) => [...ids, nodeId]);
  }, [pinnedNodeIds, saveNodeLayout]);

  const handleNodePositionChange = useCallback((nodeId: string, position: { x: number; y: number }) => {
    const layoutNode = layoutNodeFromPosition(nodeId, position, true);
    setLayoutOverrides((overrides) => upsertLayoutOverride(overrides, layoutNode));
    setPinnedNodeIds((ids) => pinnedNodeIdsWith(ids, nodeId, true));
    void saveNodeLayout(layoutNode);
  }, [saveNodeLayout]);

  return (
    <div
      className={[
        'appShell',
        visibleSelectedNode ? 'hasDocumentPanel' : 'isGraphOnly',
        sidebarOpen ? 'hasSidebar' : 'isSidebarCollapsed'
      ].join(' ')}
    >
      <input
        ref={paperInputRef}
        className="srOnly"
        type="file"
        accept="application/pdf,.pdf"
        tabIndex={-1}
        onChange={(event) => {
          const file = event.currentTarget.files?.[0];
          event.currentTarget.value = '';
          if (file) openPaper(file);
        }}
      />
      <button
        className="sidebarToggle"
        type="button"
        aria-label={sidebarOpen ? 'Hide sidebar' : 'Show sidebar'}
        aria-expanded={sidebarOpen}
        onClick={() => {
          setSidebarOpen((open) => !open);
          setActiveSidebarPanel(null);
        }}
      >
        <SidebarToggleIcon />
      </button>

      <aside
        className="workspaceSidebar"
        aria-label="Workspace navigation"
        aria-hidden={!sidebarOpen}
        inert={!sidebarOpen}
      >
        <div className="brandBlock" aria-label="Soma">
          <span className="brandMark" aria-hidden="true" />
        </div>

        <nav className="navList" aria-label="Primary">
          <button
            className={`navItem ${activeWorkspaceView === 'graph' ? 'isActive' : ''}`}
            type="button"
            aria-label="Graph"
            aria-pressed={activeWorkspaceView === 'graph'}
            onClick={() => {
              setActiveWorkspaceView('graph');
              setActiveSidebarPanel(null);
            }}
          >
            <NavIcon name="graph" />
            <span className="srOnly">Graph</span>
          </button>
          <button
            className={`navItem ${paperIsActive ? 'isActive' : ''}`}
            type="button"
            aria-label={paperFile ? 'Paper' : 'Open paper'}
            aria-pressed={paperIsActive}
            onClick={() => {
              if (paperFile) {
                setActiveWorkspaceView('paper');
                setActiveSidebarPanel(null);
                return;
              }
              paperInputRef.current?.click();
            }}
          >
            <NavIcon name="paper" />
            <span className="srOnly">{paperFile ? 'Paper' : 'Open paper'}</span>
          </button>
          <button
            className={`navItem ${activeSidebarPanel === 'search' ? 'isActive' : ''}`}
            type="button"
            aria-label="Search"
            aria-pressed={activeSidebarPanel === 'search'}
            onClick={() => toggleSidebarPanel('search')}
          >
            <NavIcon name="search" />
            <span className="srOnly">Search</span>
          </button>
          <button
            className={`navItem ${activeSidebarPanel === 'sources' ? 'isActive' : ''}`}
            type="button"
            aria-label="Sources"
            aria-pressed={activeSidebarPanel === 'sources'}
            onClick={() => toggleSidebarPanel('sources')}
          >
            <NavIcon name="sources" />
            <span className="srOnly">Sources</span>
          </button>
          <button
            className={`navItem ${activeSidebarPanel === 'jobs' ? 'isActive' : ''}`}
            type="button"
            aria-label="Compile Graph"
            aria-pressed={activeSidebarPanel === 'jobs'}
            onClick={() => toggleSidebarPanel('jobs')}
          >
            <NavIcon name="jobs" />
            {readyJobs > 0 ? <span className="navBadge">{readyJobs}</span> : null}
            <span className="srOnly">Compile Graph</span>
          </button>
          <button
            className={`navItem ${activeSidebarPanel === 'updates' ? 'isActive' : ''}`}
            type="button"
            aria-label="Updates"
            aria-pressed={activeSidebarPanel === 'updates'}
            onClick={() => toggleSidebarPanel('updates')}
          >
            <NavIcon name="updates" />
            {pendingUpdates > 0 ? <span className="navBadge">{pendingUpdates}</span> : null}
            <span className="srOnly">Updates</span>
          </button>
        </nav>

        <section className="sidebarSettings" aria-label="Sidebar controls">
          <button
            className={activeSidebarPanel === 'settings' ? 'isActive' : ''}
            type="button"
            aria-label="Settings"
            aria-pressed={activeSidebarPanel === 'settings'}
            onClick={() => toggleSidebarPanel('settings')}
          >
            <SettingsIcon />
          </button>
        </section>
      </aside>

      {sidebarOpen && activeSidebarPanel ? (
        <aside
          className={`sidebarDetailPanel ${activeSidebarPanel === 'settings' ? 'isSettingsPanel' : ''}`}
          aria-label={sidebarPanelTitle(activeSidebarPanel)}
        >
          <header className="sidebarDetailHeader">
            <div>
              <h2>{sidebarPanelTitle(activeSidebarPanel)}</h2>
            </div>
            <button type="button" aria-label="Close panel" onClick={() => setActiveSidebarPanel(null)}>
              <CloseIcon />
            </button>
          </header>

          <Suspense fallback={null}>
            {activeSidebarPanel === 'search' ? (
              <SearchPanel
                snapshot={visibleSnapshot}
                hasWorkspace={hasWorkspace}
                onSelectNode={(node) => selectGraphNode(node.id, node)}
              />
            ) : null}

            {activeSidebarPanel === 'sources' ? (
              <WorkspaceImportPanel
                workspace={workspace}
                sourcePathDraft={sourcePathDraft}
                busy={workspaceBusy || jobController.jobRunBusyId !== null}
                notice={workspaceNotice}
                error={workspaceError}
                onSourcePathChange={setSourcePathDraft}
                onCreateWorkspace={() => { void workspaceController.handleCreateWorkspace(); }}
                onOpenWorkspace={() => { void workspaceController.handleOpenWorkspace(); }}
                onImportSource={() => { void workspaceController.handleImportSource(); }}
                onCompileGraph={() => { void jobController.handleCompileGraph(); }}
              />
            ) : null}

            {activeSidebarPanel === 'jobs' ? (
              <JobRunsPanel
                jobs={jobRuns}
                busyJobId={jobController.jobRunBusyId}
                notice={workspaceNotice}
                error={workspaceError}
                onCompileGraph={() => { void jobController.handleCompileGraph(); }}
                onRunCompile={(jobId) => { void jobController.handleRunCompileJob(jobId); }}
                onImportPatch={(jobId) => { void jobController.handleImportJobRunPatch(jobId); }}
                onOpenFolder={(jobId) => { void jobController.handleOpenJobFolder(jobId); }}
                onOpenReviewUpdates={() => setActiveSidebarPanel('updates')}
                onClearHistory={() => { void jobController.handleClearJobHistory(); }}
              />
            ) : null}

            {activeSidebarPanel === 'updates' ? (
              <ReviewTray
                readModel={reviewQueue}
                nodes={snapshot.nodes}
                busy={reviewController.mutationBusy}
                onAction={reviewController.handleReviewAction}
              />
            ) : null}

            {activeSidebarPanel === 'settings' ? (
              <AiSettingsPanel
                value={aiSettingsDraft}
                notice={aiSettingsNotice}
                onChange={updateAiSettingsDraft}
                onNotice={reportAiSettingsNotice}
                onSave={saveAiSettings}
                onAuthorizeCodex={authorizeCodexBrain}
                onEnableCodex={enableCodexBrain}
              />
            ) : null}
          </Suspense>
        </aside>
      ) : null}

      <main className="workspaceShell">
        <div
          className="workspaceView"
          aria-hidden={paperIsActive}
          inert={paperIsActive}
        >
          {graphIsEmpty ? (
            <EmptyGraphWorkspace
              title={emptyGraphTitle(workspace, pendingUpdates)}
              detail={emptyGraphDetail(workspace, pendingUpdates)}
              action={emptyGraphAction(pendingUpdates, workspaceBusy, graphChatController.requestFocus, () => {
                setActiveSidebarPanel('updates');
              })}
            />
          ) : !graphCanvasReady ? (
            <section className="graphWorkspace" aria-label="Conversation graph" />
          ) : (
            <Suspense fallback={<section className="graphWorkspace" aria-label="Conversation graph" />}>
              <GraphWorkspace
                snapshot={visibleSnapshot}
                connectedness={connectedness}
                onConnectednessChange={setConnectedness}
                layoutOverrides={layoutOverrides}
                onProjectedLayoutChange={handleProjectedLayoutChange}
                selectedNodeId={selectedNode?.id ?? null}
                onSelectNode={selectGraphNode}
                onClearSelection={clearSelectedNode}
                onNodePositionChange={handleNodePositionChange}
                pinnedNodeIds={pinnedNodeIds}
                onTogglePin={togglePinnedNode}
                focusNodeIds={focusNodeIds}
                onToggleFocusNode={toggleFocusNode}
                viewportKey={sidebarOpen ? 'sidebar' : 'full'}
              />
            </Suspense>
          )}
        </div>

        {paperFile ? (
          <div
            className="workspaceView"
            aria-hidden={!paperIsActive}
            inert={!paperIsActive}
          >
            <Suspense fallback={<section className="paperReader" aria-label="Opening paper" />}>
              <PaperReader
                file={paperFile}
                onContextChange={setReadingContext}
                onClose={closePaper}
              />
            </Suspense>
          </div>
        ) : null}
      </main>

      {visibleSelectedNode ? (
        <Suspense fallback={null}>
          <NodeInspectorHost
            key={`${workspaceKey}:${visibleSelectedNode.id}`}
            workspaceGuard={workspaceGuard}
            hasWorkspace={hasWorkspace}
            node={visibleSelectedNode}
            reviewQueue={reviewQueue}
            graphReadModels={graphReadModels}
            setWorkspaceNotice={setWorkspaceNotice}
            setWorkspaceError={setWorkspaceError}
            brainSetupMessage={brainSetupMessage}
            brainLabel={brainLabel}
            brainEffort={brainEffort}
            canStopBrain={canStopBrain}
            onBrainSetupRequired={handleBrainSetupRequired}
            captureGraphChanges={captureGraphChanges}
            canFocus={nodesById.has(visibleSelectedNode.id)}
            isFocused={focusNodeIds.includes(visibleSelectedNode.id)}
            onSelectNode={selectGraphNode}
            onToggleFocusNode={toggleFocusNode}
            onCaptureGraphChangesChange={setCaptureGraphChanges}
            onOpenReviewUpdates={() => setActiveSidebarPanel('updates')}
            onUndoGraphChanges={graphChatController.undo}
            undoBusyPatchId={graphChatController.undoBusyPatchId}
          />
        </Suspense>
      ) : null}

      <footer className="statusDock" aria-label="Workspace status">
        <Suspense fallback={<div className="graphChatLoading" role="status">Loading chat...</div>}>
          <GraphChatPanel
            messages={graphChatController.messages}
            draft={graphChatController.draft}
            usedAreas={graphChatController.usedAreas}
            focusAreas={focusAreas}
            readingContextPending={readingContextPending}
            captureGraphChanges={captureGraphChanges}
            reviewQueue={reviewQueue}
            errorsByMessageId={graphChatController.errorsByMessageId}
            busyMessageId={graphChatController.busyMessageId}
            brainLabel={brainLabel}
            brainEffort={brainEffort}
            activeRun={graphChatController.activeRun}
            canStop={canStopBrain}
            focusRequest={graphChatController.focusRequest}
            onDraftChange={graphChatController.setDraft}
            onCaptureGraphChangesChange={setCaptureGraphChanges}
            onSubmit={graphChatController.send}
            onStop={graphChatController.stop}
            onSelectNode={selectGraphNode}
            onLoadMessages={graphChatController.ensureHistory}
            onOpenReviewUpdates={() => setActiveSidebarPanel('updates')}
            onUndoGraphChanges={graphChatController.undo}
            undoablePatch={graphChatController.undoablePatch}
            undoBusyPatchId={graphChatController.undoBusyPatchId}
          />
        </Suspense>
      </footer>
    </div>
  );

  function toggleSidebarPanel(panel: Exclude<SidebarPanel, null>) {
    setActiveSidebarPanel((current) => current === panel ? null : panel);
  }

  function openPaper(file: File) {
    if (!file.name.toLowerCase().endsWith('.pdf') && file.type !== 'application/pdf') {
      setWorkspaceError('Choose a PDF paper.');
      return;
    }
    if (file.size > 200 * 1024 * 1024) {
      setWorkspaceError('This PDF is larger than 200 MB. Choose a smaller paper.');
      return;
    }
    setPaperFile(file);
    setReadingContext(null);
    setActiveWorkspaceView('paper');
    setActiveSidebarPanel(null);
    setCaptureGraphChanges(false);
    setWorkspaceError(null);
    setWorkspaceNotice(`Opened ${file.name}.`);
  }

  function closePaper() {
    setActiveWorkspaceView('graph');
    setPaperFile(null);
    setReadingContext(null);
  }
}

type EmptyGraphWorkspaceProps = {
  title: string;
  detail: string;
  action: ReturnType<typeof emptyGraphAction>;
};

function EmptyGraphWorkspace({ title, detail, action }: EmptyGraphWorkspaceProps) {
  return (
    <section className="graphWorkspace" aria-label="Conversation graph">
      <div className="workspaceHeader">
        <div>
          <h2>Conversation Graph</h2>
        </div>
      </div>
      <div className="graphCanvas">
        <div className="canvasEmptyState">
          <p>{title}</p>
          <span>{detail}</span>
          <button type="button" disabled={action.disabled} onClick={action.onClick}>
            {action.label}
          </button>
        </div>
      </div>
    </section>
  );
}

function emptyGraphTitle(workspace: WorkspaceState | null, pendingUpdates: number) {
  if (workspace === null) return 'Loading workspace';
  if (pendingUpdates > 0) return 'Review Updates';
  return 'Start with graph chat';
}

function emptyGraphDetail(workspace: WorkspaceState | null, pendingUpdates: number) {
  if (workspace === null) return 'Restoring the last workspace.';
  if (!workspace?.has_workspace) return 'Ask in the chat dock. Soma will create a graph for this conversation.';
  if (pendingUpdates > 0) {
    return (
      `${pendingUpdates} proposed update${pendingUpdates === 1 ? '' : 's'} are ready. ` +
      'Review Updates accepts them into the graph.'
    );
  }
  return 'Ask in the chat dock. Valid evidence-backed updates are saved into the graph as you talk.';
}

function emptyGraphAction(
  pendingUpdates: number,
  workspaceBusy: boolean,
  onStartChat: () => void,
  onReviewUpdates: () => void
) {
  if (pendingUpdates > 0) {
    return {
      label: 'Review Updates',
      disabled: workspaceBusy,
      onClick: onReviewUpdates
    };
  }
  return {
    label: 'Start Chat',
    disabled: workspaceBusy,
    onClick: onStartChat
  };
}

function sidebarPanelTitle(panel: Exclude<SidebarPanel, null>) {
  if (panel === 'search') return 'Search';
  if (panel === 'sources') return 'Sources';
  if (panel === 'jobs') return 'Compile Graph';
  if (panel === 'updates') return 'Review Updates';
  return 'Settings';
}

type NavIconName = 'graph' | 'paper' | 'search' | 'sources' | 'jobs' | 'updates';

function NavIcon({ name }: { name: NavIconName }) {
  if (name === 'paper') {
    return (
      <svg className="navIcon" viewBox="0 0 24 24" aria-hidden="true">
        <path d="M6 3.5h8l4 4v13H6z" />
        <path d="M14 3.5v4h4M9 12h6M9 15.5h6" />
      </svg>
    );
  }

  if (name === 'search') {
    return (
      <svg className="navIcon" viewBox="0 0 24 24" aria-hidden="true">
        <circle cx="10.5" cy="10.5" r="5.5" />
        <path d="M15 15l4 4" />
      </svg>
    );
  }

  if (name === 'sources') {
    return (
      <svg className="navIcon" viewBox="0 0 24 24" aria-hidden="true">
        <path d="M4.5 7.5h5l1.7 2h8.3v7.8H4.5z" />
        <path d="M8 14h8" />
      </svg>
    );
  }

  if (name === 'jobs') {
    return (
      <svg className="navIcon" viewBox="0 0 24 24" aria-hidden="true">
        <path d="M5.5 7.5h13v9h-13z" />
        <path d="M8 10h8M8 13h5" />
      </svg>
    );
  }

  if (name === 'updates') {
    return (
      <svg className="navIcon" viewBox="0 0 24 24" aria-hidden="true">
        <path d="M6 12.4l3.2 3.2L18 7.8" />
        <path d="M5.5 5.5h13v13h-13z" />
      </svg>
    );
  }

  return (
    <svg className="navIcon" viewBox="0 0 24 24" aria-hidden="true">
      <circle cx="7" cy="8" r="2" />
      <circle cx="16.5" cy="6.5" r="2" />
      <circle cx="17" cy="16" r="2" />
      <circle cx="7.5" cy="17" r="2" />
      <path d="M8.8 8.9l5.1-1.6M15.5 8.2l1 5.9M15.2 16.1l-5.7.8M8.8 15.5l6.4-7.5" />
    </svg>
  );
}

function CloseIcon() {
  return (
    <svg className="closeIcon" viewBox="0 0 24 24" aria-hidden="true">
      <path d="M7 7l10 10M17 7L7 17" />
    </svg>
  );
}

const settingsIconPath = [
  'M19.4 15a1.7 1.7 0 0 0 .34 1.82l.06.06a2 2 0 1 1-2.83 2.83l-.06-.06',
  'a1.7 1.7 0 0 0-1.82-.34 1.7 1.7 0 0 0-1 1.52V21a2 2 0 1 1-4 0v-.09',
  'a1.7 1.7 0 0 0-1-1.52 1.7 1.7 0 0 0-1.82.34l-.06.06a2 2 0 1 1-2.83-2.83',
  'l.06-.06a1.7 1.7 0 0 0 .34-1.82 1.7 1.7 0 0 0-1.52-1H3a2 2 0 1 1 0-4h.09',
  'a1.7 1.7 0 0 0 1.52-1 1.7 1.7 0 0 0-.34-1.82l-.06-.06a2 2 0 1 1 2.83-2.83',
  'l.06.06a1.7 1.7 0 0 0 1.82.34h.01a1.7 1.7 0 0 0 1-1.52V3a2 2 0 1 1 4 0v.09',
  'a1.7 1.7 0 0 0 1 1.52h.01a1.7 1.7 0 0 0 1.82-.34l.06-.06a2 2 0 1 1 2.83 2.83',
  'l-.06.06a1.7 1.7 0 0 0-.34 1.82v.01a1.7 1.7 0 0 0 1.52 1H21a2 2 0 1 1 0 4h-.09',
  'a1.7 1.7 0 0 0-1.51 1.07z'
].join(' ');

function SettingsIcon() {
  return (
    <svg className="settingsIcon" viewBox="0 0 24 24" aria-hidden="true">
      <circle cx="12" cy="12" r="3" />
      <path d={settingsIconPath} />
    </svg>
  );
}

function SidebarToggleIcon() {
  return (
    <svg className="sidebarToggleIcon" viewBox="0 0 24 24" aria-hidden="true">
      <rect x="4.5" y="6.5" width="15" height="11" rx="2" />
      <path d="M9 7v10" />
    </svg>
  );
}
