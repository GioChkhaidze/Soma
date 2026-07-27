import { MarkerType, type Edge, type Node } from '@xyflow/react';

import type { ProjectedGraphSnapshot, ProjectedGraphNode } from '../../../../../packages/contracts/src';

export type GraphDetailLevel = 'far' | 'mid' | 'near';
export type SomaGraphNodeSelectionRole = 'normal' | 'selected' | 'neighbor' | 'dimmed';
export type SomaGraphEdgeSelectionRole = 'normal' | 'connected' | 'dimmed';

export type SomaGraphNodeData = {
  [key: string]: unknown;
  id: string;
  graphType: string;
  title: string;
  preview: string;
  markers: string[];
  detailLevel: GraphDetailLevel;
  selectionRole: SomaGraphNodeSelectionRole;
  selected: boolean;
  pinned: boolean;
  focused: boolean;
  prominent: boolean;
  onToggleFocus?: (nodeId: string) => void;
};

export type SomaGraphFlowNode = Node<SomaGraphNodeData, 'somaGraphNode'>;

export type SomaGraphEdgeData = {
  [key: string]: unknown;
  graphType: string;
  bridgeText: string;
  detailLevel: GraphDetailLevel;
  selectionRole: SomaGraphEdgeSelectionRole;
};

export type SomaGraphFlowEdge = Edge<SomaGraphEdgeData> & {
  pathOptions?: {
    curvature?: number;
  };
};

export type ReactFlowGraph = {
  nodes: SomaGraphFlowNode[];
  edges: SomaGraphFlowEdge[];
};

export type ReactFlowGraphDisplayOptions = {
  detailLevel?: GraphDetailLevel;
};

export function toReactFlowGraph(
  snapshot: ProjectedGraphSnapshot,
  selectedNodeId: string | null,
  pinnedNodeIds: string[],
  focusNodeIds: string[] = [],
  onToggleFocus?: (nodeId: string) => void,
  options: ReactFlowGraphDisplayOptions = {}
): ReactFlowGraph {
  const detailLevel = options.detailLevel ?? 'near';
  const visibleNodeIds = new Set(snapshot.nodes.map((node) => node.id));
  const nodesById = new Map(snapshot.nodes.map((node) => [node.id, node]));
  const visibleEdges = snapshot.edges
    .filter((edge) => visibleNodeIds.has(edge.source_node_id) && visibleNodeIds.has(edge.target_node_id));
  const selection = graphSelection(visibleEdges, selectedNodeId, visibleNodeIds);
  const pinnedNodeIdSet = new Set(pinnedNodeIds);
  const focusNodeIdSet = new Set(focusNodeIds);
  const prominentNodeIds = prominentNodes(snapshot);

  return {
    nodes: snapshot.nodes.map((node) =>
      toReactFlowNode(node, selection, pinnedNodeIdSet, focusNodeIdSet, prominentNodeIds, detailLevel, onToggleFocus)
    ),
    edges: visibleEdges
      .map((edge) => {
        const handles = edgeHandles(nodesById.get(edge.source_node_id), nodesById.get(edge.target_node_id));
        const selectionRole = edgeSelectionRole(edge.id, selection);
        const markerColor = selectionRole === 'connected'
          ? '#ffffff'
          : selectionRole === 'dimmed'
            ? 'rgba(255, 255, 255, 0.24)'
            : detailLevel === 'near'
              ? '#ececec'
              : '#c8c8c8';
        const showMarker = detailLevel !== 'far' || selectionRole === 'connected';
        return {
          id: edge.id,
          source: edge.source_node_id,
          target: edge.target_node_id,
          sourceHandle: handles.sourceHandle,
          targetHandle: handles.targetHandle,
          type: 'default',
          selectable: false,
          pathOptions: {
            curvature: detailLevel === 'near' ? 0.28 : 0.2
          },
          className: `somaEdge somaEdge--${detailLevel} `
            + `somaEdge--${snapshot.projection?.mode ?? 'graph'} somaEdge--${selectionRole}`,
          markerEnd: !showMarker
            ? undefined
            : {
                type: MarkerType.ArrowClosed,
                width: selectionRole === 'connected' ? 20 : detailLevel === 'near' ? 18 : 14,
                height: selectionRole === 'connected' ? 20 : detailLevel === 'near' ? 18 : 14,
                color: markerColor,
                strokeWidth: selectionRole === 'connected' ? 1.8 : 1.4
              },
          interactionWidth: detailLevel === 'far' ? 8 : 14,
          zIndex: selectionRole === 'connected' ? 1 : 0,
          data: {
            graphType: edge.type,
            bridgeText: edge.bridge_text,
            detailLevel,
            selectionRole
          }
        };
      })
  };
}

export function graphDetailLevelForZoom(zoom: number): GraphDetailLevel {
  if (zoom < 0.7) return 'far';
  if (zoom < 1.1) return 'mid';
  return 'near';
}

function toReactFlowNode(
  node: ProjectedGraphNode,
  selection: GraphSelection,
  pinnedNodeIds: Set<string>,
  focusNodeIds: Set<string>,
  prominentNodeIds: Set<string>,
  detailLevel: GraphDetailLevel,
  onToggleFocus?: (nodeId: string) => void
): SomaGraphFlowNode {
  const selectionRole = nodeSelectionRole(node.id, selection);
  const selected = selectionRole === 'selected';

  return {
    id: node.id,
    type: 'somaGraphNode',
    position: { x: node.layout.x, y: node.layout.y },
    origin: [0.5, 0.5],
    selected,
    draggable: true,
    deletable: false,
    zIndex: selected ? 30 : selectionRole === 'neighbor' ? 20 : 10,
    data: {
      id: node.id,
      graphType: node.type,
      title: node.title,
      preview: node.preview,
      markers: node.markers,
      detailLevel,
      selectionRole,
      selected,
      pinned: Boolean(node.layout.pinned || pinnedNodeIds.has(node.id)),
      focused: focusNodeIds.has(node.id),
      prominent: prominentNodeIds.has(node.id),
      onToggleFocus
    }
  };
}

type GraphSelection = {
  active: boolean;
  selectedNodeId: string | null;
  neighborNodeIds: Set<string>;
  edgeIds: Set<string>;
};

function graphSelection(
  edges: ProjectedGraphSnapshot['edges'],
  selectedNodeId: string | null,
  visibleNodeIds: Set<string>
): GraphSelection {
  if (!selectedNodeId || !visibleNodeIds.has(selectedNodeId)) {
    return {
      active: false,
      selectedNodeId: null,
      neighborNodeIds: new Set(),
      edgeIds: new Set()
    };
  }

  const neighborNodeIds = new Set<string>();
  const edgeIds = new Set<string>();

  for (const edge of edges) {
    if (edge.source_node_id !== selectedNodeId && edge.target_node_id !== selectedNodeId) continue;
    edgeIds.add(edge.id);
    neighborNodeIds.add(edge.source_node_id === selectedNodeId ? edge.target_node_id : edge.source_node_id);
  }

  return {
    active: true,
    selectedNodeId,
    neighborNodeIds,
    edgeIds
  };
}

function nodeSelectionRole(nodeId: string, selection: GraphSelection): SomaGraphNodeSelectionRole {
  if (!selection.active) return 'normal';
  if (nodeId === selection.selectedNodeId) return 'selected';
  if (selection.neighborNodeIds.has(nodeId)) return 'neighbor';
  return 'dimmed';
}

function edgeSelectionRole(edgeId: string, selection: GraphSelection): SomaGraphEdgeSelectionRole {
  if (!selection.active) return 'normal';
  return selection.edgeIds.has(edgeId) ? 'connected' : 'dimmed';
}

function prominentNodes(snapshot: ProjectedGraphSnapshot) {
  const degreeByNode = new Map<string, number>();
  for (const node of snapshot.nodes) {
    degreeByNode.set(node.id, 0);
  }
  for (const edge of snapshot.edges) {
    degreeByNode.set(edge.source_node_id, (degreeByNode.get(edge.source_node_id) ?? 0) + 1);
    degreeByNode.set(edge.target_node_id, (degreeByNode.get(edge.target_node_id) ?? 0) + 1);
  }

  return new Set(
    [...degreeByNode.entries()]
      .filter(([, degree]) => degree > 0)
      .sort((a, b) => b[1] - a[1] || a[0].localeCompare(b[0]))
      .slice(0, 3)
      .map(([nodeId]) => nodeId)
  );
}

function edgeHandles(source?: ProjectedGraphNode, target?: ProjectedGraphNode) {
  if (!source?.layout || !target?.layout) {
    return {
      sourceHandle: 'source-right',
      targetHandle: 'target-left'
    };
  }

  const dx = target.layout.x - source.layout.x;
  const dy = target.layout.y - source.layout.y;
  if (Math.abs(dx) >= Math.abs(dy)) {
    return dx >= 0
      ? { sourceHandle: 'source-right', targetHandle: 'target-left' }
      : { sourceHandle: 'source-left', targetHandle: 'target-right' };
  }

  return dy >= 0
    ? { sourceHandle: 'source-bottom', targetHandle: 'target-top' }
    : { sourceHandle: 'source-top', targetHandle: 'target-bottom' };
}
