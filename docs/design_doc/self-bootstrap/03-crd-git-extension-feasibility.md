# Self-Bootstrap - CRD-Based Git Command Extension Feasibility Analysis

**Module**: self-bootstrap
**Status**: Proposal
**Related Plan**: Analyze CRD model extension for git commands in self-bootstrap closed-loop workflow
**Related QA**: N/A (feasibility analysis only)
**Created**: 2026-03-03
**Last Updated**: 2026-03-03

---

## 1. Background

The current self-bootstrap workflow implements the following cycle:

```
plan → qa_doc_gen → implement → self_test → qa_testing → ticket_fix → align_tests → doc_governance → loop_guard
```

This covers the **Development → Testing → Maintenance** phases of the SDLC, but stops short of completing the full closed loop. Notably missing are:

- **Acceptance** (formal validation gate beyond QA)
- **Git Release** (version tagging, changelog generation, branch management)
- **Build** (artifact generation, binary packaging)
- **Re-entry** (feeding build/release results back into the next development cycle)

The proposed extension would close the loop into a complete SDLC cycle:

```
Development → Testing → Maintenance → Acceptance → Git Release → Build → Re-entry → Development
```

This document analyzes whether the existing CRD (Custom Resource Definition) model can be used to introduce git release commands as first-class workflow steps, without modifying the core ResourceKind enum or the scheduler engine.

---

## 2. Goals

- Evaluate feasibility of defining git operations (tag, branch, release, changelog) as CRD-backed resources
- Assess compatibility with the existing two-phase YAML parsing and zero-invasion CRD design
- Determine whether the self-bootstrap workflow can incorporate release/build steps using existing extensibility points
- Identify gaps, risks, and prerequisites for implementation

## 3. Non-goals

- Actual implementation of git CRD resources or workflow steps
- CI/CD pipeline integration (GitHub Actions, etc.)
- Remote artifact registry or package distribution
- Changes to the core ResourceKind enum

---

## 4. Feasibility Analysis

### 4.1 CRD Model Compatibility

**Conclusion: Fully compatible.**

The current CRD system provides all necessary extension points:

| Capability | Current Support | Git Extension Usage |
|---|---|---|
| Custom kind registration | ✅ Two-phase YAML parsing | Define `GitRelease`, `GitBranch`, `BuildPipeline` kinds |
| Schema validation | ✅ JSON Schema in CRD spec | Validate tag format, branch naming, semver |
| CEL rule validation | ✅ Runtime semantic rules | Enforce `self_test_passed == true` before release |
| Untyped spec storage | ✅ `serde_json::Value` | Store git config, remote URLs, branch policies |
| CLI integration | ✅ `get`/`describe`/`delete` via CRD alias | `get gitrelease/v1.0.0`, `describe gr/latest` |
| Composite key storage | ✅ `"{Kind}/{name}"` pattern | `GitRelease/v1.0.0`, `BuildPipeline/nightly` |
| Label selector queries | ✅ Existing CRD query support | Filter releases by `status: published`, `branch: main` |

The zero-invasion design means git resource kinds can be introduced purely through YAML manifests without touching `ResourceKind` enum or the core parser.

**Example CRD definition:**

```yaml
apiVersion: orchestrator.dev/v2
kind: CustomResourceDefinition
metadata:
  name: gitreleases.extensions.orchestrator.dev
spec:
  kind: GitRelease
  plural: gitreleases
  short_names: [gr]
  group: extensions.orchestrator.dev
  versions:
    - name: v1
      served: true
      schema:
        type: object
        required: [tag, strategy]
        properties:
          tag:
            type: string
            pattern: "^v[0-9]+\\.[0-9]+\\.[0-9]+.*$"
          strategy:
            type: string
            enum: [tag_only, branch_merge, full_release]
          changelog:
            type: object
            properties:
              auto_generate: { type: boolean }
              template: { type: string }
          pre_release_checks:
            type: array
            items:
              type: object
              required: [name, command]
              properties:
                name: { type: string }
                command: { type: string }
      cel_rules:
        - rule: "self.tag.startsWith('v')"
          message: "tag must follow semver with 'v' prefix"
```

### 4.2 Agent Capability Extension

**Conclusion: Compatible with existing capability model.**

Git operations can be modeled as agent capabilities:

```yaml
agents:
  releaser:
    metadata:
      name: releaser
    capabilities:
      - git_release
      - changelog_gen
      - build
    templates:
      git_release: "git tag -a {tag} -m '{message}' && git push origin {tag}"
      changelog_gen: "git log --oneline {from_tag}..HEAD > CHANGELOG.md"
      build: "cargo build --release"
```

The existing capability-driven step matching (`required_capability` → agent selection) works without modification. Steps declare `required_capability: git_release` and the scheduler selects the `releaser` agent.

**Key advantage**: Git commands are inherently shell-executable, making them natural fits for the agent template system (`templates` with placeholder substitution).

### 4.3 Workflow Step Integration

**Conclusion: Compatible with minimal workflow changes.**

The proposed closed-loop can be modeled as additional workflow steps:

```yaml
workflows:
  self-bootstrap-release:
    steps:
      # --- Development Phase ---
      - id: plan
        builtin: init_once
        repeatable: false
      - id: implement
        required_capability: implement
        repeatable: true
        scope: task

      # --- Testing Phase ---
      - id: self_test
        builtin: self_test
        repeatable: true
      - id: qa_testing
        required_capability: qa
        repeatable: true
        scope: item

      # --- Maintenance Phase ---
      - id: ticket_fix
        required_capability: fix
        repeatable: true
        scope: item
      - id: align_tests
        required_capability: align_tests
        repeatable: true
        scope: task

      # --- Acceptance Phase (NEW) ---
      - id: acceptance_gate
        required_capability: acceptance
        repeatable: true
        scope: task
        prehook:
          condition: "ctx.cycle > 1 && ctx.vars.self_test_passed == true && ctx.vars.open_tickets == 0"

      # --- Git Release Phase (NEW) ---
      - id: changelog_gen
        required_capability: changelog_gen
        repeatable: false
        scope: task
        prehook:
          condition: "ctx.vars.acceptance_passed == true"
      - id: git_release
        required_capability: git_release
        repeatable: false
        scope: task
        prehook:
          condition: "ctx.vars.changelog_generated == true"

      # --- Build Phase (NEW) ---
      - id: build_artifact
        required_capability: build
        repeatable: false
        scope: task
        prehook:
          condition: "ctx.vars.release_tag != ''"

      # --- Re-entry (loop_guard decides) ---
      - id: loop_guard
        builtin: loop_guard
        is_guard: true
        repeatable: true
```

**Prehook conditions** ensure the release/build steps only execute when all quality gates pass. The existing CEL-based prehook system (`Run`/`Skip`/`Branch`) provides sufficient control flow.

### 4.4 Safety Integration with Git Checkpoint

**Conclusion: Compatible, but requires careful ordering.**

The current git checkpoint mechanism (`checkpoint_strategy: git_tag`) creates tags at format `checkpoint/{task_id}/{cycle}`. Release tags would use semver format (`v1.0.0`). These two tag namespaces do not conflict.

**Interaction model:**

```
Cycle start → checkpoint/task-123/5 (safety tag)
  ↓
Steps execute: implement → test → fix → ...
  ↓
Acceptance passes
  ↓
Release tag: v1.0.0 (release tag, different namespace)
  ↓
Build artifact
  ↓
Cycle end → loop_guard evaluates next cycle
```

**Critical consideration**: If auto-rollback triggers after a release tag is created, the tag remains in git history. This is acceptable because:
1. Release tags are only created after acceptance passes
2. `git reset --hard` (rollback) does not remove tags — the tag points to an accepted commit
3. The release step's `prehook` prevents re-release on retry cycles

### 4.5 Self-Bootstrap Closed Loop

**Conclusion: Feasible with the proposed architecture.**

The complete closed-loop maps naturally to the existing workflow model:

```
┌─────────────────────────────────────────────────────────────────┐
│                    Self-Bootstrap Closed Loop                    │
├─────────────────────────────────────────────────────────────────┤
│                                                                 │
│  ┌──────────┐   ┌──────────┐   ┌───────────┐   ┌───────────┐  │
│  │Development│──▶│ Testing  │──▶│Maintenance│──▶│Acceptance │  │
│  │          │   │          │   │           │   │           │  │
│  │ plan     │   │ self_test│   │ ticket_fix│   │ acceptance│  │
│  │ implement│   │ qa_test  │   │ align_test│   │ _gate     │  │
│  │ qa_doc   │   │          │   │ doc_gov   │   │           │  │
│  └──────────┘   └──────────┘   └───────────┘   └─────┬─────┘  │
│       ▲                                               │        │
│       │                                               ▼        │
│  ┌────┴─────┐                                  ┌───────────┐   │
│  │ Re-entry │◀─────────────────────────────────│Git Release│   │
│  │          │          ┌──────────┐             │           │   │
│  │ loop_    │◀─────────│  Build   │◀────────────│ changelog │   │
│  │ guard    │          │          │             │ git_tag   │   │
│  └──────────┘          │ artifact │             └───────────┘   │
│                        └──────────┘                             │
└─────────────────────────────────────────────────────────────────┘
```

**Re-entry mechanism**: The `loop_guard` step evaluates whether a new development cycle should begin based on:
- Build success/failure status
- Open issue count from external sources
- Scheduled release cadence (via CEL time expressions)
- Manual trigger flag in context variables

---

## 5. Gap Analysis

### 5.1 Existing Gaps

| Gap | Severity | Description | Mitigation |
|-----|----------|-------------|------------|
| No remote git push | Medium | Current checkpoint only uses local `git tag -f`. Release workflow needs `git push origin {tag}` | Agent template already supports arbitrary shell commands; releaser agent executes push |
| No artifact storage | Medium | Build step produces binaries but no artifact registry integration | Use filesystem or external registry via agent command; introduce `ArtifactStore` CRD for metadata |
| No acceptance gate builtin | Low | Acceptance is a concept, not a builtin step | Model as capability-based step with structured output `{accepted: bool}` |
| No changelog generation | Low | No built-in changelog tooling | Agent template with `git log` or `git-cliff` integration |
| No semver management | Low | No version tracking resource | Introduce `VersionPolicy` CRD to track current/next version |
| Context variable propagation | Low | Release tag name needs to flow from `git_release` step to `build_artifact` step | Existing structured output → context vars pipeline supports this |

### 5.2 No Gaps (Already Supported)

| Feature | Status | Mechanism |
|---------|--------|-----------|
| Step ordering and dependencies | ✅ | Workflow step list + prehook conditions |
| Conditional execution | ✅ | CEL prehook: `Run`/`Skip`/`Branch` |
| Agent capability matching | ✅ | `required_capability` → agent selection |
| Shell command execution | ✅ | Agent templates with placeholder substitution |
| Structured output capture | ✅ | JSON normalization for step results |
| State persistence across cycles | ✅ | SQLite `tasks`/`command_runs`/`events` |
| Safety rollback | ✅ | `git reset --hard` + binary snapshot restore |
| CRD registration and validation | ✅ | Two-phase YAML parsing, schema + CEL validation |

---

## 6. Proposed CRD Resources

Three new CRD kinds are recommended to fully model the git release lifecycle:

### 6.1 GitRelease

Tracks individual release events with version, tag, and status.

```yaml
kind: GitRelease
metadata:
  name: v1.2.0
  labels:
    branch: main
    status: published
spec:
  tag: v1.2.0
  strategy: full_release       # tag_only | branch_merge | full_release
  changelog:
    auto_generate: true
    from_tag: v1.1.0            # diff base for changelog
  pre_release_checks:
    - name: self_test
      command: "cargo test --lib --bins"
    - name: lint
      command: "cargo clippy -- -D warnings"
```

### 6.2 VersionPolicy

Manages semver progression rules and release branch policies.

```yaml
kind: VersionPolicy
metadata:
  name: default-policy
spec:
  scheme: semver                # semver | calver | custom
  bump_rules:
    major: "breaking_changes > 0"
    minor: "new_features > 0"
    patch: "default"
  branch_policy:
    release_branch: main
    develop_branch: develop
    require_merge_before_tag: true
```

### 6.3 BuildPipeline

Defines artifact build and publication steps (post-release).

```yaml
kind: BuildPipeline
metadata:
  name: release-build
spec:
  trigger: on_release_tag
  steps:
    - name: compile
      command: "cargo build --release"
    - name: checksum
      command: "sha256sum target/release/agent-orchestrator > checksums.txt"
    - name: package
      command: "tar czf release-{tag}.tar.gz target/release/agent-orchestrator checksums.txt"
  artifacts:
    - path: "release-{tag}.tar.gz"
      type: binary
```

---

## 7. Risk Assessment

### 7.1 Technical Risks

| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|------------|
| Release tag created but build fails | Medium | Tag points to unbuildable commit | Add `pre_release_checks` validation; use lightweight tags until build passes, then promote to annotated |
| Self-bootstrap creates release during self-modification | Low | Corrupted release published | Acceptance gate + self_test prehook prevents release without full validation |
| Git push fails (network, auth) | Medium | Release incomplete | Agent retry mechanism + consecutive error tracking handles transient failures |
| Tag namespace collision with checkpoints | Very Low | Checkpoint and release tags overlap | Different prefix namespaces: `checkpoint/` vs. `v` prefix |
| Rollback after release push | Low | Remote has tag but local is rolled back | Release step is `repeatable: false` — won't re-execute after rollback |

### 7.2 Architectural Risks

| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|------------|
| CRD spec bloat | Low | Too many custom kinds for simple git operations | Start with `GitRelease` only; add `VersionPolicy` and `BuildPipeline` only if needed |
| Agent template insufficient for complex git flows | Low | Multi-step git operations hard to express | Use agent `command` with shell scripts or compound commands (`&&` chains) |
| Cross-step data dependency | Medium | Release tag must flow to build step | Existing structured output → context variable mechanism handles this |

---

## 8. Implementation Roadmap (Estimated)

If approved, implementation can proceed in three incremental phases:

### Phase 1: Git Release Agent (Minimal, ~1 design doc + ~3 QA scenarios)

- Define `GitRelease` CRD in YAML fixture
- Create `releaser` agent with `git_release` capability
- Add `acceptance_gate` and `git_release` steps to self-bootstrap workflow
- Prehook: `ctx.vars.self_test_passed == true && ctx.vars.open_tickets == 0`

### Phase 2: Build Pipeline Integration (~1 design doc + ~3 QA scenarios)

- Define `BuildPipeline` CRD
- Add `build_artifact` step with `build` capability
- Structured output captures artifact path and checksum
- Loop guard evaluates build status for re-entry decision

### Phase 3: Version Policy & Changelog (~1 design doc + ~2 QA scenarios)

- Define `VersionPolicy` CRD for semver management
- Integrate `git-cliff` or custom changelog generation
- Auto-bump version based on commit analysis

---

## 9. Conclusion

**Feasibility: HIGH**

The CRD-based git command extension is fully compatible with the existing architecture. The key reasons:

1. **Zero-invasion CRD model** allows defining `GitRelease`, `VersionPolicy`, and `BuildPipeline` as custom resource kinds without modifying the core `ResourceKind` enum or two-phase parser
2. **Agent capability model** naturally maps git operations to shell command templates, requiring no new execution primitives
3. **CEL prehook system** provides sufficient conditional logic to enforce quality gates (acceptance → release → build ordering)
4. **Git checkpoint mechanism** coexists with release tags through namespace separation (`checkpoint/` vs. `v` prefix)
5. **Self-bootstrap safety layers** (binary snapshot, self-test gate, enforcement, watchdog) protect against premature or broken releases
6. **Structured output pipeline** enables cross-step data flow (release tag → build step → loop guard)

The proposed extension transforms the self-bootstrap workflow from a development/testing loop into a complete SDLC closed loop:

```
Development → Testing → Maintenance → Acceptance → Git Release → Build → Re-entry → Development
```

All six new workflow steps (acceptance_gate, changelog_gen, git_release, build_artifact, version_bump, loop_guard re-entry) can be implemented using existing orchestrator primitives. No core engine changes are required.

**Recommendation**: Proceed with Phase 1 (Git Release Agent) as a proof-of-concept to validate the CRD-based approach in a real self-bootstrap execution cycle.
