# DD-141: Skill Single Source And Mirror Integrity

**Status**: Implemented (FR-129)
**Related**: DD-139 (gate enforcement surface), DD-140 (governance ledger regeneration), QA 179

## Background

`.claude/skills/` holds this repository's 29 skills. Other agent runtimes consume the same skills
through symlink mirrors so that there is one source and no copies. Until FR-129 nothing read those
mirrors, and the consequence was the failure mode this repository's governance work exists to
catch: a silent breakage in a load-bearing path, sitting undetected because no check looked.

The specific defect:

```
.agents/skills/fr-governance/SKILL.md -> ../../../.claude/skills/fr-governance   # a directory
.agents/skills/qa-doc-gen             -> ../../.claude/skills/qa-doc-gen         # every other entry
```

`fr-governance` was the only mirror shaped `<name>/SKILL.md` rather than `<name>`, and its
`SKILL.md` symlink resolved to a **directory**. Any runtime that opens
`.agents/skills/<name>/SKILL.md` — which is how skills are discovered — receives `EISDIR`. The FR
governance skill was unusable through that mirror for its entire life.

It is worth being precise about why this survived. Every structural property one would naturally
check was satisfied: the entry existed, the symlink resolved, the target was present, `git
ls-files -s` reported mode `120000`. A check written against any of those passes. Only opening the
file fails.

### What the FR asserted, and what was actually true

FR-129 was a proposal, and three of its claims did not survive verification against the codebase.
Recording them here because the FR file is deleted on closure.

| FR claim | Reality |
|---|---|
| "30 skills" | 29 skills plus `tools/`, a shared script bundle (`qa-api-test.sh`, `grpc-smoke.sh`, `grpcurl-docker.sh`) with no `SKILL.md`. It is not a skill, so it is declared `notSkills` rather than exempted — an exemption would imply it is a skill someone chose not to mirror. |
| `skills/orchestrator-guide/` is produced by `scripts/package-skills.sh` | It is not. That script reads `.claude/skills/orchestrator-guide` and writes `dist/orchestrator-skills-<tag>.tar.gz`. It has never written `skills/`. The directory had **no producer at all**. |
| The third copy is "easy to drift" | It had **already drifted**, by roughly 32KB. The tracked copy was missing the entire `orchestrator guide` CLI section and all ExecutionProfile / SecretStore / EnvStore / Trigger coverage. |

The FR also scoped itself to `.agents/skills/` alone. `.cursor/skills/` is a second tracked mirror
that held 16 of 29 skills, with `fr-governance` absent from it entirely, and `SKILLS.md`
documented a third mirror `.gemini/skills/` that did not exist on disk. The two real mirrors
carried arbitrarily different sets — `.agents` had `align-tests`, `orchestrator-guide`, and
`orchestrator-test-monitor`; `.cursor` had none of them. Neither was policy-driven. Closing the FR
as written would have fixed one directory and left the identical defect class alive in its
sibling, so the scope was widened to every declared mirror root.

## Design

### 1. The policy is data, and it is the only place a gap can be expressed

`config/governance/skill-mirrors.json` declares the source, the mirror roots, the entries that are
not skills, and the exemptions. It follows `qa-gate-surface.json`'s shape deliberately: a
self-describing JSON file that a deterministic gate reads, rather than a convention living in
someone's memory.

The default is mirror-everything. `exemptions` is empty and every one of the 29 skills is mirrored
into both roots. This is the point of the file: a skill that is not mirrored must be a written
decision with a reason, not an omission. FR-129's non-goal put it exactly right — *不能靠遗漏来
表达*. The gate rejects an exemption whose `reason` is under ten characters, because `"n/a"` is an
omission wearing a decision's clothes.

Exemptions are keyed by `(skill, mirrorRoot)` rather than by skill alone. A skill that genuinely
cannot run under one runtime can still be mirrored to another; an all-or-nothing exemption would
force an over-broad decision.

### 2. Six checks, one of which is a read

`scripts/qa/test-skill-mirror-integrity.sh`:

| Check | What it establishes |
|---|---|
| `check_source_inventory` | Every entry under `.claude/skills/` either declares a `SKILL.md` or is listed in `notSkills`. A helper directory appearing beside real skills, or a skill losing its `SKILL.md`, becomes a decision rather than drift. |
| `check_mirror_coverage` | Per root, in both directions: every skill is mirrored unless exempted for that root, and no mirror entry outlives its source skill. |
| `check_mirror_shape` | Every mirror entry is a symlink — not a directory, not a regular file, not a copy — whose target is the same-named source skill and exists. The expected relative target is *derived* from the root's depth rather than hardcoded to `../../`, so a mirror root at another depth is checked correctly rather than accidentally. |
| `check_skill_md_readable` | Opens `<root>/<name>/SKILL.md` the way a consuming runtime does and requires a non-empty regular file. |
| `check_no_stale_claims` | Every `notSkills` path and every exemption's skill still exists; every exemption's `mirrorRoot` is declared; every exemption carries a substantive reason. |
| `check_no_content_copies` | Every tracked `SKILL.md` in the repository lives under the source tree. Mirrors are symlinks, so git lists them as the link path and never as `<root>/<name>/SKILL.md`; a tracked `SKILL.md` anywhere else is therefore a real copy. |

`check_skill_md_readable` is the load-bearing one and the reason the other five are not
sufficient. The production defect passed every structural property; only the read failed. The
distinction is preserved in the fixture set, below, by a case in which shape is *perfect* and the
read still fails.

The separation between `source_skills()` (used for coverage) and `check_source_inventory` is
deliberate. Coverage's source set requires `SKILL.md` to *exist*; the read check requires it to be
a non-empty *regular file*. Collapsing the two would make a degraded `SKILL.md` trip several
checks at once and destroy each fixture's ability to isolate.

### 3. Fixtures prove failure, not just success

`--fixture-test` copies only the governed inputs into `mktemp -d` and injects one defect per case.
It uses `tar`, not `cp -R`, because the mirrors are symlinks and `cp -R` would flatten them into
directories — every fixture would then be exercising a tree that does not resemble the repository.
Two positive controls guard this: the unmodified copy must pass all six checks, and the copy must
still contain symlinks. The copy is also `git init`-ed and staged, because
`check_no_content_copies` asks git which `SKILL.md` files are tracked; the index alone answers
that, so nothing is committed and the repository is thrown away with the fixture root.

Each fixture asserts its target check fails **and** every other check passes, so a fixture proves
the check it names rather than tripping an earlier one.

| Fixture | Defect | Fails |
|---|---|---|
| 1 | New skill, mirrored nowhere, exempted nowhere | coverage |
| 2 | A mirror replaced by a real directory holding a real copy — readable, and still wrong | shape |
| 3 | A mirror pointing at a nonexistent target | shape + read |
| 4a | Shape perfect; the source `SKILL.md` is a directory | **read only** |
| 4b | The production defect verbatim: `<name>/SKILL.md -> <directory>` | shape + read |
| 5 | An exemption naming a skill that no longer exists | stale claims |
| 6 | A directory with no `SKILL.md` and no `notSkills` entry | inventory |
| 7 | An exemption whose reason explains nothing | stale claims |
| 8 | The deleted `skills/orchestrator-guide` copy, restored and tracked | content copies |

Fixtures 3 and 4b legitimately trip two checks each — a dangling or directory-shaped mirror is
both malformed and unreadable. Their expectation lists say so rather than papering over it, and
fixture 4a exists precisely so that `check_skill_md_readable` is proven **alone**, against a tree
where every structural check is green. That is the FR-129 defect reduced to its essence.

Fixture 4b is the regression fixture for the original bug, byte-for-byte.

### 4. The gate defends itself

FR-134 established that this repository's gates can degrade by substituting text existence for
execution fact, and that a gate silently removed from its runner is the most common shape of that
degradation. The same hazard applies here: deleting a check from `ALL_CHECKS` would stop it
running in verification mode, and the fixtures would not notice, because they invoke their targets
by name.

Two meta-assertions close it:

- `ALL_CHECKS` must name every `check_*` function the file defines (`grep` over `BASH_SOURCE`).
- Every check in `ALL_CHECKS` must be targeted by at least one negative fixture.

Both were mutation-tested during implementation. Dropping `check_skill_md_readable` from
`ALL_CHECKS` fails the first; neutering `check_skill_md_readable` or `check_mirror_shape` to
`return 0` fails three fixtures each.

### 5. The third copy is deleted, not pinned

FR-129 offered a choice: assert `skills/orchestrator-guide/**` identical to the authoritative
source, or remove it. Removal was chosen, because verification showed the copy had no producer, no
consumer, and had already drifted. Pinning it would have required a resync followed by a permanent
lockstep obligation, in exchange for a directory nothing reads. `skills/orchestrator-guide.skill`,
a zip archive of the same content, went with it — it could not have been content-compared by the
same mechanism anyway.

A deletion, however, is not a rule. Nothing about removing the directory stops an equivalent copy
from reappearing and drifting again, which is why `check_no_content_copies` exists: the invariant
that survives is "one tracked copy of a skill's content", not "this particular path is absent".
Fixture 8 restores the deleted copy and requires the gate to reject it.

`scripts/package-skills.sh` needed no change; it already sourced from `.claude/skills/`. Both user
guides already instruct users to install from `.claude/skills/orchestrator-guide`.

One statement elsewhere in the repository was falsified by the removal and repaired with it: the
FR-007 closure note in `docs/feature_request/README.md` named `skills/orchestrator-guide/**` as an
ongoing carrier of that FR's outcome. It now names `.claude/skills/orchestrator-guide/**`.

## Consequences

- Adding a skill now requires mirroring it into both roots or writing down why not. The gate names
  the missing root, so the work is mechanical.
- The mirror roots are declared in one file. Adding a runtime is a policy edit plus the symlinks,
  and the gate immediately holds the new root to the same standard.
- `SKILLS.md` no longer documents a `.gemini/skills/` mirror. It did not exist; the line was
  removed rather than an empty directory created, since nothing in this repository uses Gemini.
- The check that matters is a filesystem read, so it costs nothing and needs no daemon, no
  provider, and no database. It is classified `ci-required` with `providerIsolation: no-provider`
  in `config/governance/qa-gate-surface.json` and runs in the `ci.yml` governance job.

## Known Limits

- The gate verifies that a mirrored `SKILL.md` is a non-empty regular file. It does not parse the
  frontmatter or validate the skill's content; that is `qa-doc-lint.sh`'s and the authoring
  skills' territory.
- `notSkills` is a small escape hatch. An entry there is exempt from the `SKILL.md` requirement
  entirely, so a genuine skill mislabelled as `notSkills` would go unmirrored without complaint.
  The mitigation is that the file is short, reviewed, and each entry carries a reason; the gate
  only enforces that the path still exists.
- Mirror roots are matched by name equality against `.claude/skills` entries. On a
  case-insensitive filesystem two skills differing only in case would collide; no such pair
  exists, and the comparison is exact-string rather than existence-probing, so CI's
  case-sensitive filesystem sees the same result as macOS.
