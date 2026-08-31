import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import {
  ReactFlow,
  type NodeMouseHandler,
  type NodeTypes,
  type OnNodeDrag,
  type ReactFlowInstance,
  type Viewport
} from '@xyflow/react';
import '@xyflow/react/dist/style.css';

import type { GraphCanvasSnapshot, LayoutNode, ProjectedGraphSnapshot } from '../../../../../packages/contracts/src';
import { projectGraphView } from '../../shared/graph/projection.ts';
import { GraphNodeCard } from './GraphNodeCard';
import {
  graphDetailLevelForZoom,
  toReactFlowGraph,
  type GraphDetailLevel,
  type SomaGraphFlowEdge,
  type SomaGraphFlowNode
} from './reactFlowAdapter';

const nodeTypes = {
  somaGraphNode: GraphNodeCard
} satisfies NodeTypes;

type GraphWorkspaceProps = {
  snapshot: GraphCanvasSnapshot;
  connectedness: number;
  onConnectednessChange: (value: number) => void;
  layoutOverrides: Record<string, LayoutNode>;
  onProjectedLayoutChange: (nodes: LayoutNode[]) => void;
  selectedNodeId: string | null;
  onSelectNode: (nodeId: string) => void;
  onClearSelection: () => void;
  onNodePositionChange: (nodeId: string, position: { x: number; y: number }) => void;
  pinnedNodeIds: string[];
  onTogglePin: (nodeId: string | null) => void;
  focusNodeIds: string[];
  onToggleFocusNode: (nodeId: string) => void;
  viewportKey: string;
};

export function GraphWorkspace({
  snapshot,
  connectedness,
  onConnectednessChange,
  layoutOverrides,
  onProjectedLayoutChange,
  selectedNodeId,
  onSelectNode,
  onClearSelection,
  onNodePositionChange,
  pinnedNodeIds,
  onTogglePin,
  focusNodeIds,
  onToggleFocusNode,
  viewportKey
}: GraphWorkspaceProps) {
  const projectedSnapshot = useMemo(() => projectGraphView(snapshot, {
    connectedness,
    pinnedNodeIds,
    layoutOverrides
  }) as ProjectedGraphSnapshot, [connectedness, layoutOverrides, pinnedNodeIds, snapshot]);
  const mode = projectedSnapshot.projection?.mode ?? 'graph';
  const activeProjection = projectionPreset(mode, connectedness);
  const selectedPinned = selectedNodeId ? pinnedNodeIds.includes(selectedNodeId) : false;
  const [detailLevel, setDetailLevel] = useState<GraphDetailLevel>('mid');
  const flowInstanceRef = useRef<ReactFlowInstance<SomaGraphFlowNode, SomaGraphFlowEdge> | null>(null);
  const canvasRef = useRef<HTMLDivElement | null>(null);
  const flowGraph = useMemo(
    () => toReactFlowGraph(
      projectedSnapshot,
      selectedNodeId,
      pinnedNodeIds,
      focusNodeIds,
      onToggleFocusNode,
      { detailLevel }
    ),
    [detailLevel, focusNodeIds, onToggleFocusNode, pinnedNodeIds, projectedSnapshot, selectedNodeId]
  );

  useEffect(() => {
    onProjectedLayoutChange(projectedSnapshot.layout.nodes);
  }, [onProjectedLayoutChange, projectedSnapshot.layout.nodes]);

  useEffect(() => {
    const instance = flowInstanceRef.current;
    if (!instance) return;

    instance.setNodes(flowGraph.nodes);
    instance.setEdges(flowGraph.edges);
  }, [flowGraph.edges, flowGraph.nodes]);

  useEffect(() => {
    const canvas = canvasRef.current;
    if (!canvas || projectedSnapshot.nodes.length === 0) return undefined;
    let animationFrame: number | null = null;

    const refit = () => {
      if (animationFrame !== null) {
        window.cancelAnimationFrame(animationFrame);
      }
      animationFrame = window.requestAnimationFrame(() => {
        animationFrame = null;
        flowInstanceRef.current?.fitView({ padding: 0.22, duration: 180 });
      });
    };
    window.addEventListener('resize', refit);
    refit();
    return () => {
      window.removeEventListener('resize', refit);
      if (animationFrame !== null) {
        window.cancelAnimationFrame(animationFrame);
      }
    };
  }, [projectedSnapshot.nodes.length, viewportKey]);

  const handleFlowInit = useCallback((instance: ReactFlowInstance<SomaGraphFlowNode, SomaGraphFlowEdge>) => {
    flowInstanceRef.current = instance;
  }, []);

  const handleNodeClick = useCallback<NodeMouseHandler<SomaGraphFlowNode>>((_event, node) => {
    onSelectNode(node.id);
  }, [onSelectNode]);

  const handleNodeDragStop = useCallback<OnNodeDrag<SomaGraphFlowNode>>((_event, node) => {
    onNodePositionChange(node.id, node.position);
  }, [onNodePositionChange]);

  const handleViewportChange = useCallback((viewport: Viewport) => {
    const nextDetailLevel = graphDetailLevelForZoom(viewport.zoom);
    setDetailLevel((current) => (current === nextDetailLevel ? current : nextDetailLevel));
  }, []);

  return (
    <section className="graphWorkspace" aria-label="Conversation graph">
      <div className="workspaceHeader">
        <h2>Conversation Graph</h2>
        <div className="projectionControls" aria-label="Graph projection controls">
          <div className="modeButtons" aria-label="Projection mode presets">
            <button
              className={activeProjection === 'tree' ? 'isActive' : ''}
              type="button"
              aria-pressed={activeProjection === 'tree'}
              onClick={() => onConnectednessChange(0)}
            >
              Tree
            </button>
            <button
              className={activeProjection === 'hybrid' ? 'isActive' : ''}
              type="button"
              aria-pressed={activeProjection === 'hybrid'}
              onClick={() => onConnectednessChange(50)}
            >
              Hybrid
            </button>
            <button
              className={activeProjection === 'graph' ? 'isActive' : ''}
              type="button"
              aria-pressed={activeProjection === 'graph'}
              onClick={() => onConnectednessChange(100)}
            >
              Graph
            </button>
          </div>
          {selectedNodeId ? (
            <button
              className={`pinButton ${selectedPinned ? 'isPinned' : ''}`}
              type="button"
              aria-pressed={selectedPinned}
              onClick={() => onTogglePin(selectedNodeId)}
            >
              Pin
            </button>
          ) : null}
        </div>
      </div>

      <div className="graphCanvas" ref={canvasRef}>
        <ReactFlow
          className={`reactFlowCanvas detail-${detailLevel}`}
          defaultNodes={flowGraph.nodes}
          defaultEdges={flowGraph.edges}
          nodeTypes={nodeTypes}
          nodesDraggable={projectedSnapshot.nodes.length > 0}
          nodesConnectable={false}
          elementsSelectable={false}
          onInit={handleFlowInit}
          onNodeClick={handleNodeClick}
          onPaneClick={onClearSelection}
          onNodeDragStop={handleNodeDragStop}
          onViewportChange={handleViewportChange}
          minZoom={0.24}
          maxZoom={1.8}
          onlyRenderVisibleElements
        />
      </div>
    </section>
  );
}

function projectionPreset(mode: string, connectedness: number) {
  if (mode === 'tree' || mode === 'hybrid' || mode === 'graph') return mode;
  if (connectedness <= 0) return 'tree';
  if (connectedness >= 100) return 'graph';
  return 'hybrid';
}
