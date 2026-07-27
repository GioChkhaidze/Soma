import { readdir, readFile } from "node:fs/promises";
import { extname, join, relative } from "node:path";
import process from "node:process";

const MAX_LINE_LENGTH = 120;
const SOURCE_ROOTS = [".github", "apps", "crates", "docs", "packages", "scripts", "test"];
const ROOT_FILES = [
  ".editorconfig",
  "README.md",
  "index.html",
  "package.json",
  "playwright.config.ts",
  "rust-toolchain.toml",
  "rustfmt.toml",
  "tsconfig.json",
  "vite.config.js",
];
const SOURCE_EXTENSIONS = new Set([
  ".css",
  ".html",
  ".js",
  ".json",
  ".md",
  ".mjs",
  ".rs",
  ".svg",
  ".toml",
  ".ts",
  ".tsx",
  ".yaml",
  ".yml",
]);
const EXCLUDED_DIRECTORIES = new Set([
  "build",
  "coverage",
  "dist",
  "gen",
  "generated",
  "node_modules",
  "Soma Workspace",
  "target",
]);
const EXCLUDED_FILES = new Set(["Cargo.lock", "package-lock.json"]);

async function collectSourceFiles(directory) {
  const entries = await readdir(directory, { withFileTypes: true });
  const files = [];

  for (const entry of entries) {
    const path = join(directory, entry.name);
    if (entry.isDirectory()) {
      if (!EXCLUDED_DIRECTORIES.has(entry.name)) {
        files.push(...await collectSourceFiles(path));
      }
    } else if (SOURCE_EXTENSIONS.has(extname(entry.name)) && !EXCLUDED_FILES.has(entry.name)) {
      files.push(path);
    }
  }

  return files;
}

const files = [
  ...ROOT_FILES,
  ...(
    await Promise.all(SOURCE_ROOTS.map((root) => collectSourceFiles(root)))
  ).flat(),
].sort();
const violations = [];

for (const file of files) {
  const lines = (await readFile(file, "utf8")).split(/\r?\n/u);

  lines.forEach((line, index) => {
    const lineNumber = index + 1;
    if (line.includes("\t")) {
      violations.push(`${relative(".", file)}:${lineNumber}: tab character`);
    }
    if (line.length > MAX_LINE_LENGTH) {
      violations.push(`${relative(".", file)}:${lineNumber}: ${line.length} columns (maximum ${MAX_LINE_LENGTH})`);
    }
  });
}

if (violations.length > 0) {
  console.error(`Source style check failed with ${violations.length} violation(s):`);
  console.error(violations.join("\n"));
  process.exitCode = 1;
} else {
  console.log(`Source style check passed for ${files.length} maintained files.`);
}
