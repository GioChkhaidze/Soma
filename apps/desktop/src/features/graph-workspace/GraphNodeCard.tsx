import { memo } from 'react';

import { Handle, Position, type NodeProps } from '@xyflow/react';

import type { SomaGraphFlowNode } from './reactFlowAdapter';

export const GraphNodeCard = memo(function GraphNodeCard({
  data,
  selected
}: NodeProps<SomaGraphFlowNode>) {
  const isSelected = selected || data.selected;
  const stateClasses = [
    isSelected ? 'isSelected' : '',
    data.selectionRole === 'neighbor' ? 'isNeighbor' : '',
    data.selectionRole === 'dimmed' ? 'isDimmed' : '',
    data.pinned ? 'isPinned' : '',
    data.focused ? 'isFocused' : ''
  ].join(' ');
  const showFarLabel = isSelected || data.selectionRole === 'neighbor' || data.focused || data.pinned || data.prominent;

  if (data.detailLevel === 'far') {
    return (
      <article
        className={`graphNode graphNode--far ${stateClasses} ${showFarLabel ? 'showsLabel' : ''}`}
        aria-label={`${data.title}, ${data.graphType}`}
        aria-selected={isSelected}
        data-tooltip={data.title}
      >
        <GraphNodeHandles />
        <span className="graphNodeDot" aria-hidden="true" />
        <span className="graphNodeFarLabel">{data.title}</span>
      </article>
    );
  }

  if (data.detailLevel === 'mid') {
    return (
      <article
        className={`graphNode graphNode--mid ${stateClasses}`}
        aria-label={`${data.title}, ${data.graphType}`}
        aria-selected={isSelected}
        data-tooltip={data.title}
      >
        <GraphNodeHandles />
        <strong>{data.title}</strong>
        <button
          type="button"
          className={`nodeFocusToggle ${data.focused ? 'isActive' : ''}`}
          aria-label={`${data.focused ? 'Remove from' : 'Add to'} context: ${data.title}`}
          aria-pressed={data.focused}
          onClick={(event) => {
            event.stopPropagation();
            data.onToggleFocus?.(data.id);
          }}
        >
          {data.focused ? <CheckIcon /> : '+'}
        </button>
      </article>
    );
  }

  return (
    <article
      className={`graphNode graphNode--near ${stateClasses}`}
      aria-label={`${data.title}, ${data.graphType}`}
      aria-selected={isSelected}
      data-tooltip={data.title}
    >
      <GraphNodeHandles />
      <div className="graphNodeTopline">
        <span className="nodeType">{data.graphType}</span>
        <button
          type="button"
          className={`nodeFocusToggle ${data.focused ? 'isActive' : ''}`}
          aria-label={`${data.focused ? 'Remove from' : 'Add to'} context: ${data.title}`}
          aria-pressed={data.focused}
          onClick={(event) => {
            event.stopPropagation();
            data.onToggleFocus?.(data.id);
          }}
        >
          {data.focused ? <CheckIcon /> : '+'}
        </button>
      </div>
      <strong>{data.title}</strong>
      <span>{data.preview}</span>
    </article>
  );
});

function GraphNodeHandles() {
  return (
    <>
      <Handle id="target-top" className="graphNodeHandle" type="target" position={Position.Top} />
      <Handle id="target-right" className="graphNodeHandle" type="target" position={Position.Right} />
      <Handle id="target-bottom" className="graphNodeHandle" type="target" position={Position.Bottom} />
      <Handle id="target-left" className="graphNodeHandle" type="target" position={Position.Left} />
      <Handle id="source-top" className="graphNodeHandle" type="source" position={Position.Top} />
      <Handle id="source-right" className="graphNodeHandle" type="source" position={Position.Right} />
      <Handle id="source-bottom" className="graphNodeHandle" type="source" position={Position.Bottom} />
      <Handle id="source-left" className="graphNodeHandle" type="source" position={Position.Left} />
    </>
  );
}

function CheckIcon() {
  return (
    <svg className="nodeFocusCheckIcon" viewBox="0 0 16 16" aria-hidden="true">
      <path d="M3.5 8.2 6.6 11.2 12.8 4.8" />
    </svg>
  );
}
