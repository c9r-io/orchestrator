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

use crate::resource::Resource;
use crate::service::system::validate_manifests;
use crate::test_utils::TestState;
use serde::Deserialize;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

/// Every tracked YAML file is a candidate; what makes one a manifest is its
/// content, not its path.
const YAML_PATHSPECS: [&str; 2] = ["*.yaml", "*.yml"];

/// A manifest is a tracked YAML whose `apiVersion` names this product.
///
/// FR-176. The scope used to be `fixtures/manifests/bundles/*.yaml`, which is a
/// derived *listing* of one hard-coded *directory* — so whether a manifest was
/// governed depended on where it sat, and nothing declared which directories
/// should be. Measured at the time: 34 tracked manifests outside it across four
/// directories, 12 of them refused by the product, including three published
/// templates that had never applied even once.
///
/// **Both ends are named on purpose.** The obvious predicate — `orchestrator.dev/v2`
/// — is wrong in a direction that is easy to miss: `crd-test-invalid.yaml` is
/// `extensions.orchestrator.dev/v1`, is in the corpus today, and has a ledger
/// entry, so matching only v2 would drop it and orphan its declaration. Relaxing
/// the other way, to any `apiVersion:`, swallows the four Kubernetes manifests in
/// `project-bootstrap`'s template. Widening a matcher to catch what it missed
/// opens the opposite end unless the opposite end is stated (§4.4 shape 10).
///
/// Measured over the tree: 449 `orchestrator.dev/v2`, 2
/// `extensions.orchestrator.dev/v1`, 2 `apps/v1`, 2 `v1`.
const MANIFEST_API_MARKER: &str = "orchestrator.dev/";

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
            violations.push(format!(
                "declaration for {} has an empty reason",
                entry.path
            ));
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
    let mut args = vec!["ls-files", "-z"];
    args.extend_from_slice(&YAML_PATHSPECS);
    let output = std::process::Command::new("git")
        .args(&args)
        .current_dir(root)
        .output()
        .expect("git ls-files must run: the corpus scope is derived from the index");
    assert!(
        output.status.success(),
        "git ls-files failed ({}): {}",
        output.status,
        String::from_utf8_lossy(&output.stderr)
    );
    let candidates: Vec<String> = String::from_utf8(output.stdout)
        .expect("git ls-files emits utf-8 paths")
        .split('\0')
        .filter(|path| !path.is_empty())
        .map(str::to_string)
        .collect();
    assert!(
        !candidates.is_empty(),
        "git ls-files matched no YAML at all: the index is unreadable and this check is now \
         scanning nothing"
    );

    // Read to classify. A path predicate is what FR-176 removed, so this cannot
    // fall back to one: a file is a manifest because of what it declares.
    // Unreadable is a failure rather than a skip — a corpus that silently drops
    // what it cannot open is the green-and-worthless state above.
    let paths: Vec<String> = candidates
        .into_iter()
        .filter(|path| {
            let content = std::fs::read_to_string(root.join(path)).unwrap_or_else(|error| {
                panic!("cannot read tracked YAML {path}, so it cannot be classified: {error}")
            });
            content
                .lines()
                .any(|line| line.starts_with("apiVersion:") && line.contains(MANIFEST_API_MARKER))
        })
        .collect();
    assert!(
        !paths.is_empty(),
        "no tracked YAML declares an {MANIFEST_API_MARKER} apiVersion: the corpus moved and this \
         check is now scanning nothing"
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

/// FR-152: the quickstart bundle applies without compatibility warnings.
///
/// README.md and docs/guide/01-quickstart.md walk a new user through this
/// exact file, and a `[legacy_*]` warning on that first apply is the defect
/// FR-152 exists to remove. Corpus validity above only proves the bundle is
/// *accepted*; warnings are non-fatal and invisible to it. This collects them
/// the way the apply path does — `collect_warnings` on each dispatched
/// resource — rather than grepping the YAML for `driver:`, so the assertion
/// holds against what the product would print, not how the fixture is spelled.
///
/// FR-173 removed the Agent arm: a command-only Agent is now rejected by
/// validate rather than warned about, so corpus validity above already covers
/// it and a warning collector here would have nothing to collect.
#[test]
fn quickstart_bundle_applies_without_warnings() {
    let root = repo_root();
    let path = "fixtures/manifests/bundles/quickstart.yaml";
    let content = std::fs::read_to_string(root.join(path))
        .unwrap_or_else(|error| panic!("cannot read {path}: {error}"));
    let manifests = crate::resource::parse_manifests_from_yaml(&content)
        .unwrap_or_else(|error| panic!("{path} does not parse: {error}"));
    assert!(!manifests.is_empty(), "{path} parsed to zero documents");

    let mut warnings = Vec::new();
    let mut dispatched = 0usize;
    for manifest in manifests {
        let crate::crd::ParsedManifest::Builtin(resource) = manifest else {
            continue;
        };
        let registered = crate::resource::dispatch_resource(*resource)
            .unwrap_or_else(|error| panic!("{path} resource does not dispatch: {error}"));
        dispatched += 1;
        if let crate::resource::RegisteredResource::Workflow(wf) = &registered {
            warnings.extend(wf.collect_warnings());
        }
    }
    assert_eq!(
        dispatched, 3,
        "{path} should hold exactly the Workspace, Agent and Workflow the guide teaches"
    );
    assert!(
        warnings.is_empty(),
        "applying {path} would print compatibility warnings:\n  {}",
        warnings.join("\n  ")
    );
}

/// Extracts complete YAML fences from a Markdown document. A dangling fence
/// is a hard failure because accepting a truncated onboarding example would
/// make the behavior check vacuous.
fn fenced_yaml_blocks_of(source: &str, markdown: &str) -> Vec<String> {
    let mut blocks = Vec::new();
    let mut current: Option<Vec<&str>> = None;
    for line in markdown.lines() {
        match (line.trim(), current.as_mut()) {
            ("```yaml", None) => current = Some(Vec::new()),
            ("```", Some(_)) => {
                let block = current.take().expect("matched open YAML fence").join("\n");
                assert!(
                    !block.trim().is_empty(),
                    "{source} contains an empty YAML fence"
                );
                blocks.push(block);
            }
            (_, Some(lines)) => lines.push(line),
            _ => {}
        }
    }
    assert!(
        current.is_none(),
        "{source} contains an unclosed YAML fence"
    );
    blocks
}

fn fenced_yaml_blocks(markdown: &str) -> Vec<String> {
    fenced_yaml_blocks_of("AGENTS.md", markdown)
}

/// FR-166: the resource-model chapter documented two of a Trigger's four jobs.
/// The webhook and filesystem examples added for the other two are parsed out of
/// the chapter itself rather than restated here, because a copy in a test proves
/// the copy parses. `TriggerSpec` and its children declare `deny_unknown_fields`,
/// so a field the guide spells wrong is a rejection rather than a silent no-op --
/// which is what makes this worth asserting: `debounce_ms` is snake_case while
/// every webhook field beside it is camelCase, and nothing about the surrounding
/// YAML would tell an author that.
#[test]
fn guide_trigger_examples_deserialize_as_written() {
    use orchestrator_config::cli_types::TriggerSpec;

    let path = "docs/guide/02-resource-model.md";
    let content = std::fs::read_to_string(repo_root().join(path))
        .unwrap_or_else(|error| panic!("cannot read {path}: {error}"));

    // Derived from the chapter, not listed: any future `source:` example under a
    // `spec:` fence joins this set without anyone remembering to add it.
    let specs: Vec<String> = fenced_yaml_blocks_of(path, &content)
        .into_iter()
        .filter(|block| block.starts_with("spec:") && block.contains("  event:"))
        .collect();
    assert!(
        specs.len() >= 3,
        "{path} should document the lifecycle, webhook and filesystem event triggers; found {} spec fences",
        specs.len()
    );

    let mut sources = Vec::new();
    for block in &specs {
        let wrapper: serde_yaml::Value = serde_yaml::from_str(block)
            .unwrap_or_else(|error| panic!("{path} event example is not YAML: {error}\n{block}"));
        let spec = wrapper
            .get("spec")
            .expect("filtered on a spec: fence")
            .clone();
        let parsed: TriggerSpec = serde_yaml::from_value(spec).unwrap_or_else(|error| {
            panic!("{path} documents a Trigger field the product rejects: {error}\n{block}")
        });
        let event = parsed.event.expect("filtered on an event: key");
        sources.push(event.source.clone());
        match event.source.as_str() {
            "webhook" => assert!(
                event.webhook.is_some(),
                "the webhook example parsed without a webhook block, so it proves nothing"
            ),
            "filesystem" => {
                let fs = event
                    .filesystem
                    .expect("the filesystem example parsed without a filesystem block");
                assert_eq!(
                    fs.debounce_ms, 500,
                    "the documented debounce key did not reach the field"
                );
            }
            _ => {}
        }
    }
    for required in ["task_completed", "webhook", "filesystem"] {
        assert!(
            sources.iter().any(|source| source == required),
            "{path} no longer documents the {required} trigger; found {sources:?}"
        );
    }
}

/// FR-155: the repository's primary agent onboarding document must teach a
/// manifest that the product can validate and apply without compatibility
/// warnings. This intentionally drives typed parsing, dispatch, validation,
/// warning collection, and apply rather than treating a text grep as proof.
#[test]
fn agents_md_manifests_apply_without_legacy_warnings() {
    let root = repo_root();
    let path = "AGENTS.md";
    let content = std::fs::read_to_string(root.join(path))
        .unwrap_or_else(|error| panic!("cannot read {path}: {error}"));
    assert!(
        !content.contains("root_path"),
        "{path} must teach canonical Workspace spec.work_dir, not the compatibility alias"
    );

    let blocks = fenced_yaml_blocks(&content);
    assert!(
        !blocks.is_empty(),
        "{path} contains no complete YAML manifest fence"
    );

    let mut config = crate::config::OrchestratorConfig::default();
    let mut agent_count = 0usize;
    let mut warnings = Vec::new();
    let mut applied = 0usize;
    for (index, block) in blocks.iter().enumerate() {
        let manifests = crate::resource::parse_manifests_from_yaml(block).unwrap_or_else(|error| {
            panic!("{path} YAML fence {} does not parse: {error}", index + 1)
        });
        assert!(
            !manifests.is_empty(),
            "{path} YAML fence {} parsed to zero documents",
            index + 1
        );
        for manifest in manifests {
            let crate::crd::ParsedManifest::Builtin(resource) = manifest else {
                panic!(
                    "{path} YAML fence {} contains an unregistered custom resource",
                    index + 1
                );
            };
            let registered =
                crate::resource::dispatch_resource(*resource).unwrap_or_else(|error| {
                    panic!(
                        "{path} YAML fence {} resource does not dispatch: {error}",
                        index + 1
                    )
                });
            registered.validate().unwrap_or_else(|error| {
                panic!(
                    "{path} YAML fence {} resource is invalid: {error}",
                    index + 1
                )
            });
            match &registered {
                crate::resource::RegisteredResource::Workflow(workflow) => {
                    warnings.extend(workflow.collect_warnings());
                }
                crate::resource::RegisteredResource::Agent(agent) => {
                    agent_count += 1;
                    assert!(
                        agent.spec.driver.is_some(),
                        "Agent '{}' in {path} YAML fence {} has no typed driver",
                        agent.metadata.name,
                        index + 1
                    );
                }
                _ => {}
            }
            registered.apply(&mut config).unwrap_or_else(|error| {
                panic!(
                    "{path} YAML fence {} resource does not apply: {error}",
                    index + 1
                )
            });
            applied += 1;
        }
    }

    assert!(
        agent_count > 0,
        "{path} contains no Agent resource to teach"
    );
    assert!(
        applied >= agent_count,
        "{path} applied no complete resource set"
    );
    assert!(
        warnings.iter().all(|warning| !warning.contains("[legacy_")),
        "applying {path} examples would print legacy warnings:\n  {}",
        warnings.join("\n  ")
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
            // FR-173 deleted `behavior.captures` and gave StepBehavior
            // `deny_unknown_fields`, so the retirement diagnostic is now serde's
            // unknown-field error rather than a named `[legacy_*]` code. What the
            // fixture asserts is unchanged: the construct is refused, and the
            // refusal says which field.
            error.contains("captures")
        }),
        "expected the refusal to name the retired field; got: {:?}",
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

/// FR-176: the widening is only real if a manifest *outside* the old directory
/// is actually judged.
///
/// The fixture above derives its target as "the first accepted, undeclared
/// manifest", and `git ls-files` order puts `fixtures/manifests/bundles/` early
/// — so it can keep passing on a bundle while every newly-scoped directory goes
/// unexamined, which is exactly the state this FR exists to end. This one
/// therefore excludes the old root by construction and fails if no manifest
/// outside it is available to mutate.
///
/// It asserts the same two halves: the product refuses the injected construct by
/// name, and the evaluator surfaces the file as undeclared rot rather than
/// passing it through. Both are needed — a corpus that scanned the new
/// directories but had no ledger opinion about them would satisfy the first
/// alone.
#[test]
fn a_manifest_outside_the_old_bundle_root_is_judged_too() {
    const OLD_ROOT: &str = "fixtures/manifests/bundles/";

    let root = repo_root();
    let paths = tracked_bundles(&root);
    let observed = observe(&root, &paths);
    let ledger = load_ledger(&root);
    let declared: BTreeSet<&str> = ledger
        .bundles
        .iter()
        .map(|entry| entry.path.as_str())
        .collect();

    let outside: Vec<&String> = paths
        .iter()
        .filter(|path| !path.starts_with(OLD_ROOT))
        .collect();
    assert!(
        !outside.is_empty(),
        "the corpus scanned nothing outside {OLD_ROOT}, so the FR-176 widening is not in effect"
    );

    let target = outside
        .iter()
        .find(|path| {
            observed.get(**path) == Some(&Outcome::Valid) && !declared.contains(path.as_str())
        })
        .expect(
            "no accepted, undeclared manifest outside the old bundle root left to mutate — the \
             premise this fixture rests on no longer holds, which is a failure and not a reason \
             to skip",
        );

    let mut content = std::fs::read_to_string(root.join(target)).expect("read target");
    content.push_str(RETIRED_CONSTRUCT_DOCUMENT);

    let mut fixture = TestState::new().without_seeded_agents_and_workflows();
    let state = fixture.build();
    let report = validate_manifests(&state, &content, None).expect("validate mutated manifest");

    assert!(
        !report.valid,
        "{target} still validates after a retired construct was appended to it"
    );
    assert!(
        report.errors.iter().any(|error| error.contains("captures")),
        "expected the refusal to name the retired field; got: {:?}",
        report.errors
    );

    let mut mutated = observed.clone();
    mutated.insert((*target).clone(), Outcome::Invalid(report.errors));
    let violations = evaluate(&mutated, &ledger);
    assert!(
        violations
            .iter()
            .any(|violation| violation.contains("undeclared rejection")
                && violation.contains(*target)),
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
            vec![declaration(
                "a.yaml",
                Status::Environment,
                "work_dir not found",
            )],
        );
        let violations = evaluate(&observed, &ledger);
        assert_eq!(violations.len(), 1, "{violations:?}");
        assert!(
            violations[0].contains("stale declaration"),
            "{violations:?}"
        );
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
                "[example_rejection_code]",
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
                "[example_rejection_code] workflow 'w' step 's'".to_string(),
            ]),
        )]);
        let ledger = ledger(
            1,
            vec![declaration(
                "a.yaml",
                Status::Rotted,
                "[example_rejection_code]",
            )],
        );
        assert!(evaluate(&observed, &ledger).is_empty());
    }

    #[test]
    fn an_undeclared_rejection_is_a_violation() {
        let observed = BTreeMap::from([(
            "a.yaml".to_string(),
            Outcome::Invalid(vec!["[example_rejection_a] workflow 'w'".to_string()]),
        )]);
        let violations = evaluate(&observed, &ledger(0, vec![]));
        assert_eq!(violations.len(), 1, "{violations:?}");
        assert!(
            violations[0].contains("undeclared rejection")
                && violations[0].contains("example_rejection_a"),
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
                "[example_rejection_b] workflow 'second' step 'plan'".to_string(),
            ]),
        )]);
        let mut entry = declaration("a.yaml", Status::Intentional, "unused");
        entry.expect = vec![
            "[example_rejection_a] workflow 'first' step 'qa'".to_string(),
            "[example_rejection_b] workflow 'second' step 'plan'".to_string(),
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
