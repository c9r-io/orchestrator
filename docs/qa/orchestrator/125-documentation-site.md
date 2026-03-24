# QA 125: Documentation Site & Landing Page

## FR Reference

FR-073

## Verification Scenarios

### Scenario 1: VitePress builds successfully

**Steps:**
1. `cd site && npm ci && npx vitepress build`

**Expected:** Build completes with no errors, output in `site/.vitepress/dist/`.

### Scenario 2: EN landing page

**Steps:**
1. `cd site && npx vitepress dev`
2. Open `http://localhost:5173/en/`

**Expected:** Hero section with title, tagline, feature cards, install commands, quick start.

### Scenario 3: ZH landing page

**Steps:**
1. Open `http://localhost:5173/zh/`

**Expected:** Chinese translation of the landing page.

### Scenario 4: Language switcher

**Steps:**
1. On EN page, click language switcher in nav
2. Select "中文"

**Expected:** Navigates to ZH version of the current page.

### Scenario 5: Full-text search

**Steps:**
1. Click search icon or press `/`
2. Type "prehook"

**Expected:** Search results show relevant guide chapters in both languages.

### Scenario 6: Guide navigation

**Steps:**
1. Navigate to `/en/guide/quickstart`
2. Use sidebar to navigate through all 7 chapters
3. Verify internal links between chapters work

**Expected:** All 7 chapters render, sidebar shows all entries, cross-links work.

### Scenario 7: "Why Orchestrator?" page

**Steps:**
1. Navigate to `/en/why`
2. Verify comparison table and differentiator sections

**Expected:** Table with 5 competitors, 4 differentiator sections with code examples.

### Scenario 8: README is concise

**Steps:**
1. `wc -l README.md`

**Expected:** Under 100 lines.

### Scenario 9: Cloudflare Pages deployment (manual, post-setup)

**Steps:**
1. Add `CLOUDFLARE_API_TOKEN` and `CLOUDFLARE_ACCOUNT_ID` secrets
2. Push a change to `site/` on main branch
3. Check GitHub Actions docs workflow completes

**Expected:** Site deployed to Cloudflare Pages, accessible via project URL.
