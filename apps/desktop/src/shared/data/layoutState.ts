import type { LayoutNode } from '../../../../../packages/contracts/src';

const CANVAS_WIDTH = 1000;
const CANVAS_HEIGHT = 650;

export function layoutNodeFromPosition(
  nodeId: string,
  position: { x: number; y: number },
  pinned: boolean
): LayoutNode {
  const left = clamp((position.x / CANVAS_WIDTH) * 100, 4, 96);
  const top = clamp((position.y / CANVAS_HEIGHT) * 100, 8, 92);

  return {
    node_id: nodeId,
    x: round((left / 100) * CANVAS_WIDTH),
    y: round((top / 100) * CANVAS_HEIGHT),
    left: round(left),
    top: round(top),
    pinned
  };
}

export function upsertLayoutOverride(
  overrides: Record<string, LayoutNode>,
  layoutNode: LayoutNode
): Record<string, LayoutNode> {
  return {
    ...overrides,
    [layoutNode.node_id]: layoutNode
  };
}

export function pinnedNodeIdsWith(nodeIds: string[], nodeId: string, pinned: boolean): string[] {
  if (!pinned) return nodeIds.filter((id) => id !== nodeId);
  return nodeIds.includes(nodeId) ? nodeIds : [...nodeIds, nodeId];
}

function clamp(value: number, min: number, max: number) {
  return Math.max(min, Math.min(max, Number(value)));
}

function round(value: number) {
  return Math.round(value * 100) / 100;
}
