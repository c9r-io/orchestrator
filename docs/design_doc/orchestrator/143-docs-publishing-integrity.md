# DD-143: Documentation Publishing Single Source and Link Integrity

**Status**: Implemented (FR-131)
**Related**: [DD-83](83-documentation-site.md) (the site), [DD-90](90-workflow-template-library.md)
(the hand-authored showcase pages), [DD-139](139-qa-gate-enforcement-surface.md) (FR-127's
enforcement surface), [DD-141](141-skill-mirror-integrity.md) (the single-source pattern this
follows)

## The defect

`scripts/sync-docs.mjs` generated `site/{en,zh}/guide/**` from `docs/guide/**`, and those
directories were gitignored. `site/{en,zh}/showcases/**` was 36 tracked files with no generator,
no upstream, and no check on either side.

One source had no published page at all:
`docs/showcases/streaming-mark-done-convergence.md`, the typed-driver showcase that
`scripts/qa/test-agent-driver-documentation-alignment.sh` governs and that both CEL guides send
readers to. It had never been on the site.

The two halves of one pipeline were governed by opposite rules, so the repository could be green
while the site was wrong — and was.

## What the FR got wrong

FR-131 was a proposal, not ground truth. Rebuilding its claims changed the work.

| FR claim | Reality |
|---|---|
| `site/*/showcases/` are manual **copies** of `docs/showcases/` | 17 of 18 differ. `site/en` was an English **translation** existing nowhere else; `site/zh` was the Chinese with links rewritten to VitePress routes and link text translated. Both hand-authored derivatives |
| Gitignore them, as the guide already is | As written this would have **deleted 17 English translations and one Chinese one**. The sources had to be recovered first |
| The CEL guides *link* the missing showcase, so readers hit a 404 | Both name it as an inline code span, not a markdown link. Nothing was clickable; the defect is that a guide points readers at a repo path the site never published |
| 3 accumulated broken links | **1.** `docs/qa/orchestrator/125b-*.md` is a code span in a sentence about checking links; `.claude/skills/playwright-cli/SKILL.md` is inside a fence showing sample output. Six more the FR never found — `](fr-watch)` and friends in the site showcases — are valid VitePress routes that a naive checker reports |
| 596 markdown files / 85702 lines | 603 / 87037 at the time of work; 567 tracked after the 36 generated pages were untracked |

Four gaps the FR did not name:

1. **`docs.yml` could not deploy a showcase change.** Its `paths:` filter was `site/**` and
   `docs/guide/**`. Once the site pages became generated and gitignored, a `docs/showcases/**`
   edit matched no trigger.
2. **`sync-docs.mjs` never deleted.** A renamed or removed source left its page on the site
   permanently — the drift mode that survives making the directory generated.
3. **Twelve already-published pages were unreachable from the navigation**: eight EN guide
   chapters, three ZH, and one ZH showcase. Publishing a page and linking it are independent acts
   in VitePress, and only the first had ever been checked.
4. **The FR's non-goal contradicted its own acceptance criterion.** "Do not change nav config"
   cannot coexist with "the missing showcase appears in the EN/ZH site": a generated page with no
   sidebar entry is not reachable by any reader. The criterion won.

## Design

### One policy, two consumers

`config/governance/docs-publishing.json` declares the publish set. `scripts/sync-docs.mjs` reads
it to decide what to generate, and `scripts/qa/test-docs-publishing-integrity.sh` reads it to
decide what must exist. Neither hardcodes a collection table, so they cannot disagree about what
is published — the same relationship `config/governance/skill-mirrors.json` has with the skill
mirrors.

```
collections[]      name, sources{lang → dir}, stripNumericPrefix, requireBilingual, fallback
translationGaps[]  collection, slug, absentSource, owedTranslation, reason
navConfig          site/.vitepress/config.ts
navExemptions[]    empty
```

### Showcase source layout

`docs/showcases/*.md` is the English source and `docs/showcases/zh/*.md` the Chinese, matching
`docs/guide`. Because the top-level paths do not move, every existing repository reference keeps
resolving; only the language at those paths changed.

The English text was recovered from `site/en/showcases/` with its VitePress routes converted back
to file-relative links, and the Chinese moved down one level with its `../guide/x.md` links
rewritten to `../../guide/zh/x.md`.

`orchestrator-usage-manual-testing` was the trap inside the trap: its repository source was
**already English** at `HEAD`, so the recovery initially wrote English into both slots and the
only Chinese copy — living in `site/zh/showcases/` — would have been lost when that directory
became generated. A Han-density check over the recovered `zh/` tree caught it.

### Translation gaps

`requireBilingual: true` on showcases means a page present in one locale and not the other fails
the gate unless it is a declared gap with a written reason. Two are declared:

| Slug | Absent source | Owed | Effect |
|---|---|---|---|
| `full-qa-execution` | `zh` | an **English** translation — the Chinese text occupies the `en` slot | served at both routes |
| `streaming-mark-done-convergence` | `zh` | a Chinese translation | served at both routes |

The `fallback: "declared-gaps"` rule publishes such a page into the absent locale from the locale
that has it. Its links are rewritten from the directory the file actually lives in, so an
untranslated page points at pages that exist.

The guide collection sets `requireBilingual: false` with a written reason: EN/ZH guide parity is
the `guide-alignment` skill's scope, and an untranslated chapter is simply absent from that
locale rather than served in the wrong language.

### Locale-preferring link rewrite

When a source link names another locale's file and the locale being written also publishes that
slug, the generated link stays in-locale. A source may name the other locale's file because only
that locale had one when it was written; a declared gap publishes it in both, and the reader
should not be thrown across the language boundary. Links to slugs the destination locale does
*not* publish still cross — that is how `zh/guide/agent-process-console` reaches the EN-only
operations chapter, unchanged.

### The gates

`scripts/qa/test-docs-publishing-integrity.sh`, seven checks:

| Check | Assertion |
|---|---|
| `check_policy_fresh` | Every source dir, gap slug, and nav exemption still exists; every reason is substantive |
| `check_source_inventory` | No two sources collapse onto one slug; every monolingual page in a bilingual collection is a declared gap |
| `check_generated_not_tracked` | Nothing under a published site directory is tracked by git |
| `check_publish_bijection` | Runs the generator into `$TMPDIR`, compares produced against declared, per locale, **both directions** |
| `check_sync_idempotent` | Two syncs of unchanged sources are byte-identical |
| `check_nav_reachable` | Every nav route resolves to a produced page |
| `check_nav_complete` | Every produced page is linked from the navigation |

The expected set is derived from the policy in bash, independently of the JavaScript generator.
Without that independence the bijection check would only prove the generator agrees with itself.

`scripts/qa/test-markdown-link-integrity.sh`, two checks: every relative link target in
`git ls-files '*.md'` resolves, and no exemption outlives the link it excuses.

Its resolution rules exist because of specific false positives, not in the abstract:

- fenced blocks and inline code spans are stripped first — two of the FR's three "broken links"
  are exactly these;
- `#fragment` and `?query` are split off before resolving;
- `/`-rooted targets are VitePress routes, not paths;
- an extensionless target resolves against `X` and `X.md` — the six site links the FR missed;
- resolution follows symlinks, so `.agents/skills/**` does not false-positive.

There is deliberately **no** `<dir>/README.md` branch. Mutation testing found it unreachable —
a link to a directory already resolves on the `-e` line — and an unreachable branch presented as
a rule is worse than no rule.

### Self-defense

Both scripts assert that `ALL_CHECKS` names every check function the file defines and that every
registered check is targeted by at least one negative fixture. This is FR-134's finding — a gate
silently removed from its runner is the commonest degradation — applied to the gates themselves.

Every check was mutation-tested by neutering it and confirming its fixtures fail, and every
resolution rule in the link gate was mutation-tested by deleting it and confirming a positive
control fails. Those runs are recorded in [QA 181](../../qa/orchestrator/181-docs-publishing-integrity.md).

## Decisions and their alternatives

**Recover the English rather than publish one language.** Dropping `site/en/showcases` would have
been the smallest change and would have discarded 17 real translations while leaving the EN site
advertising a Showcases section in its nav.

**Declare translation gaps rather than write the translations.** Two showcases (~750 lines) would
have needed machine translation presented as source. The debt is recorded with a reason instead,
and an *undeclared* gap fails the gate, so a new showcase cannot quietly ship monolingual.

**Gate navigation reachability and change the nav.** The FR's non-goal said not to. Honoring it
would have satisfied the acceptance criterion on disk and for no reader.

**Fix one broken link, not three.** The other two are correct as written. Two positive fixtures
assert the gate does not report them; "fixing" them would have been damage.

## Known limits

- **The language switcher 404s on the seven asymmetric guide chapters** (six EN-only, one
  ZH-only). VitePress renders a locale-switch link on every page regardless of whether the other
  locale has it. This predates FR-131 — the pages were already published — and closing it means
  translating them, not changing the publishing rule. Recorded in the policy's
  `requireBilingualReason` so it is a known state rather than a surprise.
- **EN and ZH showcase content still diverges** beyond translation. `webhook-integration` is the
  clearest case: the English has a Plugin Policy Governance section the Chinese lacked until this
  work ported it back, and the Chinese has an FR-099 Slack source note and expanded manifest
  examples the English lacks. Content parity is the `guide-alignment` skill's scope and an
  explicit FR-131 non-goal; the gate asserts a page *exists* in both locales, never that it says
  the same thing.
- **Generated ZH pages lose the translated link text** the hand-maintained pages had
  (`[Quick Start]` had been rewritten to `[快速开始]`). A generator cannot reproduce that. The
  alternative is hand-maintained pages, which is the defect.
- **The link gate does not resolve anchors**, only paths. `file.md#no-such-heading` passes.
  Anchor resolution needs a heading-slug model per renderer and would trade a class of false
  negatives for a class of false positives.
- **Reference-style links (`[text][ref]`) are not extracted.** The repository has none; if one is
  ever written, the gate is silent rather than wrong.
