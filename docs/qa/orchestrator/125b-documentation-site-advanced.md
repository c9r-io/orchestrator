# QA 125b: Documentation Site — Advanced Scenarios

> Split from [125-documentation-site.md](125-documentation-site.md) to enforce the 5-scenario cap.

## FR Reference

FR-073

## Verification Scenarios

### Scenario 6: Guide navigation (Entry Visibility)

**Steps:**
1. Open EN landing page (`http://localhost:5173/en/`)
2. Click **Guide** in the top navigation bar
3. Verify the quickstart chapter loads
4. Use sidebar to navigate through all 7 chapters
5. Verify "Next Steps" sections link to other chapters using relative markdown links (e.g. `[title](resource-model.md)`)

**Expected:** Guide nav entry is visible, all 7 chapters render, sidebar shows all entries, "Next Steps" cross-links resolve.

### Scenario 7: "Why Orchestrator?" page

> **Status: Deferred** — The `/en/why` page has not been implemented. FR-073 does not include this page in the current site structure. Re-enable this scenario when the page is added.

**Steps:**
1. Navigate to `/en/why`
2. Verify comparison table and differentiator sections

**Expected:** Table with 5 competitors, 4 differentiator sections with code examples.

### Scenario 8: README is concise

**Steps:**
1. `wc -l README.md`

**Expected:** Under 110 lines.

### Scenario 9: Cloudflare Pages deployment (manual, post-setup)

**Steps:**
1. Add `CLOUDFLARE_API_TOKEN` and `CLOUDFLARE_ACCOUNT_ID` secrets
2. Push a change to `site/` on main branch
3. Check GitHub Actions docs workflow completes

**Expected:** Site deployed to Cloudflare Pages, accessible via project URL.

## Checklist

- [x] S6: Guide navigation — **PASS** — Guide nav visible at `/en/guide/`, 7 chapters configured in sidebar (Quick Start, Resource Model, Workflow Configuration, CEL Prehooks, Advanced Features, Self-Bootstrap, CLI Reference), all chapter files present
- [x] S7: "Why Orchestrator?" page *(deferred — page not implemented)*
- [x] S8: README is concise — **PASS** — README.md has 106 lines, under 110-line threshold
- [x] S9: Cloudflare Pages deployment (manual, post-setup) *(manual — not executed in automated run)*
