import { useCallback, useEffect, useRef, useState } from 'react';
import { getDocument, GlobalWorkerOptions, type PDFDocumentLoadingTask, type PDFDocumentProxy } from 'pdfjs-dist';
import { EventBus, PDFLinkService, PDFViewer } from 'pdfjs-dist/web/pdf_viewer.mjs';
import workerUrl from 'pdfjs-dist/build/pdf.worker.min.mjs?url';

import type { SourceReadingContext } from '../../../../../packages/contracts/src';
import './paper-reader.css';

GlobalWorkerOptions.workerSrc = workerUrl;

const WHEEL_ZOOM_SENSITIVITY = 0.0015;
const WHEEL_ZOOM_DRAWING_DELAY = 250;

type PaperReaderProps = {
  file: File;
  onContextChange: (context: SourceReadingContext | null) => void;
  onClose: () => void;
};

type ExtractedPageText = {
  file: File;
  document: PDFDocumentProxy;
  pageNumber: number;
  text: string;
};

export function PaperReader({ file, onContextChange, onClose }: PaperReaderProps) {
  const containerRef = useRef<HTMLDivElement | null>(null);
  const viewerElementRef = useRef<HTMLDivElement | null>(null);
  const viewerRef = useRef<PDFViewer | null>(null);
  const pageNumberRef = useRef(1);
  const [pdfDocument, setPdfDocument] = useState<PDFDocumentProxy | null>(null);
  const [documentTitle, setDocumentTitle] = useState(file.name);
  const [pageNumber, setPageNumber] = useState(1);
  const [pageCount, setPageCount] = useState(0);
  const [pageText, setPageText] = useState<ExtractedPageText | null>(null);
  const [scale, setScale] = useState(1);
  const [selectedText, setSelectedText] = useState('');
  const [selectionPageNumber, setSelectionPageNumber] = useState<number | null>(null);
  const [error, setError] = useState<string | null>(null);
  const fileNameAddsContext = normalizedPaperTitle(documentTitle) !== normalizedPaperTitle(file.name);

  useEffect(() => {
    pageNumberRef.current = pageNumber;
  }, [pageNumber]);

  useEffect(() => {
    const container = containerRef.current;
    const viewerElement = viewerElementRef.current;
    if (!container || !viewerElement) return undefined;

    let cancelled = false;
    let loadingTask: PDFDocumentLoadingTask | null = null;
    const eventBus = new EventBus();
    const linkService = new PDFLinkService({ eventBus });
    const viewer = new PDFViewer({
      container,
      viewer: viewerElement,
      eventBus,
      linkService,
      removePageBorders: true,
      enableSelectionRendering: true,
      supportsPinchToZoom: true
    });
    linkService.setViewer(viewer);
    viewerRef.current = viewer;
    pageNumberRef.current = 1;
    onContextChange(null);
    setPdfDocument(null);
    setDocumentTitle(file.name);
    setPageNumber(1);
    setPageCount(0);
    setPageText(null);
    setSelectedText('');
    setSelectionPageNumber(null);
    setError(null);

    const handlePagesInit = () => {
      viewer.currentScaleValue = 'page-width';
      setScale(viewer.currentScale);
    };
    const handlePageChanging = ({ pageNumber: nextPage }: { pageNumber: number }) => {
      onContextChange(null);
      setPageNumber(nextPage);
    };
    const handleScaleChanging = ({ scale: nextScale }: { scale: number }) => {
      setScale(nextScale);
    };
    eventBus.on('pagesinit', handlePagesInit);
    eventBus.on('pagechanging', handlePageChanging);
    eventBus.on('scalechanging', handleScaleChanging);

    void file.arrayBuffer()
      .then((data) => {
        if (cancelled) return null;
        loadingTask = getDocument({ data });
        return loadingTask.promise;
      })
      .then(async (document) => {
        if (!document || cancelled) return;
        setPdfDocument(document);
        setPageCount(document.numPages);
        setError(null);
        viewer.setDocument(document);
        linkService.setDocument(document);
        const metadata = await document.getMetadata().catch(() => null);
        const info = metadata?.info as Record<string, unknown> | undefined;
        const title = typeof info?.Title === 'string' ? info.Title.trim() : '';
        if (!cancelled && title) setDocumentTitle(title);
      })
      .catch((loadError: unknown) => {
        if (!cancelled) {
          setError(loadError instanceof Error ? loadError.message : 'Soma could not open this PDF.');
        }
      });

    return () => {
      cancelled = true;
      eventBus.off('pagesinit', handlePagesInit);
      eventBus.off('pagechanging', handlePageChanging);
      eventBus.off('scalechanging', handleScaleChanging);
      viewer.cleanup();
      viewerRef.current = null;
      setPdfDocument(null);
      void loadingTask?.destroy();
    };
  }, [file, onContextChange]);

  useEffect(() => {
    let cancelled = false;
    if (!pdfDocument) {
      setPageText(null);
      return undefined;
    }
    void pdfDocument.getPage(pageNumber)
      .then((page) => page.getTextContent())
      .then((content) => {
        if (!cancelled) {
          setPageText({ file, document: pdfDocument, pageNumber, text: textFromPageContent(content.items) });
        }
      })
      .catch(() => {
        if (!cancelled) setPageText({ file, document: pdfDocument, pageNumber, text: '' });
      });
    return () => {
      cancelled = true;
    };
  }, [file, pdfDocument, pageNumber]);

  useEffect(() => {
    const container = containerRef.current;
    if (!container) return undefined;
    const captureSelection = () => {
      const selection = window.getSelection();
      const anchor = selection?.anchorNode;
      const focus = selection?.focusNode;
      if (!selection || selection.isCollapsed || !anchor || !focus) return;
      if (!container.contains(anchor) || !container.contains(focus)) return;
      const text = truncateCharacters(normalizeText(selection.toString()), 6_000);
      if (!text) return;
      const anchorElement = anchor instanceof Element ? anchor : anchor.parentElement;
      const pageElement = anchorElement?.closest<HTMLElement>('.page');
      setSelectedText(text);
      setSelectionPageNumber(Number(pageElement?.dataset.pageNumber) || pageNumberRef.current);
    };
    document.addEventListener('selectionchange', captureSelection);
    return () => document.removeEventListener('selectionchange', captureSelection);
  }, []);

  useEffect(() => {
    if (
      !pdfDocument
      || pageCount === 0
      || !pageText
      || pageText.file !== file
      || pageText.document !== pdfDocument
      || pageText.pageNumber !== pageNumber
    ) {
      onContextChange(null);
      return;
    }
    onContextChange({
      kind: 'pdf',
      document_name: file.name,
      page_number: pageNumber,
      page_count: pageCount,
      page_text: truncateCharacters(pageText.text, 12_000),
      selected_text: selectedText || undefined,
      selection_page_number: selectionPageNumber ?? undefined
    });
  }, [
    file,
    onContextChange,
    pageCount,
    pageNumber,
    pageText,
    pdfDocument,
    selectedText,
    selectionPageNumber
  ]);

  const clearSelection = useCallback(() => {
    window.getSelection()?.removeAllRanges();
    viewerRef.current?.clearSelection();
    setSelectedText('');
    setSelectionPageNumber(null);
  }, []);

  const zoomBySteps = useCallback((steps: number) => {
    const viewer = viewerRef.current;
    const container = containerRef.current;
    if (!viewer || !container) return;
    viewer.updateScale({
      steps,
      drawingDelay: 150,
      origin: viewportCenter(container)
    });
  }, []);

  const fitToWidth = useCallback(() => {
    if (viewerRef.current) viewerRef.current.currentScaleValue = 'page-width';
  }, []);

  useEffect(() => {
    const container = containerRef.current;
    if (!container) return undefined;

    let pendingDelta = 0;
    let pendingOrigin: [number, number] = viewportCenter(container);
    let frameId: number | null = null;

    const applyWheelZoom = () => {
      frameId = null;
      const viewer = viewerRef.current;
      if (!viewer || pendingDelta === 0) return;
      const boundedDelta = Math.max(-160, Math.min(160, pendingDelta));
      pendingDelta = 0;
      viewer.updateScale({
        scaleFactor: Math.exp(-boundedDelta * WHEEL_ZOOM_SENSITIVITY),
        drawingDelay: WHEEL_ZOOM_DRAWING_DELAY,
        origin: pendingOrigin
      });
    };

    const handleWheel = (event: WheelEvent) => {
      if ((!event.ctrlKey && !event.metaKey) || !viewerRef.current) return;
      event.preventDefault();
      pendingDelta += wheelDeltaInPixels(event, container);
      pendingOrigin = [event.clientX, event.clientY];
      frameId ??= window.requestAnimationFrame(applyWheelZoom);
    };

    container.addEventListener('wheel', handleWheel, { passive: false });
    return () => {
      container.removeEventListener('wheel', handleWheel);
      if (frameId !== null) window.cancelAnimationFrame(frameId);
    };
  }, []);

  useEffect(() => {
    const handleShortcut = (event: globalThis.KeyboardEvent) => {
      const container = containerRef.current;
      if (!container || container.closest('[inert]')) return;
      if ((!event.ctrlKey && !event.metaKey) || event.altKey) return;
      if (event.key === '=' || event.key === '+' || event.code === 'NumpadAdd') {
        event.preventDefault();
        zoomBySteps(1);
      } else if (event.key === '-' || event.code === 'NumpadSubtract') {
        event.preventDefault();
        zoomBySteps(-1);
      } else if (event.key === '0' || event.code === 'Numpad0') {
        event.preventDefault();
        fitToWidth();
      }
    };

    window.addEventListener('keydown', handleShortcut);
    return () => window.removeEventListener('keydown', handleShortcut);
  }, [fitToWidth, zoomBySteps]);

  return (
    <section className="paperReader" aria-label={`Paper reader: ${documentTitle}`}>
      <header className="paperToolbar">
        <div className="paperIdentity">
          <PaperIcon />
          <div>
            <strong title={documentTitle}>{documentTitle}</strong>
            {fileNameAddsContext ? <span>{file.name}</span> : null}
          </div>
        </div>

        <div className="paperTools" role="toolbar" aria-label="Paper controls">
          <button type="button" onClick={() => viewerRef.current?.previousPage()} disabled={pageNumber <= 1}>
            <PreviousIcon />
            <span className="srOnly">Previous page</span>
          </button>
          <span className="paperPageCount" aria-live="polite">{pageNumber} / {pageCount || '—'}</span>
          <button
            type="button"
            onClick={() => viewerRef.current?.nextPage()}
            disabled={pageCount === 0 || pageNumber >= pageCount}
          >
            <NextIcon />
            <span className="srOnly">Next page</span>
          </button>
          <span className="paperToolDivider" aria-hidden="true" />
          <button type="button" onClick={() => zoomBySteps(-1)} title="Zoom out (Ctrl+-)">
            <ZoomOutIcon />
            <span className="srOnly">Zoom out</span>
          </button>
          <span className="paperZoom">{Math.round(scale * 100)}%</span>
          <button type="button" onClick={() => zoomBySteps(1)} title="Zoom in (Ctrl++)">
            <ZoomInIcon />
            <span className="srOnly">Zoom in</span>
          </button>
          <button type="button" onClick={fitToWidth} title="Fit to width (Ctrl+0)">
            Fit
          </button>
        </div>

        <div className="paperToolbarEnd">
          {selectedText ? (
            <button
              className="paperSelection"
              type="button"
              onClick={clearSelection}
              title={selectedText}
              aria-label={`Clear selection from page ${selectionPageNumber ?? pageNumber}`}
            >
              Selection · p. {selectionPageNumber ?? pageNumber}
              <CloseSmallIcon />
            </button>
          ) : (
            <span className="paperSelectionHint">Select text to ground your question</span>
          )}
          <button className="paperClose" type="button" onClick={onClose} aria-label="Close paper">
            <CloseIcon />
          </button>
        </div>
      </header>

      <div className="paperStage">
        <div className="paperViewport" ref={containerRef} tabIndex={0}>
          <div className="pdfViewer" ref={viewerElementRef} />
          {!pdfDocument && !error ? (
            <div className="paperLoading" role="status"><span />Opening paper</div>
          ) : null}
          {error ? (
            <div className="paperError" role="alert">
              <strong>Could not open this paper</strong>
              <span>{error}</span>
            </div>
          ) : null}
        </div>
      </div>
    </section>
  );
}

function viewportCenter(container: HTMLElement): [number, number] {
  const rect = container.getBoundingClientRect();
  return [rect.left + rect.width / 2, rect.top + rect.height / 2];
}

function wheelDeltaInPixels(event: WheelEvent, container: HTMLElement) {
  if (event.deltaMode === WheelEvent.DOM_DELTA_LINE) return event.deltaY * 16;
  if (event.deltaMode === WheelEvent.DOM_DELTA_PAGE) return event.deltaY * container.clientHeight;
  return event.deltaY;
}

function textFromPageContent(items: Array<unknown>) {
  return normalizeText(items.flatMap((item) => {
    if (!item || typeof item !== 'object' || !('str' in item)) return [];
    const textItem = item as { str?: unknown; hasEOL?: unknown };
    const text = typeof textItem.str === 'string' ? textItem.str : '';
    return textItem.hasEOL ? [`${text}\n`] : [text];
  }).join(' '));
}

function normalizeText(value: string) {
  return value.replace(/[ \t]+\n/g, '\n').replace(/\s+/g, ' ').trim();
}

function truncateCharacters(value: string, maxCharacters: number) {
  return [...value].slice(0, maxCharacters).join('');
}

function normalizedPaperTitle(value: string) {
  return value.trim().replace(/\.pdf$/i, '').toLowerCase();
}

function PaperIcon() {
  return <svg viewBox="0 0 24 24" aria-hidden="true"><path d="M6 3.5h8l4 4v13H6zM14 3.5v4h4M9 12h6M9 15.5h6" /></svg>;
}

function PreviousIcon() {
  return <svg viewBox="0 0 24 24" aria-hidden="true"><path d="M14.5 7l-5 5 5 5" /></svg>;
}

function NextIcon() {
  return <svg viewBox="0 0 24 24" aria-hidden="true"><path d="M9.5 7l5 5-5 5" /></svg>;
}

function ZoomOutIcon() {
  return (
    <svg viewBox="0 0 24 24" aria-hidden="true">
      <circle cx="10.5" cy="10.5" r="5.5" />
      <path d="M15 15l4 4M8 10.5h5" />
    </svg>
  );
}

function ZoomInIcon() {
  return (
    <svg viewBox="0 0 24 24" aria-hidden="true">
      <circle cx="10.5" cy="10.5" r="5.5" />
      <path d="M15 15l4 4M10.5 8v5M8 10.5h5" />
    </svg>
  );
}

function CloseIcon() {
  return <svg viewBox="0 0 24 24" aria-hidden="true"><path d="M7 7l10 10M17 7L7 17" /></svg>;
}

function CloseSmallIcon() {
  return <svg viewBox="0 0 20 20" aria-hidden="true"><path d="M6 6l8 8M14 6l-8 8" /></svg>;
}
