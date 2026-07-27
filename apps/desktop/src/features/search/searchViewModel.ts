type TextHighlightPart = {
  text: string;
  match: boolean;
};

export function clampSearchIndex(index: number, resultCount: number) {
  if (resultCount <= 0) return 0;
  return Math.min(Math.max(index, 0), resultCount - 1);
}

export function nextSearchIndex(index: number, resultCount: number, direction: 1 | -1) {
  if (resultCount <= 0) return 0;
  return (clampSearchIndex(index, resultCount) + direction + resultCount) % resultCount;
}

export function resultCountLabel(resultCount: number) {
  return `${resultCount} ${resultCount === 1 ? 'result' : 'results'}`;
}

export function highlightedTextParts(text: string, query: string): TextHighlightPart[] {
  const needle = query.trim();
  if (!needle) return [{ text, match: false }];

  const normalizedText = text.toLocaleLowerCase();
  const normalizedNeedle = needle.toLocaleLowerCase();
  const parts: TextHighlightPart[] = [];
  let cursor = 0;

  while (cursor < text.length) {
    const matchIndex = normalizedText.indexOf(normalizedNeedle, cursor);
    if (matchIndex === -1) break;

    if (matchIndex > cursor) {
      parts.push({ text: text.slice(cursor, matchIndex), match: false });
    }

    const matchEnd = matchIndex + needle.length;
    parts.push({ text: text.slice(matchIndex, matchEnd), match: true });
    cursor = matchEnd;
  }

  if (cursor < text.length) {
    parts.push({ text: text.slice(cursor), match: false });
  }

  return parts.length > 0 ? parts : [{ text, match: false }];
}
