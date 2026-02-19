---
name: e2e-testing
description: Run and author Playwright E2E tests for the project (frontend-only or full-stack). Use when the user asks to add E2E tests, run Playwright tests, validate critical user journeys, or stabilize flaky E2E tests.
---

# E2E Testing (Playwright)

Run and write Playwright E2E tests with a stable, low-flake strategy.

This skill assumes `project-bootstrap` style repos when applicable:
- Frontend in `portal/`
- Full-stack environment via Docker Compose in `docker/docker-compose.yml`
- Clean reset script at `./scripts/reset-docker.sh`

## Test Strategy

| Type | Directory | Target | Requirements | Purpose |
|------|-----------|--------|--------------|---------|
| Frontend isolation | `portal/tests/e2e/` | dev server (e.g. localhost:5173) | Vite dev only | UI rendering, component behavior |
| Full-stack integration | `portal/tests/e2e-integration/` | Docker services (e.g. localhost:3000) | Docker + all services | Login, API calls, data flow |

Prefer a small number of stable E2E tests covering critical journeys:
- Login/auth (if applicable)
- One core create/update flow
- One permissions/visibility check (if applicable)

Split tests if the project supports it:
- Frontend-only E2E: runs against a dev server, no Docker required.
- Full-stack E2E: runs against Docker Compose services; always reset state before running.

## Discover Existing Setup

From repo root, look for Playwright config and scripts:
- `portal/package.json` scripts like `test:e2e`, `test:e2e:full`, `test:e2e:full:reset`
- `playwright.config.*` or `portal/playwright.config.*`
- existing test directories under `portal/tests/`

Prefer existing scripts/config. Only introduce new structure if none exists.

## Running Tests

### Frontend-only (if supported)

```bash
cd portal
npx playwright test
```

### Full-stack (recommended when verifying integration behavior)

Always reset state first to avoid flaky failures from dirty data:

```bash
./scripts/reset-docker.sh
cd portal
npx playwright test
```

If the project already has a combined script (recommended), use it (example):

```bash
cd portal
npm run test:e2e:full:reset    # Reset env + run tests (recommended)
npm run test:e2e:full          # Run only (requires services)
npm run test:e2e:full -- --ui  # With Playwright UI mode
npm run test:e2e:full -- --headed  # See browser during run
```

## Writing Tests

Guidelines:
- Prefer role/label based selectors: `getByRole`, `getByLabel`, `getByText`.
- Avoid timing sleeps; wait for conditions and URLs.
- Keep each test scenario narrow and deterministic.

Skeleton:

```ts
import { test, expect } from "@playwright/test";

test("scenario: critical flow", async ({ page }) => {
  await page.goto("/");
  await page.getByRole("button", { name: /create/i }).click();
  await page.getByLabel(/name/i).fill("example");
  await page.getByRole("button", { name: /save/i }).click();
  await expect(page.getByText(/created/i)).toBeVisible();
});
```

### API Test Pattern

```ts
import { test, expect } from "@playwright/test";

test("Health endpoint accessible", async ({ request }) => {
  const response = await request.get("http://localhost:8080/health");
  expect(response.ok()).toBeTruthy();
});
```

## Troubleshooting Flakes

Checklist:
- Confirm environment is clean: run `./scripts/reset-docker.sh`.
- Confirm services are healthy: `docker compose -f docker/docker-compose.yml ps` and logs.
- Replace brittle selectors with role/label queries.
- Reduce shared state across tests; make setup explicit per test or in Playwright `globalSetup`.

## Reports

```bash
cd portal
npx playwright show-report playwright-report       # Frontend tests
npx playwright show-report playwright-report-full  # Full-stack tests (if separated)
```

