//! FR-148: does the product still accept what the fixture corpus declares?
//!
//! `scripts/qa/test-coordination-collapse.sh` could not finish for four days
//! because a bundle it applies carries `behavior.captures`, which DD-137
//! removed by design. The defect never needed the gate to run: the fixture
//! said `behavior.captures`, the validator rejected `behavior.captures`, and
//! nothing put the two side by side. This module is that comparison, and it
//! runs inside the existing `Rust test` job — no new governance step, no
//! budget.
//!
//! Two properties make it worth having rather than reassuring:
//!
//! * **Scope is derived, never listed.** The corpus comes from `git ls-files`,
//!   so a bundle added tomorrow is in scope tomorrow. A `git` that cannot run,
//!   or an empty result, fails the test — a corpus check that silently scans
//!   nothing is green and worthless (§4.4 shape 7).
//! * **A rejection must match its declared diagnostic**, not merely be a
//!   rejection. Capability validation runs *before* the retirement checks, so a
//!   bundle missing an agent fails with `no agent supports capability` and an
//!   exit code cannot tell that apart from the retirement it was supposed to
//!   demonstrate.

use crate::service::system::validate_manifests;
use crate::test_utils::TestState;
use serde::Deserialize;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

/// Pathspec for the corpus, relative to the repository root.
const BUNDLE_GLOB: &str = "fixtures/manifests/bundles/*.yaml";
/// The declaration every rejected bundle has to appear in.
const LEDGER_PATH: &str = "config/governance/fixture-bundle-validity.json";

/// What `validate_manifests` said about one bundle.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Outcome {
    Valid,
    Invalid(Vec<String>),
}

/// Why a bundle is allowed to be rejected.
#[derive(Debug, Deserialize, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
enum Status {
    /// Rejection is the point, and a live gate asserts it.
    Intentional,
    /// Declares a construct the product no longer accepts, and nobody wants it
    /// to. Ratcheted: the count is exact, so this set can only shrink.
    Rotted,
    /// Contents are fine; the bundle is not self-sufficient (a Workflow whose
    /// capabilities are supplied by an Agent applied from elsewhere).
    Fragment,
    /// Contents are fine; validity depends on an ambient path or a base policy
    /// the consuming gate installs first.
    Environment,
    /// Valid only once another bundle has been applied.
    Dependent,
}

/// One declared exception.
#[derive(Debug, Deserialize, Clone)]
struct Declaration {
    /// Repository-relative path, matching what `git ls-files` emits.
    path: String,
    status: Status,
    /// Substrings, at least one of which must appear in some error the
    /// validator returns.
    ///
    /// A list rather than a single string because a bundle carrying several
    /// retired constructs is rejected on whichever the merge reaches first, and
    /// the merge walks a `HashMap`. Every alternative is spelled out per
    /// workflow, so the entry still says which constructs the bundle holds —
    /// it does not degrade to matching the rule tag alone.
    expect: Vec<String>,
    /// Why this bundle must be rejected, in one sentence.
    reason: String,
    /// Who consumes it, if anyone. Empty means orphan.
    #[serde(default)]
    #[allow(dead_code)] // documentation for the reader, not an assertion input
    consumers: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct Ledger {
    /// How many `rotted` entries there are meant to be. Compared for equality,
    /// not as a ceiling: retiring rot has to move this number down.
    rotted_count: usize,
    bundles: Vec<Declaration>,
}

/// Compare what the product said against what the ledger declares.
///
/// Returns every violation rather than the first, so one run tells you the
/// whole story. Kept free of I/O so the negative fixtures below can drive it
/// with synthetic corpora instead of mutating the repository.
fn evaluate(observed: &BTreeMap<String, Outcome>, ledger: &Ledger) -> Vec<String> {
    let mut violations = Vec::new();
    let mut declared: BTreeMap<&str, &Declaration> = BTreeMap::new();
    for entry in &ledger.bundles {
        if declared.insert(entry.path.as_str(), entry).is_some() {
            violations.push(format!(
                "duplicate declaration: {} appears more than once; the later entry silently wins",
                entry.path
            ));
        }
    }

    for (path, outcome) in observed {
        match (outcome, declared.get(path.as_str())) {
            (Outcome::Valid, None) => {}
            (Outcome::Valid, Some(entry)) => violations.push(format!(
                "stale declaration: {path} is declared invalid ({:?}) but the product accepts it \
                 — delete the entry rather than leaving the reason to rot",
                entry.status
            )),
            (Outcome::Invalid(errors), None) => violations.push(format!(
                "undeclared rejection: {path} is rejected by the product and appears in no \
                 declaration: {}",
                summarize(errors)
            )),
            (Outcome::Invalid(errors), Some(entry)) => {
                let matched = entry
                    .expect
                    .iter()
                    .any(|expected| errors.iter().any(|error| error.contains(expected)));
                if !matched {
                    violations.push(format!(
                        "wrong diagnostic: {path} is declared to fail with one of {:?} but failed \
                         with: {} — it is rejected for a reason nobody wrote down",
                        entry.expect,
                        summarize(errors)
                    ));
                }
            }
        }
    }

    for entry in &ledger.bundles {
        if !observed.contains_key(&entry.path) {
            violations.push(format!(
                "declaration names a path outside the corpus: {}",
                entry.path
            ));
        }
        if entry.expect.is_empty() || entry.expect.iter().any(|e| e.trim().is_empty()) {
            violations.push(format!(
                "declaration for {} has an empty expect — an entry that expects nothing accepts \
                 every rejection, including tomorrow's",
                entry.path
            ));
        }
        if entry.reason.trim().is_empty() {
            violations.push(format!("declaration for {} has an empty reason", entry.path));
        }
    }

    let rotted = ledger
        .bundles
        .iter()
        .filter(|entry| entry.status == Status::Rotted)
        .count();
    if rotted != ledger.rotted_count {
        violations.push(format!(
            "rot ratchet: rotted_count says {} but {rotted} entries are declared rotted",
            ledger.rotted_count
        ));
    }

    violations
}

/// First two errors, joined. A bundle can fail several ways at once and the
/// order is not stable — the merge walks a `HashMap`.
fn summarize(errors: &[String]) -> String {
    if errors.is_empty() {
        return "(the validator reported no error text)".to_string();
    }
    errors
        .iter()
        .take(2)
        .map(|error| error.replace('\n', " "))
        .collect::<Vec<_>>()
        .join(" | ")
}

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("core/ must have a parent")
        .to_path_buf()
}

/// The corpus, derived from the git index.
///
/// Every failure here is an assertion failure. A skip would leave the suite
/// green having compared nothing.
fn tracked_bundles(root: &Path) -> Vec<String> {
    let output = std::process::Command::new("git")
        .args(["ls-files", "-z", BUNDLE_GLOB])
        .current_dir(root)
        .output()
        .expect("git ls-files must run: the corpus scope is derived from the index");
    assert!(
        output.status.success(),
        "git ls-files failed ({}): {}",
        output.status,
        String::from_utf8_lossy(&output.stderr)
    );
    let paths: Vec<String> = String::from_utf8(output.stdout)
        .expect("git ls-files emits utf-8 paths")
        .split('\0')
        .filter(|path| !path.is_empty())
        .map(str::to_string)
        .collect();
    assert!(
        !paths.is_empty(),
        "no bundles matched {BUNDLE_GLOB}: the corpus moved and this check is now scanning nothing"
    );
    paths
}

fn load_ledger(root: &Path) -> Ledger {
    let raw = std::fs::read_to_string(root.join(LEDGER_PATH))
        .unwrap_or_else(|error| panic!("cannot read {LEDGER_PATH}: {error}"));
    serde_json::from_str(&raw).unwrap_or_else(|error| panic!("cannot parse {LEDGER_PATH}: {error}"))
}

/// Validate every bundle against one shared state.
///
/// `validate_manifests` only reads state, so 93 calls share one `InnerState`:
/// 1.8s rather than the 24s a fresh `TestState` per bundle costs.
fn observe(root: &Path, paths: &[String]) -> BTreeMap<String, Outcome> {
    let mut fixture = TestState::new().without_seeded_agents_and_workflows();
    let state = fixture.build();
    paths
        .iter()
        .map(|path| {
            let content = std::fs::read_to_string(root.join(path))
                .unwrap_or_else(|error| panic!("cannot read {path}: {error}"));
            let report = validate_manifests(&state, &content, None)
                .unwrap_or_else(|error| panic!("validate_manifests errored on {path}: {error}"));
            let outcome = if report.valid {
                Outcome::Valid
            } else {
                Outcome::Invalid(report.errors)
            };
            (path.clone(), outcome)
        })
        .collect()
}

/// A Workflow document carrying a construct DD-137 retired.
///
/// The step sets `command`, which makes it self-contained: capability
/// validation is skipped and the `behavior.captures` check is the one that
/// fires. Without that, the bundle would fail with `no agent supports
/// capability` and the fixture would be asserting the wrong thing.
const RETIRED_CONSTRUCT_DOCUMENT: &str = r#"
---
apiVersion: orchestrator.dev/v2
kind: Workflow
metadata:
  name: fr148-retired-construct-probe
spec:
  steps:
    - id: probe
      type: probe
      enabled: true
      command: "true"
      behavior:
        captures:
          - var: probe_var
            source: exit_code
  loop:
    mode: once
"#;

#[test]
fn every_tracked_bundle_is_accepted_or_declared() {
    let root = repo_root();
    let paths = tracked_bundles(&root);
    let observed = observe(&root, &paths);
    let ledger = load_ledger(&root);

    let violations = evaluate(&observed, &ledger);
    assert!(
        violations.is_empty(),
        "{} of {} bundles disagree with {LEDGER_PATH}:\n  {}",
        violations.len(),
        paths.len(),
        violations.join("\n  ")
    );
}

/// The mutation is an *appended* Workflow rather than an edited step, because
/// that is the shape the regression actually takes: someone adds a workflow to
/// a bundle without knowing the construct is gone. Editing an existing step is
/// the case an author writing this check would have had in mind, so it proves
/// less.
///
/// The target is derived — the first accepted, undeclared bundle — never named.
/// A fixture that names its file goes stale the day the file moves, and §4.4
/// shape 7 records eight of nine such fixtures staying green while blind.
#[test]
fn an_injected_retired_construct_is_rejected_by_its_own_diagnostic() {
    let root = repo_root();
    let paths = tracked_bundles(&root);
    let observed = observe(&root, &paths);
    let ledger = load_ledger(&root);
    let declared: BTreeSet<&str> = ledger
        .bundles
        .iter()
        .map(|entry| entry.path.as_str())
        .collect();

    let target = paths
        .iter()
        .find(|path| {
            observed.get(*path) == Some(&Outcome::Valid) && !declared.contains(path.as_str())
        })
        .expect(
            "no accepted, undeclared bundle left to mutate — the premise this fixture rests on \
             no longer holds, which is a failure and not a reason to skip",
        );

    let mut content = std::fs::read_to_string(root.join(target)).expect("read target");
    content.push_str(RETIRED_CONSTRUCT_DOCUMENT);

    let mut fixture = TestState::new().without_seeded_agents_and_workflows();
    let state = fixture.build();
    let report = validate_manifests(&state, &content, None).expect("validate mutated bundle");

    assert!(
        !report.valid,
        "{target} still validates after a retired construct was appended to it"
    );
    assert!(
        report.errors.iter().any(|error| {
            error.contains("[legacy_coordination_removed]")
                && error.contains("fr148-retired-construct-probe")
        }),
        "expected the retirement diagnostic naming the injected workflow; got: {:?}",
        report.errors
    );

    // The diagnostic is only half of it: the evaluator has to surface the
    // bundle as undeclared rot rather than pass it through.
    let mut mutated = observed.clone();
    mutated.insert(target.clone(), Outcome::Invalid(report.errors));
    let violations = evaluate(&mutated, &ledger);
    assert!(
        violations
            .iter()
            .any(|violation| violation.contains("undeclared rejection")
                && violation.contains(target)),
        "evaluator did not report {target} as an undeclared rejection; got: {violations:?}"
    );
}

#[cfg(test)]
mod evaluator {
    use super::*;

    fn declaration(path: &str, status: Status, expect: &str) -> Declaration {
        Declaration {
            path: path.to_string(),
            status,
            expect: vec![expect.to_string()],
            reason: "a recorded reason".to_string(),
            consumers: Vec::new(),
        }
    }

    fn ledger(rotted_count: usize, bundles: Vec<Declaration>) -> Ledger {
        Ledger {
            rotted_count,
            bundles,
        }
    }

    #[test]
    fn an_accepted_undeclared_corpus_is_clean() {
        let observed = BTreeMap::from([("a.yaml".to_string(), Outcome::Valid)]);
        assert!(evaluate(&observed, &ledger(0, vec![])).is_empty());
    }

    #[test]
    fn a_declared_entry_that_now_validates_is_a_violation() {
        let observed = BTreeMap::from([("a.yaml".to_string(), Outcome::Valid)]);
        let ledger = ledger(
            0,
            vec![declaration("a.yaml", Status::Environment, "work_dir not found")],
        );
        let violations = evaluate(&observed, &ledger);
        assert_eq!(violations.len(), 1, "{violations:?}");
        assert!(violations[0].contains("stale declaration"), "{violations:?}");
    }

    #[test]
    fn failing_for_a_reason_nobody_declared_is_a_violation() {
        // The whole point of `expect`: this bundle *is* rejected and *is*
        // declared, so an exit-code check would call it fine. It is rejected
        // for the wrong reason.
        let observed = BTreeMap::from([(
            "a.yaml".to_string(),
            Outcome::Invalid(vec![
                "no agent supports capability for step 'qa' used by workflow 'w'".to_string(),
            ]),
        )]);
        let ledger = ledger(
            1,
            vec![declaration(
                "a.yaml",
                Status::Rotted,
                "[legacy_coordination_removed]",
            )],
        );
        let violations = evaluate(&observed, &ledger);
        assert_eq!(violations.len(), 1, "{violations:?}");
        assert!(violations[0].contains("wrong diagnostic"), "{violations:?}");
    }

    #[test]
    fn the_expected_diagnostic_may_appear_in_any_error_not_only_the_first() {
        // A bundle can fail several ways at once, and the order is not stable:
        // the merge walks a `HashMap`. Measured on cycle-overflow-test.yaml,
        // which named a different workflow on two consecutive runs.
        let observed = BTreeMap::from([(
            "a.yaml".to_string(),
            Outcome::Invalid(vec![
                "workspace 'w' work_dir not found: /nope".to_string(),
                "[legacy_json_path_removed] workflow 'w' step 's'".to_string(),
            ]),
        )]);
        let ledger = ledger(
            1,
            vec![declaration(
                "a.yaml",
                Status::Rotted,
                "[legacy_json_path_removed]",
            )],
        );
        assert!(evaluate(&observed, &ledger).is_empty());
    }

    #[test]
    fn an_undeclared_rejection_is_a_violation() {
        let observed = BTreeMap::from([(
            "a.yaml".to_string(),
            Outcome::Invalid(vec!["[legacy_coordination_removed] workflow 'w'".to_string()]),
        )]);
        let violations = evaluate(&observed, &ledger(0, vec![]));
        assert_eq!(violations.len(), 1, "{violations:?}");
        assert!(
            violations[0].contains("undeclared rejection")
                && violations[0].contains("legacy_coordination_removed"),
            "the violation has to carry the diagnostic, or the reader learns nothing: \
             {violations:?}"
        );
    }

    #[test]
    fn a_declaration_for_a_path_outside_the_corpus_is_a_violation() {
        let observed = BTreeMap::from([("a.yaml".to_string(), Outcome::Valid)]);
        let ledger = ledger(
            0,
            vec![declaration("deleted.yaml", Status::Fragment, "anything")],
        );
        let violations = evaluate(&observed, &ledger);
        assert!(
            violations
                .iter()
                .any(|violation| violation.contains("outside the corpus")),
            "{violations:?}"
        );
    }

    #[test]
    fn the_rot_ratchet_is_exact_in_both_directions() {
        let rotted = vec![declaration("a.yaml", Status::Rotted, "boom")];
        let observed = BTreeMap::from([(
            "a.yaml".to_string(),
            Outcome::Invalid(vec!["boom".to_string()]),
        )]);

        assert!(evaluate(&observed, &ledger(1, rotted.clone())).is_empty());

        for declared_count in [0usize, 2] {
            let violations = evaluate(&observed, &ledger(declared_count, rotted.clone()));
            assert!(
                violations
                    .iter()
                    .any(|violation| violation.contains("rot ratchet")),
                "rotted_count={declared_count} should have tripped the ratchet: {violations:?}"
            );
        }
    }

    #[test]
    fn a_reason_or_expect_left_blank_is_a_violation() {
        let observed = BTreeMap::from([(
            "a.yaml".to_string(),
            Outcome::Invalid(vec!["boom".to_string()]),
        )]);
        let mut blank = declaration("a.yaml", Status::Fragment, "boom");
        blank.reason = "   ".to_string();
        let violations = evaluate(&observed, &ledger(0, vec![blank]));
        assert!(
            violations
                .iter()
                .any(|violation| violation.contains("empty reason")),
            "{violations:?}"
        );

        // An empty `expect` list would match nothing, so the entry would report
        // "wrong diagnostic" forever — but an entry holding one blank string
        // matches *everything*, and that is the dangerous half: it turns the
        // declaration into a blanket acceptance of any future rejection.
        for empty in [Vec::new(), vec!["  ".to_string()]] {
            let mut entry = declaration("a.yaml", Status::Fragment, "boom");
            entry.expect = empty.clone();
            let violations = evaluate(&observed, &ledger(0, vec![entry]));
            assert!(
                violations
                    .iter()
                    .any(|violation| violation.contains("empty expect")),
                "expect={empty:?} should have been rejected: {violations:?}"
            );
        }
    }

    #[test]
    fn any_declared_alternative_may_be_the_one_that_fires() {
        // coordination-strangler-parity.yaml holds six workflows with retired
        // constructs and is rejected on whichever the merge reaches first.
        let observed = BTreeMap::from([(
            "a.yaml".to_string(),
            Outcome::Invalid(vec![
                "[legacy_json_path_removed] workflow 'second' step 'plan'".to_string(),
            ]),
        )]);
        let mut entry = declaration("a.yaml", Status::Intentional, "unused");
        entry.expect = vec![
            "[legacy_coordination_removed] workflow 'first' step 'qa'".to_string(),
            "[legacy_json_path_removed] workflow 'second' step 'plan'".to_string(),
        ];
        assert!(evaluate(&observed, &ledger(0, vec![entry.clone()])).is_empty());

        // ... but a rejection outside the declared set still fails, so the list
        // is an enumeration and not a wildcard.
        let elsewhere = BTreeMap::from([(
            "a.yaml".to_string(),
            Outcome::Invalid(vec!["workspace 'w' work_dir not found: /nope".to_string()]),
        )]);
        let violations = evaluate(&elsewhere, &ledger(0, vec![entry]));
        assert!(
            violations
                .iter()
                .any(|violation| violation.contains("wrong diagnostic")),
            "{violations:?}"
        );
    }

    #[test]
    fn a_duplicated_path_is_a_violation() {
        let observed = BTreeMap::from([(
            "a.yaml".to_string(),
            Outcome::Invalid(vec!["boom".to_string()]),
        )]);
        let ledger = ledger(
            0,
            vec![
                declaration("a.yaml", Status::Fragment, "boom"),
                declaration("a.yaml", Status::Fragment, "boom"),
            ],
        );
        let violations = evaluate(&observed, &ledger);
        assert!(
            violations
                .iter()
                .any(|violation| violation.contains("duplicate declaration")),
            "{violations:?}"
        );
    }
}
