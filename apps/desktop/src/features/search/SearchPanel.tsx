import { useEffect, useMemo, useState, type KeyboardEvent } from 'react';

import type { GraphCanvasNode, GraphCanvasSnapshot } from '../../../../../packages/contracts/src';

import { searchGraphNodeCards } from '../../shared/commands/graphWorkspaceCommands';
import { createSearchRequestOwnership } from './searchRequestOwnership';
import { clampSearchIndex, highlightedTextParts, nextSearchIndex, resultCountLabel } from './searchViewModel';

const SEARCH_RESULT_LIMIT = 5;
const SEARCH_DELAY_MS = 120;

type SearchPanelProps = {
  snapshot: GraphCanvasSnapshot;
  hasWorkspace: boolean;
  onSelectNode: (node: GraphCanvasNode) => void;
};

export function SearchPanel({
  snapshot,
  hasWorkspace,
  onSelectNode
}: SearchPanelProps) {
  const requestOwnership = useMemo(createSearchRequestOwnership, []);
  const [query, setQuery] = useState('');
  const [activeIndex, setActiveIndex] = useState(0);
  const [results, setResults] = useState<GraphCanvasNode[]>([]);
  const [searchState, setSearchState] = useState<'idle' | 'searching' | 'settled' | 'error'>('idle');
  const activeResult = results[activeIndex] ?? null;
  const partialScope = snapshot.is_partial
    && snapshot.total_node_count !== undefined
    && snapshot.total_node_count > snapshot.nodes.length
    ? `Canvas shows ${snapshot.nodes.length} of ${snapshot.total_node_count} nodes. Search covers all.`
    : null;

  useEffect(() => {
    setActiveIndex(0);
  }, [query]);

  useEffect(() => {
    const request = requestOwnership.begin();
    const trimmedQuery = query.trim();
    if (!hasWorkspace || !trimmedQuery) {
      setResults([]);
      setSearchState('idle');
      return () => requestOwnership.cancel(request);
    }

    setResults([]);
    setSearchState('searching');
    const searchTimer = window.setTimeout(() => {
      void searchGraphNodeCards(trimmedQuery, SEARCH_RESULT_LIMIT)
        .then((nextResults) => {
          if (!requestOwnership.owns(request)) return;
          setResults(nextResults);
          setSearchState('settled');
        })
        .catch(() => {
          if (!requestOwnership.owns(request)) return;
          setResults([]);
          setSearchState('error');
        });
    }, SEARCH_DELAY_MS);

    return () => {
      window.clearTimeout(searchTimer);
      requestOwnership.cancel(request);
    };
  }, [hasWorkspace, query, requestOwnership, snapshot]);

  useEffect(() => {
    setActiveIndex((index) => clampSearchIndex(index, results.length));
  }, [results.length]);

  return (
    <section className="sidebarSearch" aria-label="Graph node search">
      <label className="srOnly" htmlFor="graphSearch">Search nodes</label>
      <div className="searchField">
        <SearchIcon />
        <input
          id="graphSearch"
          value={query}
          onChange={(event) => setQuery(event.target.value)}
          onKeyDown={handleKeyDown}
          placeholder="Find in graph"
          autoComplete="off"
          spellCheck={false}
        />
        {query ? (
          <button
            className="searchClear"
            type="button"
            aria-label="Clear search"
            title="Clear search"
            onClick={clearSearch}
          >
            <ClearIcon />
          </button>
        ) : null}
      </div>
      {partialScope ? <p className="searchResultsMeta">{partialScope}</p> : null}
      {query.trim() ? (
        <p className="searchResultsMeta" aria-live="polite">
          {searchState === 'searching' ? 'Searching' : resultCountLabel(results.length)}
        </p>
      ) : null}
      {results.length > 0 ? (
        <div className="searchResults" role="listbox" aria-label="Search results">
          {results.map((node, index) => (
            <button
              key={node.id}
              type="button"
              role="option"
              aria-selected={index === activeIndex}
              className={index === activeIndex ? 'isActive' : ''}
              onClick={() => selectNode(node)}
              onFocus={() => setActiveIndex(index)}
              onMouseEnter={() => setActiveIndex(index)}
            >
              <span className="searchResultMain">
                <span className="searchResultTitle">{renderHighlightedText(node.title, query)}</span>
                <span className="searchResultPreview">{renderHighlightedText(node.preview, query)}</span>
              </span>
              <span className="searchResultType">{node.type}</span>
            </button>
          ))}
        </div>
      ) : (
        query.trim() && searchState === 'settled'
          ? <p className="panelEmpty">No matching nodes.</p>
          : searchState === 'error'
            ? <p className="panelEmpty" role="alert">Search unavailable.</p>
            : null
      )}
    </section>
  );

  function clearSearch() {
    setQuery('');
    setActiveIndex(0);
  }

  function selectNode(node: GraphCanvasNode) {
    onSelectNode(node);
  }

  function handleKeyDown(event: KeyboardEvent<HTMLInputElement>) {
    if (event.key === 'Escape') {
      event.preventDefault();
      clearSearch();
      return;
    }

    if (event.key === 'ArrowDown') {
      event.preventDefault();
      setActiveIndex((index) => nextSearchIndex(index, results.length, 1));
      return;
    }

    if (event.key === 'ArrowUp') {
      event.preventDefault();
      setActiveIndex((index) => nextSearchIndex(index, results.length, -1));
      return;
    }

    if (event.key === 'Enter' && activeResult) {
      event.preventDefault();
      selectNode(activeResult);
    }
  }
}

function renderHighlightedText(text: string, query: string) {
  return highlightedTextParts(text, query).map((part, index) => (
    part.match
      ? <mark key={index}>{part.text}</mark>
      : <span key={index}>{part.text}</span>
  ));
}

function SearchIcon() {
  return (
    <svg className="searchIcon" viewBox="0 0 24 24" aria-hidden="true">
      <circle cx="10.5" cy="10.5" r="5.5" />
      <path d="M15 15l4 4" />
    </svg>
  );
}

function ClearIcon() {
  return (
    <svg className="searchClearIcon" viewBox="0 0 24 24" aria-hidden="true">
      <path d="M7 7l10 10M17 7L7 17" />
    </svg>
  );
}
