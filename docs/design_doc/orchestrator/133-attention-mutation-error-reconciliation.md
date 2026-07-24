# Orchestrator - Attention Mutation Error Reconciliation

**Module**: Orchestrator
**Status**: Approved
**Related Plan**: FR-121 Attention mutation error feedback and authoritative reconciliation
**Related QA**: `docs/qa/orchestrator/171-attention-mutation-error-reconciliation.md`
**Created**: 2026-07-25
**Last Updated**: 2026-07-25

## Background

Attention Inbox previously stored snapshot, follow-stream, and mutation failures in one React error value. Every snapshot load cleared that value before issuing the query. A failed Claim, Snooze, Resolve, or custom Action therefore set an error and immediately called `load()`, which could erase the mutation failure during the same React update sequence. The daemon state converged, but the operator could not tell whether the action failed or succeeded.

This overlay preserves the daemon as the sole state authority while making mutation failure and reconciliation explicit, persistent, accessible, and observable.

## Goals

- Keep query, stream, and mutation error lifecycles independent.
- Reconcile once after every failed mutation without erasing its cause.
- Use one contract for Claim, Snooze, Resolve, and custom Action.
- Never imply success until the mutation RPC itself succeeds.
- Preserve selection and recover focus when reconciliation removes the initiating control.
- Expose only stable, privacy-safe error categories and low-cardinality metrics.

## Non-goals

- Optimistically mutating Attention rows before daemon confirmation.
- Automatically retrying a failed business mutation.
- Exposing provider errors, database errors, requested decisions, or source message bodies.
- Replacing the canonical Action Audit envelope or daemon version fence.

## Error Boundary

Attention Tauri commands return a serializable `SafeGrpcError`:

| Field | Contract |
|---|---|
| `category` | Closed category derived from gRPC status: `conflict`, `already_applied`, `not_found`, `invalid_request`, `permission`, `unavailable`, `timeout`, or `internal` |
| `message` | Allowlisted operator copy; raw daemon/provider text is discarded |
| `request_id` | Optional validated correlation identifier from response metadata |

The React boundary normalizes the envelope again. Unknown values become `internal`, message text is selected locally by category, and malformed request IDs are discarded. This defense prevents mocked, legacy, or compromised bridge payloads from injecting arbitrary provider text into the DOM.

Snapshot errors, follow-stream errors, and mutation errors have separate state:

- A successful snapshot clears only the query error.
- A healthy follow delta clears only the stream error.
- A mutation error clears only on dismiss, a later success for the same item/operation, or an explicit successful state check.

## Shared Mutation State Machine

All four mutations enter one `runMutation` path:

1. Capture the initiating control and current item/version.
2. Generate one fresh idempotency key and set the page-wide mutation fence.
3. Invoke exactly one mutation RPC.
4. On success, apply the returned daemon item, announce success, clear a matching old mutation error, and record a success metric.
5. On failure, persist the safe mutation error, do not announce success, record the failure metric, and issue one bounded snapshot reconciliation.
6. Release the fence after reconciliation settles.

The UI performs no optimistic row update. Reconciliation never resubmits the failed idempotency key. “Retry latest state check” repeats only `AttentionList`; a later user-authorized business action receives a new key and uses the newly read version.

## Reconciliation Outcomes

| Outcome | UI behavior |
|---|---|
| `pending` | Persistent alert says the latest daemon state is being confirmed |
| `confirmed` | Snapshot replaces local rows, original mutation cause remains visible, and the UI states that the failed action was not announced as successful |
| `unconfirmed` | Mutation cause remains and a separate high-priority query alert says the latest state is unknown; repeating the action is discouraged |

Snapshot application preserves the failed item selection when it remains visible under the active filters. If it is no longer visible, the first valid row is selected. When the initiating control survives, focus is untouched. When authoritative state removes it, focus moves to the persistent queue listbox rather than `document.body`.

## Accessibility

- Mutation failure uses a persistent atomic `role="alert"`.
- State-check retry and dismiss are native keyboard-operable buttons with explicit accessible names.
- Follow disconnection uses a separate status region and dismiss action.
- Busy mutation controls are disabled to prevent overlapping local submissions.
- Error surfaces use danger tokens, a two-pixel unconfirmed border, and opaque design-system fallbacks; narrow layouts stack content and controls.

## Observability And Privacy

The existing optional local telemetry sink accepts two new families:

| Metric | Dimensions |
|---|---|
| `attention_mutation_total` | `action`, `result`, optional `error_category` |
| `attention_reconciliation_total` | `action`, `result` |

`action` is restricted by the UI to `claim`, `snooze`, `resolve`, or `execute`; `result` is success/failure or confirmed/unconfirmed. Project ID remains the metric isolation scope, not a label. Item IDs, versions, requested decisions, Slack content, credentials, and provider error text are not accepted dimensions. Telemetry failure never blocks the operator flow.

## Alternatives And Tradeoffs

- Reusing the existing global error was smaller but could not express dual failure or independent clearing.
- Retrying the mutation after a transport failure could hide response-loss ambiguity and repeat effects. Explicit state reconciliation is safer.
- Focusing the alert would guarantee visual attention but interrupt keyboard context. A live alert plus conditional listbox fallback preserves operator position.
- Passing raw daemon messages would provide more detail but violate the default Console privacy boundary. Request ID retains diagnostic correlation.

## Risks And Mitigations

- **Stale error after unrelated work**: errors are item/operation scoped and have explicit dismiss/retry controls.
- **Reload storm**: one mutation failure causes exactly one automatic snapshot; there is no automatic retry loop.
- **Duplicate submissions**: a page-wide busy fence blocks controls until the attempt and reconciliation finish.
- **Late local contradiction**: only daemon responses and snapshots update rows; failure never enters the polite success announcement.
- **Metric cardinality or content leakage**: the core repository rejects dimensions outside the closed allowlist.

## Verification

- Vitest covers four mutation kinds, confirmed and unconfirmed reconciliation, subsequent success, dismiss, retry-only state checks, unique keys, selection, announcements, and focus fallback.
- Rust tests cover safe gRPC mapping, request correlation, content stripping, and telemetry dimension rejection.
- Playwright covers a version conflict with a changed assignee/version, persistent alert, removed Claim control, listbox focus, Axe, and metric calls.
- The isolated daemon QA runs two independent CLI clients against one version and verifies one winner, stable conflict output, authoritative reread, and distinct canonical retry identities.

## Major Code Touchpoints

- `crates/gui/src/errors.rs`
- `crates/gui/src/commands/attention.rs`
- `crates/gui/src/commands/stream.rs`
- `gui/src/pages/AttentionInbox.tsx`
- `gui/src/lib/telemetry.ts`
- `core/src/process_metrics.rs`
- `scripts/qa/test-attention-inbox.sh`
