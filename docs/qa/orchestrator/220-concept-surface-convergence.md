---
lifecycle: active
related_fr: FR-166
self_referential_safe: true
---

# 220. Concept surface convergence

Verifies the FR-166 decisions: the console names its object Task, the guide's
built-in kinds list is tied to `ResourceKind`, the English source directory holds
no undeclared Chinese, and the two Trigger examples added to chapter 02 are
accepted by the product as written.

Every scenario is read-only or runs a test suite. Nothing starts a daemon, writes
the runtime database, or edits `~/.orchestratord`. The negative fixtures operate
on a `git ls-files` copy under `$TMPDIR`, never on the working tree.

> The GUI suite requires Node within the range `gui/package.json` declares
> (`>=22 <26`). Under Node 26, twelve tests unrelated to this FR fail on
> `localStorage` being undefined in jsdom. Run under a supported Node before
> reading any GUI result as evidence.

## Scenario 1 — the console names its object Task, in label and in URL together

**Steps**

```bash
cd gui && npm test -- src/App.test.tsx src/lib/routes.test.ts
```

**Expected result**

- `names the task surface Tasks and links it at the task path` passes. It reads
  the label and the `href` from the same rendered element, and additionally
  asserts no link named `Processes` remains and the button reads `New task`.
- `gives every page exactly one url segment` passes, deriving the comparison from
  `pathForPage` so the nav and `formatConsoleRoute` cannot disagree.

**Why both halves are in one assertion**: a rename that touched only the label
leaves the nav linking `#/processes`; one that touched only the route leaves the
console saying Processes. Each passes an assertion that reads only the other.

## Scenario 2 — hashes minted before the rename still land on the same page

**Steps**

```bash
cd gui && npm test -- src/lib/routes.test.ts
```

**Expected result**

`keeps hashes minted before the Task rename landing on the same page` passes:
`#/processes`, `#/processes/task-1`, `#/processes/task-1?review=safe-resume` and
`#/new-process/draft-1` all parse to the same routes as their new spellings, and
`formatConsoleRoute` writes back `#/tasks/task-1` and `#/new-task/draft-1`.

The last two assertions are the ones with teeth: an alias that also became the
canonical output would leave the console showing Tasks and minting `#/processes`
forever, which every "the old link still works" test would pass.

**Browser-level evidence**: `gui/tests/e2e/process-console.spec.ts` keeps one
case (`non-code process uses task semantics in the console`) deliberately on
`/#/processes/task-non-code`, so the alias is exercised through a real browser
and not only through `parseConsoleRoute`.

## Scenario 3 — a blanket 进程 → 任务 substitution is rejected

**Steps**

```bash
cd gui && npm test -- src/lib/i18n.test.ts
```

**Expected result**

- `names the unit of work a task, not a process` passes on the five renamed
  strings.
- `still calls the daemon a process, because it is one` passes, asserting
  `connection.cause1Title` is `"守护进程未启动"`.

**The negative fixture**: run `perl -0pi -e 's/进程/任务/g' src/lib/i18n.ts` on a
scratch copy. The first test still passes and the second fails, naming the
daemon-diagnostic string. That is the whole point — Chinese collapses two
unrelated senses onto 进程, and a substitution pass turns a daemon-down
diagnostic into advice for a different problem while satisfying every vocabulary
assertion.

## Scenario 4 — the guide's built-in kinds list is derived from the enum

**Steps**

```bash
bash scripts/qa/test-docs-reality-alignment.sh > /tmp/reality.log 2>&1; echo $?
grep -c 'PASS: check_resource_kind_catalog' /tmp/reality.log
```

**Expected result**

Exit `0`, the summary line `=== docs reality alignment: 7 passed, 0 failed ===`
is present, and `check_resource_kind_catalog` passes.

**Why the summary line is checked**: a run that terminates early reads exactly
like a complete one from the exit code alone.

## Scenario 5 — both gates fail in both directions, naming what broke

**Steps**

```bash
bash scripts/qa/test-docs-reality-alignment.sh --fixture-test > /tmp/fx.log 2>&1; echo $?
```

**Expected result**

Exit `0` with `=== fixtures: 21 passed, 0 failed ===`, including these four:

| Fixture | Mutation | Required diagnostic |
|---|---|---|
| `fixture new kind unlisted` | adds a 13th `ResourceKind` variant, leaves the guide alone | `omits ResourceKind::ProbeKind` |
| `fixture prose names a non-kind` | adds `WorkflowStore` back to the guide list | `names WorkflowStore, which is not a ResourceKind variant` |
| `fixture Chinese in the EN slot` | writes an undeclared Chinese file into `docs/guide/` | `docs/guide/zz-probe.md is Chinese text in the English source slot` |
| `fixture ZH-only numbered chapter` | writes `docs/guide/zh/99-probe.md` with no EN counterpart | `docs/guide/zh/99-probe.md has no same-numbered English chapter` |

These use `expect_fail_naming`, which asserts the **diagnostic** and not only a
non-zero exit. An exit code cannot say which branch a gate failed through, and a
gate already red before the mutation satisfies "it failed". Each check also has a
mutation in both directions, so the log distinguishes an under-reaching matcher
from an over-reaching one.

The two Chinese fixtures are additions rather than deletions on purpose: the case
an implementation is least likely to catch is a new Chinese file, because nobody
edits a guard-list when they add one.

## Scenario 6 — the English source directory holds no undeclared Chinese

**Steps**

```bash
ruby -e '
Dir.glob("docs/guide/*.md").sort.each do |p|
  lines = File.read(p).gsub(/```.*?```/m, "").lines.map(&:strip)
  lines.reject! { |l| l.empty? || l.start_with?("|---", "---") }
  han = lines.count { |l| l.match?(/[一-鿿]/) }
  printf("%-50s %.2f\n", File.basename(p), han.to_f / lines.size)
end'
jq -r '.translationGaps[] | select(.collection == "guide") | .slug' config/governance/docs-publishing.json
```

**Expected result**

Exactly four files score above 0.50 — the four Slack runbooks — and those four
are exactly the slugs `translationGaps` declares for the guide collection.
`agent-driver-model.md` scores about 0.37 and is deliberately below the line: it
is an English chapter containing Chinese passages, which is a different problem
and is recorded as a known limit in DD-182.

## Scenario 7 — the guide's Trigger examples are accepted by the product

**Steps**

```bash
cargo test -p agent-orchestrator --lib guide_trigger_examples
```

**Expected result**

`guide_trigger_examples_deserialize_as_written` passes. It reads the YAML fences
out of `docs/guide/02-resource-model.md`, keeps the `spec:` fences containing an
`event:` key, and deserializes each as `TriggerSpec`. It requires all three of
`task_completed`, `webhook` and `filesystem` to be documented, and checks that
the filesystem example's debounce value reaches the field rather than merely
parsing.

**The negative fixture**: change `debounce_ms: 500` to `debounceMs: 500` in the
chapter — the camelCase an author would guess from the webhook fields directly
above it. The test fails with `unknown field 'debounceMs', expected one of
'paths', 'events', 'debounce_ms'`. `TriggerSpec` and its children declare
`deny_unknown_fields`, so a wrong spelling in the guide is a rejection at apply
time and not a silent no-op.

**Why the examples are read from the chapter**: a copy of the example inside the
test proves the copy parses.

## Scenario 8 — the document gates stay green

**Steps**

```bash
bash scripts/qa-doc-lint.sh                          > /tmp/1.log 2>&1; echo $?
ruby scripts/qa/doc-lifecycle.rb                     > /tmp/2.log 2>&1; echo $?
bash scripts/qa/test-docs-publishing-integrity.sh    > /tmp/3.log 2>&1; echo $?
bash scripts/qa/test-cli-doc-parity.sh               > /tmp/4.log 2>&1; echo $?
```

**Expected result**

All four exit `0`, each log ending in its own summary line. Publishing integrity
in particular must pass `check_nav_complete`: the new English chapter 08 is
published, so `site/.vitepress/config.ts` has to link it or the page is generated
and unreachable.

`test-cli-doc-parity.sh` is expected to be **unaffected**. It governs
`07-cli-reference.md` in both locales, and chapter 08 is a console guide rather
than a CLI surface — the FR asked for that scope to be evaluated, and the
evaluation's answer was that 08 does not belong there. `check_guide_language_parity`
covers the EN/ZH chapter-set question instead.

## Regression Checklist

- [ ] S1: nav label and nav href both read Task, from the same element
- [ ] S2: pre-rename hashes parse; the canonical output is the new spelling
- [ ] S3: the daemon-sense 进程 string survives a vocabulary sweep
- [ ] S4: the kinds catalog passes and the summary line is present
- [ ] S5: 21 fixtures pass, and the four new ones assert their diagnostics
- [ ] S6: the Chinese files in the EN slot are exactly the declared ones
- [ ] S7: the chapter's Trigger examples deserialize; `debounceMs` fails
- [ ] S8: doc lint, lifecycle, publishing integrity and CLI parity all green
