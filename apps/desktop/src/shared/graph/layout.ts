import type { GraphCanvasEdge, GraphCanvasNode, LayoutNode } from '../../../../../packages/contracts/src';

const CANVAS_WIDTH = 1000;
const CANVAS_HEIGHT = 650;

type GraphLayoutOptions = {
  mode?: 'tree' | 'hybrid' | 'graph' | string;
  viewport?: 'narrow' | 'desktop' | string;
  pinnedNodeIds?: string[];
  layoutOverrides?: Record<string, unknown> | Map<string, unknown>;
};

export function buildGraphLayout(
  nodes: GraphCanvasNode[],
  edges: GraphCanvasEdge[],
  options: GraphLayoutOptions = {}
): LayoutNode[] {
  const graphNodes = safeArray(nodes).filter((node) => node?.id);
  const graphEdges = safeArray(edges);
  const mode = options.mode ?? 'graph';
  const viewport = options.viewport === 'narrow' ? 'narrow' : 'desktop';
  const pinnedNodeIds = new Set(safeArray(options.pinnedNodeIds));
  const layoutOverrides = normalizeLayoutOverrides(options.layoutOverrides);

  const computed = viewport === 'narrow'
    ? buildNarrowLayout(graphNodes)
    : mode === 'tree'
      ? buildTreeLayout(graphNodes, graphEdges)
      : buildRadialLayout(graphNodes);

  return graphNodes.map((node) => {
    const pinned = pinnedNodeIds.has(node.id);
    const position = pinned && layoutOverrides.has(node.id)
      ? layoutOverrides.get(node.id)!
      : computed.get(node.id) ?? positionFromPercent(50, 50);

    return {
      node_id: node.id,
      ...position,
      pinned
    };
  });
}

function normalizeLayoutPosition(value: unknown): Omit<LayoutNode, 'node_id' | 'pinned'> | null {
  if (!value || typeof value !== 'object') return null;
  const position = value as { x?: unknown; y?: unknown; left?: unknown; top?: unknown };

  if (Number.isFinite(position.x) && Number.isFinite(position.y)) {
    return positionFromXY(Number(position.x), Number(position.y));
  }

  if (Number.isFinite(position.left) && Number.isFinite(position.top)) {
    return positionFromPercent(Number(position.left), Number(position.top));
  }

  return null;
}

function buildNarrowLayout(nodes: GraphCanvasNode[]) {
  const layout = new Map<string, Omit<LayoutNode, 'node_id' | 'pinned'>>();
  const count = Math.max(nodes.length, 1);
  nodes.forEach((node, index) => {
    const top = count === 1 ? 50 : 14 + (index * 72) / (count - 1);
    layout.set(node.id, positionFromPercent(50, top));
  });
  return layout;
}

function buildTreeLayout(nodes: GraphCanvasNode[], edges: GraphCanvasEdge[]) {
  const ids = new Set(nodes.map((node) => node.id));
  const outgoing = new Map(nodes.map((node) => [node.id, [] as string[]]));
  const indegree = new Map(nodes.map((node) => [node.id, 0]));

  for (const edge of edges) {
    if (!ids.has(edge.source_node_id) || !ids.has(edge.target_node_id)) continue;
    outgoing.get(edge.source_node_id)?.push(edge.target_node_id);
    indegree.set(edge.target_node_id, (indegree.get(edge.target_node_id) ?? 0) + 1);
  }

  for (const targets of outgoing.values()) targets.sort();

  const roots = nodes
    .filter((node) => (indegree.get(node.id) ?? 0) === 0)
    .map((node) => node.id);
  const queue = roots.length > 0 ? [...roots] : nodes[0] ? [nodes[0].id] : [];
  const depth = new Map(queue.map((id) => [id, 0]));

  while (queue.length > 0) {
    const id = queue.shift()!;
    for (const target of outgoing.get(id) ?? []) {
      if (depth.has(target)) continue;
      depth.set(target, (depth.get(id) ?? 0) + 1);
      queue.push(target);
    }
  }

  const fallbackDepth = Math.max(0, ...depth.values()) + 1;
  for (const node of nodes) {
    if (!depth.has(node.id)) depth.set(node.id, fallbackDepth);
  }

  const groups = new Map<number, GraphCanvasNode[]>();
  for (const node of nodes) {
    const nodeDepth = depth.get(node.id) ?? fallbackDepth;
    if (!groups.has(nodeDepth)) groups.set(nodeDepth, []);
    groups.get(nodeDepth)?.push(node);
  }

  const orderedDepths = [...groups.keys()].sort((a, b) => a - b);
  const layout = new Map<string, Omit<LayoutNode, 'node_id' | 'pinned'>>();
  orderedDepths.forEach((nodeDepth, depthIndex) => {
    const group = groups.get(nodeDepth) ?? [];
    const top = orderedDepths.length === 1 ? 50 : 18 + (depthIndex * 64) / (orderedDepths.length - 1);
    group.forEach((node, index) => {
      const left = group.length === 1 ? 50 : 26 + (index * 48) / (group.length - 1);
      layout.set(node.id, positionFromPercent(left, top));
    });
  });

  return layout;
}

function buildRadialLayout(nodes: GraphCanvasNode[]) {
  const layout = new Map<string, Omit<LayoutNode, 'node_id' | 'pinned'>>();
  if (nodes.length === 0) return layout;
  if (nodes.length === 1) {
    layout.set(nodes[0].id, positionFromPercent(50, 50));
    return layout;
  }

  nodes.forEach((node, index) => {
    const angle = -Math.PI / 2 + (index * Math.PI * 2) / nodes.length;
    const left = 50 + Math.cos(angle) * 26;
    const top = 50 + Math.sin(angle) * 26;
    layout.set(node.id, positionFromPercent(left, top));
  });

  return layout;
}

function normalizeLayoutOverrides(value: GraphLayoutOptions['layoutOverrides']) {
  const entries = value instanceof Map ? [...value.entries()] : Object.entries(value ?? {});
  const overrides = new Map<string, Omit<LayoutNode, 'node_id' | 'pinned'>>();
  for (const [nodeId, position] of entries) {
    const normalized = normalizeLayoutPosition(position);
    if (normalized) overrides.set(nodeId, normalized);
  }
  return overrides;
}

function positionFromXY(x: number, y: number) {
  return positionFromPercent((x / CANVAS_WIDTH) * 100, (y / CANVAS_HEIGHT) * 100);
}

function positionFromPercent(left: number, top: number) {
  const safeLeft = clamp(left, 4, 96);
  const safeTop = clamp(top, 8, 92);
  return {
    x: round((safeLeft / 100) * CANVAS_WIDTH),
    y: round((safeTop / 100) * CANVAS_HEIGHT),
    left: round(safeLeft),
    top: round(safeTop)
  };
}

function clamp(value: number, min: number, max: number) {
  return Math.max(min, Math.min(max, Number(value)));
}

function round(value: number) {
  return Math.round(value * 100) / 100;
}

function safeArray<T>(value: T[] | undefined | null): T[] {
  return Array.isArray(value) ? value : [];
}
