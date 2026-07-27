import type {
  GraphCanvasEdge,
  GraphCanvasNode,
  GraphCanvasSnapshot,
  LayoutNode,
  ProjectedGraphSnapshot
} from '../../../../../packages/contracts/src';

import { buildGraphLayout } from './layout.ts';

const EDGE_TYPE_PRIORITY = new Map([
  ['part_of', 100],
  ['depends_on', 92],
  ['implements', 88],
  ['supports', 84],
  ['answers', 80],
  ['derived_from', 76],
  ['mitigates', 72],
  ['next_step', 68],
  ['blocks', 64],
  ['alternative_to', 48],
  ['contradicts', 44],
  ['mentions', 24]
]);

type ProjectionOptions = {
  connectedness?: number;
  viewport?: 'narrow' | 'desktop' | string;
  pinnedNodeIds?: string[];
  layoutOverrides?: Record<string, LayoutNode> | Map<string, LayoutNode>;
};

export function projectGraphView(
  snapshot: GraphCanvasSnapshot | null | undefined,
  options: ProjectionOptions = {}
): ProjectedGraphSnapshot {
  const connectedness = normalizeConnectedness(options.connectedness);
  const mode = projectionMode(connectedness);
  const nodes = activeNodes(snapshot);
  const edges = activeEdges(snapshot, nodes);
  const visibleEdges = projectedEdges(nodes, edges, connectedness);
  const totalEdgeCount = Number.isFinite(Number(snapshot?.total_edge_count))
    ? Math.max(0, Number(snapshot?.total_edge_count))
    : edges.length;
  const layoutNodes = buildGraphLayout(nodes, visibleEdges, {
    mode,
    viewport: options.viewport,
    pinnedNodeIds: options.pinnedNodeIds,
    layoutOverrides: options.layoutOverrides
  });
  const layoutByNode = new Map(layoutNodes.map((node) => [node.node_id, node]));

  return {
    schema_version: snapshot?.schema_version ?? 1,
    nodes: nodes.map((node) => ({
      ...node,
      layout: layoutByNode.get(node.id) ?? fallbackLayoutNode(node.id)
    })),
    edges: visibleEdges,
    paths: Array.isArray(snapshot?.paths) ? snapshot.paths : [],
    is_partial: snapshot?.is_partial,
    node_limit: snapshot?.node_limit,
    edge_limit: snapshot?.edge_limit,
    total_node_count: snapshot?.total_node_count,
    total_edge_count: totalEdgeCount,
    projection: {
      connectedness,
      mode,
      total_edge_count: totalEdgeCount,
      visible_edge_count: visibleEdges.length,
      hidden_edge_count: Math.max(0, totalEdgeCount - visibleEdges.length)
    },
    layout: {
      nodes: layoutNodes
    }
  };
}

function normalizeConnectedness(value: unknown) {
  const numeric = Number.isFinite(Number(value)) ? Number(value) : 100;
  return Math.max(0, Math.min(100, Math.round(numeric)));
}

function projectionMode(connectedness: number): 'tree' | 'hybrid' | 'graph' {
  if (connectedness <= 0) return 'tree';
  if (connectedness >= 100) return 'graph';
  return 'hybrid';
}

function projectedEdges(
  nodes: GraphCanvasNode[],
  edges: GraphCanvasEdge[],
  connectedness: number
) {
  if (connectedness >= 100) return sortedEdges(edges).map(copyEdge);

  const treeEdges = spanningForestEdges(nodes, edges);
  if (connectedness <= 0) return treeEdges.map(copyEdge);

  const treeIds = new Set(treeEdges.map((edge) => edge.id));
  const crossLinks = sortedEdges(edges.filter((edge) => !treeIds.has(edge.id)));
  const crossLinkCount = Math.min(
    crossLinks.length,
    Math.max(1, Math.floor((crossLinks.length * connectedness) / 100))
  );

  return sortedEdges([...treeEdges, ...crossLinks.slice(0, crossLinkCount)]).map(copyEdge);
}

function spanningForestEdges(nodes: GraphCanvasNode[], edges: GraphCanvasEdge[]) {
  const parent = new Map(nodes.map((node) => [node.id, node.id]));
  const result: GraphCanvasEdge[] = [];

  for (const edge of sortedEdges(edges)) {
    const source = find(parent, edge.source_node_id);
    const target = find(parent, edge.target_node_id);
    if (!source || !target || source === target) continue;
    parent.set(source, target);
    result.push(edge);
  }

  return result;
}

function find(parent: Map<string, string>, nodeId: string) {
  if (!parent.has(nodeId)) return null;
  let current = nodeId;
  while (parent.get(current) !== current) {
    current = parent.get(current)!;
  }
  return current;
}

function activeNodes(snapshot: GraphCanvasSnapshot | null | undefined) {
  return safeArray(snapshot?.nodes)
    .filter((node) => node?.id && node.status === 'active')
    .map((node) => ({ ...node }));
}

function activeEdges(snapshot: GraphCanvasSnapshot | null | undefined, nodes: GraphCanvasNode[]) {
  const activeNodeIds = new Set(nodes.map((node) => node.id));
  return safeArray(snapshot?.edges)
    .filter((edge) => {
      return edge?.id
        && edge.status === 'active'
        && activeNodeIds.has(edge.source_node_id)
        && activeNodeIds.has(edge.target_node_id);
    })
    .map(copyEdge);
}

function sortedEdges(edges: GraphCanvasEdge[]) {
  return [...edges].sort((a, b) => {
    const priority = edgePriority(b) - edgePriority(a);
    if (priority !== 0) return priority;
    return edgeSortKey(a).localeCompare(edgeSortKey(b));
  });
}

function edgePriority(edge: GraphCanvasEdge) {
  return EDGE_TYPE_PRIORITY.get(edgeType(edge)) ?? 50;
}

function edgeSortKey(edge: GraphCanvasEdge) {
  return [
    edge.source_node_id ?? '',
    edge.target_node_id ?? '',
    edgeType(edge),
    edge.id ?? ''
  ].join(':');
}

function edgeType(edge: GraphCanvasEdge) {
  return edge.type ?? (edge as GraphCanvasEdge & { edge_type?: string }).edge_type ?? '';
}

function copyEdge(edge: GraphCanvasEdge) {
  return { ...edge };
}

function fallbackLayoutNode(nodeId: string): LayoutNode {
  return {
    node_id: nodeId,
    x: 500,
    y: 325,
    left: 50,
    top: 50,
    pinned: false
  };
}

function safeArray<T>(value: T[] | undefined | null): T[] {
  return Array.isArray(value) ? value : [];
}
