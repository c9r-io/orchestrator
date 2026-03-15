# Custom Resource Definitions (CRD) Extension System

**Module**: orchestrator
**Scope**: CRD registration, custom resource lifecycle (apply/get/list/describe/delete/export), schema+CEL validation, lifecycle hooks
**Scenarios**: 5
**Priority**: High

---

## Background

The CRD extension system allows users to define new resource types beyond the 10 builtin kinds (Workspace, Agent, Workflow, Project, Defaults, RuntimePolicy, StepTemplate, EnvStore, SecretStore, Trigger). A CRD defines the kind name, plural form, short aliases, API group, versioned JSON Schema, CEL validation rules, and lifecycle hooks. Custom resource instances are validated against their CRD's schema and CEL rules before being persisted.

> **Note**: Since the unified CRD migration (see `42-crd-unified-resource-store.md`), the 10 builtin types are themselves registered as CRDs with `builtin: true`. All resources (builtin + user-defined) flow through the same ResourceStore pipeline. This document covers **user-defined CRD** behavior; for builtin CRD store mechanics, projection writeback, and normalization, see doc 42.

**Key design principles**:
- All resources stored in unified `ResourceStore` with composite key `"{Kind}/{name}"`
- Builtin CRDs are protected (`builtin: true`) — cannot be deleted or overwritten by users
- User-defined CRDs use untyped spec (`serde_json::Value`), validated at runtime via schema + CEL
- Two-phase YAML parsing: `kind` string is read first to route to builtin Resource trait or CRD validation path

**Entry points**:
- `orchestrator apply -f <manifest.yaml>` — register CRD and create CR instances
- `orchestrator get <plural|short_name>` — list custom resources
- `orchestrator get <kind_or_alias>/<name> -o yaml` — get single CR
- `orchestrator describe <kind_or_alias>/<name>` — describe CR
- `orchestrator delete <kind_or_alias>/<name> --force` — delete CR
- `orchestrator manifest export -o yaml` — export includes CRD + CR

### Test Fixture

The fixture file `fixtures/manifests/bundles/crd-test.yaml` contains:
1. A CRD defining `PromptLibrary` with JSON Schema and CEL validation
2. A valid `PromptLibrary` instance `qa-prompts`
3. A builtin `Workspace` to verify mixed-document parsing

---

## Scenario 1: CRD Registration and Custom Resource Creation

### Preconditions
- Orchestrator initialized (`orchestrator init`)
- Fixture file `fixtures/manifests/bundles/crd-test.yaml` exists

### Goal
Verify that a CRD can be registered and a custom resource instance created in a single multi-document apply.

### Steps

1. Apply the CRD test fixture:
   ```bash
   orchestrator apply -f fixtures/manifests/bundles/crd-test.yaml
   ```

2. Verify apply output contains all three resources:
   ```bash
   # Expected output lines:
   # crd/promptlibraries.extensions.orchestrator.dev (kind: PromptLibrary) created
   # promptlibrary/qa-prompts created
   # workspace/crd-test-ws created   (or unchanged if already exists)
   ```

3. Verify the CRD is queryable (list custom resources):
   ```bash
   orchestrator get promptlibraries
   ```

4. Verify the CR instance is retrievable:
   ```bash
   orchestrator get pl/qa-prompts -o yaml
   ```

### Expected
- Apply succeeds with exit code 0
- CRD registration shows `created` (or `unchanged` on re-apply)
- CR instance shows `created`
- `get promptlibraries` lists `qa-prompts`
- `get pl/qa-prompts -o yaml` returns the spec with prompts array

---

## Scenario 2: Schema and CEL Validation Rejects Invalid Resources

### Preconditions
- CRD `PromptLibrary` is already registered (Scenario 1 completed)
- Invalid fixture file `fixtures/manifests/bundles/crd-test-invalid.yaml` exists

### Goal
Verify that schema validation (missing required fields) and CEL validation (empty prompts array) correctly reject invalid custom resources.

### Steps

1. Apply an invalid CR that is missing the required `prompts` field:
   ```bash
   orchestrator apply -f fixtures/manifests/bundles/crd-test-invalid.yaml
   ```

2. Verify the apply reports validation errors and exits with code 1.

3. Verify no invalid resource was persisted:
   ```bash
   orchestrator get promptlibraries
   # Should NOT contain "invalid-prompts"
   ```

### Expected
- Apply exits with code 1
- Error output contains schema validation failure (e.g., "missing required field 'prompts'")
- No invalid CR instances are created in the config

---

## Scenario 3: Custom Resource Get, Describe, and Label Selector

### Preconditions
- CRD `PromptLibrary` registered and `qa-prompts` instance exists (Scenario 1)

### Goal
Verify get/describe/list operations work for custom resources, including label-based filtering.

### Steps

1. Get single CR in JSON format:
   ```bash
   orchestrator get pl/qa-prompts -o json
   ```

2. Describe the CR:
   ```bash
   orchestrator describe pl/qa-prompts
   ```

3. List with label selector:
   ```bash
   orchestrator get promptlibraries -l team=platform
   ```

4. List with non-matching label selector:
   ```bash
   orchestrator get promptlibraries -l team=nonexistent
   ```

### Expected
- JSON output contains `kind`, `apiVersion`, `metadata`, `spec`, `generation`
- Describe output shows resource details (kind, name, apiVersion, generation, timestamps, spec)
- Label selector `team=platform` returns `qa-prompts`
- Non-matching selector returns empty list

---

## Scenario 4: Custom Resource Delete and CRD Cascade Protection

### Preconditions
- CRD `PromptLibrary` registered and `qa-prompts` exists (Scenario 1)

### Goal
Verify that deleting a CR works, deleting a CRD with existing instances is rejected, and deleting a CRD after all instances are removed succeeds.

### Steps

1. Attempt to delete the CRD while instances exist:
   ```bash
   orchestrator delete crd/PromptLibrary --force
   ```
   Expected: fails with "custom resource instances still exist"

2. Delete the CR instance:
   ```bash
   orchestrator delete pl/qa-prompts --force
   ```

3. Verify deletion:
   ```bash
   orchestrator get promptlibraries
   # Should be empty
   ```

4. Now delete the CRD:
   ```bash
   orchestrator delete crd/PromptLibrary --force
   ```

5. Verify CRD is gone (listing should fail):
   ```bash
   orchestrator get promptlibraries
   # Should error: "unknown list resource type"
   ```

### Expected
- Step 1: delete fails with cascade protection error
- Step 2: CR deleted successfully
- Step 4: CRD deleted successfully
- Step 5: `get promptlibraries` errors because CRD no longer exists

---

## Scenario 5: Manifest Export Round-Trip with CRD and CR

### Preconditions
- Orchestrator initialized

### Goal
Verify that CRD definitions and custom resource instances are included in `manifest export` and can be re-applied unchanged.

### Steps

1. Apply the CRD test fixture:
   ```bash
   orchestrator apply -f fixtures/manifests/bundles/crd-test.yaml
   ```

2. Export the full manifest:
   ```bash
   orchestrator manifest export -o yaml > /tmp/crd-export.yaml
   ```

3. Verify the export contains CRD and CR:
   ```bash
   grep "kind: CustomResourceDefinition" /tmp/crd-export.yaml
   grep "kind: PromptLibrary" /tmp/crd-export.yaml
   ```

4. Re-apply the exported manifest:
   ```bash
   orchestrator apply -f /tmp/crd-export.yaml
   ```

5. Verify all resources are `unchanged`:
   ```bash
   # Expected output should show "unchanged" for CRD and CR
   ```

### Expected
- Export contains `CustomResourceDefinition` and `PromptLibrary` documents
- Re-apply of exported manifest shows all resources as `unchanged`
- Full round-trip is idempotent

---

## Checklist

| # | Scenario | Status | Test Date | Tester | Notes |
|---|----------|--------|-----------|--------|-------|
| 1 | CRD Registration and Custom Resource Creation | PASS | 2026-03-15 | claude | CRD+CR persist to DB, get/list work after daemon restart, re-apply shows unchanged |
| 2 | Schema and CEL Validation Rejects Invalid Resources | PASS | 2026-03-04 | claude | Exit 1, "missing required field 'prompts'" |
| 3 | Custom Resource Get, Describe, and Label Selector | PASS | 2026-03-15 | claude | JSON/YAML/describe/label selector all correct via service layer CRD fallback |
| 4 | Custom Resource Delete and CRD Cascade Protection | PASS | 2026-03-15 | claude | Cascade protection, CR delete, CRD delete via service layer CRD routing |
| 5 | Manifest Export Round-Trip with CRD and CR | PASS | 2026-03-15 | claude | Export contains CRD+CR, re-apply shows unchanged (CR persisted to DB) |
