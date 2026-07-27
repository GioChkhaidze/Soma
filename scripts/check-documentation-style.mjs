import { access, readdir, readFile } from "node:fs/promises";
import { dirname, extname, join, relative, resolve } from "node:path";
import process from "node:process";

const MAX_SENTENCE_WORDS = 25;
const DOCUMENT_EXTENSIONS = new Set([".md", ".txt"]);
const CONTRACTIONS = [
  /\b(?:can't|cannot've|couldn't|didn't|doesn't|don't|hasn't|haven't)\b/iu,
  /\b(?:isn't|mustn't|shouldn't|wasn't|weren't|won't|wouldn't)\b/iu,
  /\b(?:I'm|you're|we're|they're|it's|that's|there's|here's|what's|who's)\b/iu,
  /\b(?:I've|you've|we've|they've|I'd|you'd|we'd|they'd|I'll|you'll|we'll|they'll)\b/iu,
];
const BRITISH_SPELLING = /\b(?:colour|colours|favour|favourites?|optimise|organise|organisation|centre|licence)\b/iu;
const CORRUPT_TEXT = /(?:â€|â†|â”|Â|Ã|�)/u;

async function collectDocuments(directory) {
  const entries = await readdir(directory, { withFileTypes: true });
  const files = [];

  for (const entry of entries) {
    const path = join(directory, entry.name);
    if (entry.isDirectory()) {
      files.push(...await collectDocuments(path));
    } else if (DOCUMENT_EXTENSIONS.has(extname(entry.name))) {
      files.push(path);
    }
  }

  return files;
}

function plainText(markdown) {
  return markdown
    .replace(/!\[([^\]]*)\]\([^)]+\)/gu, "$1")
    .replace(/\[([^\]]+)\]\([^)]+\)/gu, "$1")
    .replace(/https?:\/\/\S+/gu, "link")
    .replace(/`[^`]+`/gu, "technical-term")
    .replace(/<[^>]+>/gu, " ")
    .replace(/^\s*(?:[-*+]|\d+\.)\s+/u, "")
    .replace(/\*\*|__|[*_~]/gu, "")
    .replace(/\s+/gu, " ")
    .trim();
}

function sentenceWordCount(sentence) {
  return sentence.match(/[\p{L}\p{N}]+(?:[-'][\p{L}\p{N}]+)*/gu)?.length ?? 0;
}

const files = ["README.md", ...await collectDocuments("docs")].sort();
const violations = [];

for (const file of files) {
  const content = await readFile(file, "utf8");
  const lines = content.split(/\r?\n/u);
  let inFence = false;
  let paragraph = [];
  let paragraphStart = 1;

  for (const match of content.matchAll(/!?\[[^\]]*\]\(([^)]+)\)/gu)) {
    const target = match[1].trim().replace(/^<|>$/gu, "").split("#", 1)[0];
    if (!target || /^(?:[a-z]+:|#)/iu.test(target)) {
      continue;
    }
    try {
      await access(resolve(dirname(file), decodeURIComponent(target)));
    } catch {
      violations.push(`${relative(".", file)}: broken local link to ${target}`);
    }
  }

  const checkParagraph = () => {
    const text = plainText(paragraph.join(" "));
    paragraph = [];

    if (!text) {
      return;
    }

    if (text.includes(";")) {
      violations.push(`${relative(".", file)}:${paragraphStart}: semicolon in prose`);
    }
    if (CONTRACTIONS.some((pattern) => pattern.test(text))) {
      violations.push(`${relative(".", file)}:${paragraphStart}: contraction in prose`);
    }
    if (BRITISH_SPELLING.test(text)) {
      violations.push(`${relative(".", file)}:${paragraphStart}: British spelling in prose`);
    }
    if (CORRUPT_TEXT.test(text)) {
      violations.push(`${relative(".", file)}:${paragraphStart}: corrupted text encoding`);
    }

    for (const sentence of text.match(/[^.!?]+[.!?]+/gu) ?? []) {
      const wordCount = sentenceWordCount(sentence);
      if (wordCount > MAX_SENTENCE_WORDS) {
        violations.push(
          `${relative(".", file)}:${paragraphStart}: ${wordCount}-word sentence (maximum ${MAX_SENTENCE_WORDS})`,
        );
      }
    }
  };

  lines.forEach((line, index) => {
    const lineNumber = index + 1;
    if (/^\s*```/u.test(line)) {
      checkParagraph();
      inFence = !inFence;
      return;
    }
    if (inFence || /^\s*(?:#|<|\|)/u.test(line)) {
      checkParagraph();
      return;
    }
    if (/^\s*(?:[-*+]|\d+\.)\s+/u.test(line)) {
      checkParagraph();
      paragraphStart = lineNumber;
      paragraph = [line];
      checkParagraph();
      return;
    }
    if (!line.trim()) {
      checkParagraph();
      return;
    }
    if (paragraph.length === 0) {
      paragraphStart = lineNumber;
    }
    paragraph.push(line);
  });

  checkParagraph();
}

if (violations.length > 0) {
  console.error(`Documentation style check failed with ${violations.length} violation(s):`);
  console.error(violations.join("\n"));
  process.exitCode = 1;
} else {
  console.log(`Documentation style check passed for ${files.length} files.`);
}
