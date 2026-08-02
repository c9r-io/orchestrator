---
name: e2e-testing
description: Run and author the repository's Playwright E2E tests, validate critical user journeys, and stabilize flaky browser behavior using the actual GUI and service contract.
---

# E2E Testing (Playwright)

Use the existing Playwright configuration and a small set of stable, behavior-focused journeys.

## Repository Layout

- Frontend package: `gui/package.json`
- Playwright config: `gui/playwright.config.ts`
- Browser tests: `gui/tests/e2e/`
- Default test server: the Vite server declared by the Playwright config
- Native host: `crates/gui/`; use Tauri-specific tests only when native commands are in scope

Do not assume Docker Compose, Kubernetes, or a legacy portal tree. If a different target repository owns those surfaces, discover and verify them there first.

## Run Existing Tests

```bash
cd gui
npm run test:e2e
```

Use the package's `test:all` script when unit, coverage, E2E, and build evidence are all required. Install browsers or packages only with user approval when the environment is missing them.

## Authoring Tests

- Prefer `getByRole`, `getByLabel`, and accessible names over CSS structure.
- Avoid sleeps; wait for visible state, URLs, responses, or emitted events.
- Keep each scenario narrow and deterministic.
- Use the existing mock and transport boundaries. If a daemon is required, apply a deterministic fixture from `fixtures/manifests/bundles/` and isolate it by project.
- Capture a trace or screenshot for failures when the config supports it.

Example:

```ts
import { expect, test } from "@playwright/test";

test("critical control remains reachable", async ({ page }) => {
  await page.goto("/");
  await expect(page.getByRole("main")).toBeVisible();
});
```

## Flake Triage

1. Re-run the single failing spec with the same revision and server state.
2. Inspect the retained trace and browser console.
3. Replace timing assumptions and shared state with explicit conditions and setup.
4. Re-run the single spec, then `npm run test:e2e`, then the relevant build/tests.

Never weaken an assertion or add retries without identifying the nondeterministic boundary.
