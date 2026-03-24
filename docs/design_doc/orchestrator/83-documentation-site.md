# Design Doc 83: Documentation Site & Landing Page

## FR Reference

FR-073: 文档站点与 Landing Page

## Design Decisions

### Framework: VitePress

Selected VitePress over mdBook and Docusaurus:
- Modern, clean default theme without being flashy
- Built-in i18n with locale switcher (EN/ZH)
- Built-in local search (MiniSearch)
- Pure static output — deployable to any static host
- Fast build (~9s for the full site)

### Site Structure

```
site/
├── .vitepress/config.ts   # i18n, nav, sidebar, search config
├── en/
│   ├── index.md           # Landing page (VitePress hero layout)
│   ├── guide/*.md         # Guide chapters (copied from docs/guide/)
│   └── why.md             # Comparison vs Airflow/Prefect/n8n/Dagger
├── zh/
│   ├── index.md           # Landing page (ZH)
│   ├── guide/*.md         # Guide chapters (copied from docs/guide/zh/)
│   └── why.md             # Comparison page (ZH)
└── package.json
```

### Content Strategy

Guide files are **copied** into `site/` (not symlinked) to:
- Avoid cross-platform symlink issues
- Allow doc-site-specific link adjustments (renamed from numbered `01-*.md` to clean `quickstart.md`)
- Keep `docs/guide/` as the canonical source for repo-local browsing

### Landing Page

VitePress hero layout with:
- Elevator pitch, 6 feature cards
- Install commands (curl, brew, cargo)
- Quick start code snippet

### README Slimdown

Reduced from 371 lines to 74 lines:
- Kept: elevator pitch, architecture one-liner, install, quick start, feature bullets
- Added: link to documentation site
- Removed: detailed tables, full YAML examples, project tree, skills index, CI details

### Deployment: Cloudflare Pages

- Free plan: unlimited bandwidth, builds, requests
- Auto-deploy on push to `main` when `site/**` or `docs/guide/**` changes
- Uses `cloudflare/wrangler-action@v3`
- Requires `CLOUDFLARE_API_TOKEN` and `CLOUDFLARE_ACCOUNT_ID` secrets

## Files Created

| File | Purpose |
|------|---------|
| `site/` | VitePress project root |
| `site/.vitepress/config.ts` | VitePress config with i18n, nav, sidebar, search |
| `site/en/index.md` | EN landing page |
| `site/zh/index.md` | ZH landing page |
| `site/en/guide/*.md` | EN guide chapters (7 files) |
| `site/zh/guide/*.md` | ZH guide chapters (7 files) |
| `site/en/why.md` | EN comparison page |
| `site/zh/why.md` | ZH comparison page |
| `site/package.json` | VitePress dependency |
| `.github/workflows/docs.yml` | Auto-deploy to Cloudflare Pages |

## Files Modified

| File | Change |
|------|--------|
| `README.md` | Slimmed from 371 to 74 lines |
