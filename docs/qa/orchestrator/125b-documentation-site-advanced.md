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
5. Verify internal links between chapters work

**Expected:** Guide nav entry is visible, all 7 chapters render, sidebar shows all entries, cross-links work.

### Scenario 7: "Why Orchestrator?" page

> **Status: Deferred** — The `/en/why` page has not been implemented. FR-073 does not include this page in the current site structure. Re-enable this scenario when the page is added.

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

## Checklist

- [ ] S6: Guide navigation
- [ ] S7: "Why Orchestrator?" page *(deferred — page not implemented)*
- [ ] S8: README is concise
- [ ] S9: Cloudflare Pages deployment (manual, post-setup)
