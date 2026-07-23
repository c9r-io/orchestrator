#!/usr/bin/env node
/**
 * sync-docs.mjs — Single-source sync for guide documentation.
 *
 * Copies docs/guide/NN-slug.md  →  site/en/guide/slug.md
 *        docs/guide/zh/NN-slug.md  →  site/zh/guide/slug.md
 *
 * Transformations applied:
 *   1. Strip numbered prefix from filename (01-quickstart.md → quickstart.md)
 *   2. Rewrite internal links so they resolve inside the published site:
 *      - links to another synced guide file  → site-relative path (prefix stripped)
 *      - links to files outside the guide set → absolute GitHub blob URL
 *        (design_doc/qa/security/fixtures/deploy/showcases are not published)
 *
 * Skips README.md (VitePress sidebar provides navigation).
 */

import { readdirSync, readFileSync, writeFileSync, mkdirSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const ROOT = path.join(dirname(), "..");
function dirname() {
  return path.dirname(fileURLToPath(import.meta.url));
}

const GITHUB_BLOB = "https://github.com/c9r-io/orchestrator/blob/main";

const SOURCES = [
  { srcRepoDir: "docs/guide", lang: "en" },
  { srcRepoDir: "docs/guide/zh", lang: "zh" },
];

/** Strip leading NN- prefix from a filename. */
function stripPrefix(name) {
  return name.replace(/^\d{2}-/, "");
}

/** Site directory (repo-relative, posix) that a synced guide file lands in. */
function siteGuideDir(lang) {
  return `site/${lang}/guide`;
}

/**
 * Rewrite one markdown link target so it resolves in the published site.
 * `srcRepoDir` is the posix repo-relative directory of the source file.
 */
function rewriteTarget(target, srcRepoDir, lang) {
  const hashIdx = target.indexOf("#");
  const linkPath = hashIdx === -1 ? target : target.slice(0, hashIdx);
  const anchor = hashIdx === -1 ? "" : target.slice(hashIdx);

  // External links, mail, and pure anchors pass through untouched.
  if (!linkPath || /^(https?:|mailto:|tel:|data:|\/\/)/.test(linkPath)) {
    return target;
  }
  // Site-absolute VitePress routes (/en/...) already resolve; leave them.
  if (linkPath.startsWith("/")) {
    return target;
  }

  const repoRel = path.posix.normalize(path.posix.join(srcRepoDir, linkPath));
  // Escapes the repo — cannot classify, leave as-is.
  if (repoRel.startsWith("..")) {
    return target;
  }

  // Classify the resolved target.
  let targetLang = null;
  if (repoRel.startsWith("docs/guide/zh/")) {
    targetLang = "zh";
  } else if (
    repoRel.startsWith("docs/guide/") &&
    !repoRel.slice("docs/guide/".length).includes("/")
  ) {
    // A top-level (English) guide file, e.g. docs/guide/agent-driver-model.md.
    targetLang = "en";
  }

  if (targetLang === null) {
    // Outside the synced guide set → point at the real file on GitHub.
    return `${GITHUB_BLOB}/${repoRel}${anchor}`;
  }

  // Another synced guide file: compute a site-relative link, prefix stripped.
  const slug = stripPrefix(path.posix.basename(repoRel));
  const fromDir = siteGuideDir(lang);
  const toPath = `${siteGuideDir(targetLang)}/${slug}`;
  let rel = path.posix.relative(fromDir, toPath);
  if (!rel.startsWith(".")) rel = `./${rel}`;
  return `${rel}${anchor}`;
}

/** Rewrite every markdown link in a document. */
function rewriteLinks(content, srcRepoDir, lang) {
  return content.replace(
    /(\]\()([^)]+)(\))/g,
    (_, open, target, close) =>
      `${open}${rewriteTarget(target.trim(), srcRepoDir, lang)}${close}`
  );
}

let count = 0;

for (const { srcRepoDir, lang } of SOURCES) {
  const src = path.join(ROOT, srcRepoDir);
  const dest = path.join(ROOT, siteGuideDir(lang));
  mkdirSync(dest, { recursive: true });

  const files = readdirSync(src).filter(
    (f) => f.endsWith(".md") && f !== "README.md" && !f.startsWith(".")
  );

  for (const file of files) {
    const raw = readFileSync(path.join(src, file), "utf8");
    const transformed = rewriteLinks(raw, srcRepoDir, lang);
    writeFileSync(path.join(dest, stripPrefix(file)), transformed, "utf8");
    count++;
  }
}

console.log(`[sync-docs] Synced ${count} guide files.`);
