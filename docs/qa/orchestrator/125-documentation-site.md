---
self_referential_safe: false
self_referential_safe_scenarios:
  - S1
# S1 安全：npm ci + vitepress build，纯构建验证无副作用
# S2-S5 不安全：需要启动 dev server + 浏览器/GUI 交互
---

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

## Checklist

- [x] S1: VitePress builds successfully
- [x] S2: EN landing page
- [x] S3: ZH landing page
- [x] S4: Language switcher
- [x] S5: Full-text search
