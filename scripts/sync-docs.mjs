#!/usr/bin/env node
/**
 * sync-docs.mjs — Single-source sync for guide documentation.
 *
 * Copies docs/guide/NN-slug.md  →  site/en/guide/slug.md
 *        docs/guide/zh/NN-slug.md  →  site/zh/guide/slug.md
 *
 * Transformations applied:
 *   1. Strip numbered prefix from filename (01-quickstart.md → quickstart.md)
 *   2. Rewrite internal links: (NN-slug.md) → (slug.md)
 *
 * Skips README.md (VitePress sidebar provides navigation).
 */

import { readdirSync, readFileSync, writeFileSync, mkdirSync } from "node:fs";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";

const ROOT = join(dirname(fileURLToPath(import.meta.url)), "..");

const SOURCES = [
  { src: join(ROOT, "docs/guide"), dest: join(ROOT, "site/en/guide") },
  { src: join(ROOT, "docs/guide/zh"), dest: join(ROOT, "site/zh/guide") },
];

/** Strip leading NN- prefix from a filename. */
function stripPrefix(name) {
  return name.replace(/^\d{2}-/, "");
}

/** Rewrite numbered internal links: (NN-slug.md) → (slug.md) */
function rewriteLinks(content) {
  return content.replace(
    /\((\d{2}-[^)]+\.md(?:#[^)]*)?)\)/g,
    (_, ref) => `(${stripPrefix(ref)})`
  );
}

let count = 0;

for (const { src, dest } of SOURCES) {
  mkdirSync(dest, { recursive: true });

  const files = readdirSync(src).filter(
    (f) => f.endsWith(".md") && f !== "README.md" && !f.startsWith(".")
  );

  for (const file of files) {
    // Skip subdirectories (e.g. docs/guide/zh/ handled as separate source)
    const srcPath = join(src, file);
    const raw = readFileSync(srcPath, "utf8");
    const transformed = rewriteLinks(raw);
    const destName = stripPrefix(file);
    const destPath = join(dest, destName);
    writeFileSync(destPath, transformed, "utf8");
    count++;
  }
}

console.log(`[sync-docs] Synced ${count} guide files.`);
