---
name: security-test-doc-gen
description: Generate and complete reusable security test documents under docs/security/ based on the current project implementation and/or a confirmed plan. Use when a developer asks to complete security test docs, add missing security scenarios, or establish a security test baseline for the project.
---

# Security Test Doc Gen

Generate/complete `docs/security/**` so it evolves from a generic template into a set of project-aligned, reproducible, executable security test scenarios.

## ASVS 5.0 High Bar (Required)

- **Baseline**: Use **OWASP ASVS 5.0** as the control source. Default target is **L2** unless the project explicitly targets L1, or a high-value system requires L3.
- **No "fake compliance"**: Docs may claim "covered chapters/requirements + evidence", but must not claim "ASVS certified/passed".
- **Traceability**: Each doc header must list applicable `OWASP ASVS 5.0` chapters (and requirement ids if needed). Record selection rationale and evidence sources in `docs/security/_surface/asvs_profile.md`.
- **Do not invent requirement ids**: If you reference a specific requirement id, it must match ASVS 5.0 text.
  - Recommended format: `v5.0.0-<chapter>.<section>.<requirement>`

## Inputs

- Current repo code and configuration (`core/`, `portal/`, `docker/`, `k8s/`, `deploy/`, `.github/`, etc.)
- If available: a confirmed plan mode output (to cover new security scenarios introduced by new features)

## Outputs

- Update/add `docs/security/**` (only the portions that apply to the current project)
- Update `docs/security/README.md` index (keep it lightweight; do not hardcode totals)

## Workflow

1. **Define scope**
   - `feature-only`: only cover security scenarios relevant to the current feature (recommended default)
   - `system-baseline`: establish/refresh a baseline across the whole project (pre-release/security regression)
   - Decide the **ASVS target level** (default L2) and record it in `docs/security/_surface/asvs_profile.md`

2. **Discover the security surface (from code/config)**
   - Prefer running the extraction script to produce a "security surface inventory" (endpoints/routes/services):
     - `.claude/skills/security-test-doc-gen/scripts/extract_surface.sh`
     - Output defaults to `docs/security/_surface/`
     - Optional overrides:
       - `CORE_DIRS=core,auth9-core` (backend candidates, comma-separated)
       - `PORTAL_DIRS=portal,auth9-portal` (frontend candidates, comma-separated)
     - If output is empty: the project may not have `core/`/`portal/` yet or routes are generated elsewhere; fall back to manual search.
   - HTTP API:
     - Search for router definitions (framework-specific) and `/api/` paths
     - If OpenAPI/Swagger exists, use it as the endpoint inventory source
   - gRPC:
     - Search for `.proto` files and service definitions
   - Authentication:
     - Search for `Authorization`, `Bearer`, `jwt`, `oidc`, `session`, `cookie`
   - Authorization model:
     - Search for `rbac`, `permission`, `role`, `tenant`, `org`, `workspace`
   - Outbound calls:
     - Search for HTTP client usage and URL fields (SSRF scenarios)
   - File handling:
     - Search for upload/download endpoints, multipart handling, object storage
   - Data stores and secrets:
     - Search for `sqlx`/`redis`/`DATABASE_URL`/`REDIS_URL`/`SECRET`, etc.

3. **Generate/update ASVS profile and input inventory (required)**
   - Generate/update:
     - `docs/security/_surface/asvs_profile.md` (ASVS 5.0 chapter selection: goals/scope/rationale)
     - `docs/security/_surface/input_inventory.md` (external input inventory for V1/V2/V4/V5, etc.)
   - Input inventory must include constraints and high-risk sinks (DB queries, template rendering, file paths, outbound URLs, deserialization, logging). It is not a "checkbox spreadsheet".

4. **Select applicable docs and fill placeholders**
   - For each applicable doc:
     - Replace `{placeholder}` with real endpoints/routes/field names where possible (keep a small number of placeholders for tester-provided token/id)
     - Add a runnable verification method per scenario (curl/grpcurl/SQL/log grep)
   - For each non-applicable doc:
     - Either remove from the index (preferred) or keep it but mark the doc header as "Not applicable to this project"

5. **Add incremental scenarios for the current feature (feature-only mode)**
   - Extract from confirmed plan output or code diffs:
     - new/modified endpoints and data changes
     - new roles/permission boundaries
     - new external URL inputs/callbacks (SSRF)
     - new uploads/exports (data exfiltration)
   - Add scenarios to the best matching existing doc (avoid scattering tiny new docs).

6. **Update index and traceability**
   - Update `docs/security/README.md` to reflect what applies to the current project.
   - Update `docs/security/_surface/asvs_profile.md` with:
     - applicable chapters (ASVS chapter number/name)
     - which chapters were triggered by the current feature and why
     - key evidence (endpoint inventory, code paths, config)

## Doc Format Rules

- Language: English throughout. Keep technical identifiers (path/header/SQL) as-is.
- Each doc should have <= 5 numbered scenarios (split if needed).
- Each scenario must include:
  - Preconditions
  - Attack Objective
  - Attack Steps
  - Expected Secure Behavior
  - Verification (runnable commands/checks)

## References

- Baseline entry point: `docs/security/README.md`
- Writing style guide: `.claude/skills/security-test-doc-gen/references/security-doc-style.md`
- Surface extraction script: `.claude/skills/security-test-doc-gen/scripts/extract_surface.sh`
- ASVS chapter/id notes: `.claude/skills/security-test-doc-gen/references/asvs-5.0-overview.md`
- ASVS profile template: `.claude/skills/security-test-doc-gen/references/asvs-5.0-profile-template.md`
- Input inventory template: `.claude/skills/security-test-doc-gen/references/input-inventory-template.md`
- Input-validation high bar checklist: `.claude/skills/security-test-doc-gen/references/asvs-5.0-input-validation-checklist.md`

