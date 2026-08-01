//! FR-152: no new driverless Agent fixture.
//!
//! The corpus taught the deprecated form at scale: 137 of 147 `kind: Agent`
//! fixture documents omitted `spec.driver`, so every copy-paste started from
//! a shape that warns at apply time. After the one-off migration, this module
//! keeps the corpus modern: a driverless Agent document is a violation unless
//! its own chunk carries a `# fixture-driverless-exempt: <reason>` comment —
//! the machine-parseable convention for "a live gate asserts the legacy
//! warning on this exact document" (see fixtures/manifests/README.md).
//!
//! Same discipline as [`crate::fixture_corpus_tests`]:
//!
//! * **Scope is derived, never listed** — `git ls-files '*.yaml' '*.yml'`
//!   over the whole repository, so bundles, benchmarks, workflow fixtures and
//!   the integration-test manifests are all in scope the day they land. An
//!   empty result fails; a subtree exclusion must still match something or it
//!   fails as stale (§4.4 shape 8: a blanket that outlives its reason).
//! * **The evaluator is pure** so the negative tests below drive it with
//!   mutated corpora instead of touching the repository.
//! * **Exemptions can go stale in both directions**: a document that gained
//!   `driver:` while keeping the exempt comment is a violation too — the
//!   comment would otherwise keep authorizing a legacy form the document no
//!   longer has.

use std::path::{Path, PathBuf};

/// Subtrees deliberately outside this gate, each with the reason and a
/// staleness assertion: the moment the subtree stops matching any tracked
/// file, the exclusion itself is reported so it cannot silently outlive its
/// premise.
const EXCLUDED_PREFIXES: &[(&str, &str)] = &[(
    "test-yaml-warnings/",
    "scheduled for deletion by FR-155; churning files on death row breaks the FR-155 diff",
)];

const EXEMPT_MARK: &str = "# fixture-driverless-exempt:";

/// One `kind: Agent` document as this gate sees it.
#[derive(Debug, Clone)]
struct AgentDoc {
    path: String,
    name: String,
    has_driver: bool,
    /// `Some(reason)` when the document's chunk carries the exempt comment;
    /// the reason may be empty, which is its own violation.
    exempt_reason: Option<String>,
}

/// Splits a multi-document YAML file into chunks, keeping each `---`
/// separator (and any comment lines that follow it) with the document below
/// so a per-document exemption comment stays attached to its document.
fn split_documents(text: &str) -> Vec<String> {
    let mut chunks: Vec<Vec<&str>> = vec![Vec::new()];
    for line in text.lines() {
        if line.trim_end() == "---" {
            chunks.push(Vec::new());
        }
        chunks
            .last_mut()
            .expect("chunks starts non-empty")
            .push(line);
    }
    chunks
        .into_iter()
        .map(|lines| lines.join("\n"))
        .filter(|chunk| !chunk.trim().is_empty())
        .collect()
}

/// Parses one chunk into an [`AgentDoc`] when it is a `kind: Agent` document.
/// Unparseable chunks and other kinds return `None` — this gate's subject is
/// driver presence, not validity; `fixture_corpus_tests` owns validity.
fn agent_doc(path: &str, chunk: &str) -> Option<AgentDoc> {
    let body = chunk
        .trim_start_matches('\n')
        .strip_prefix("---")
        .map(|rest| rest.to_string())
        .unwrap_or_else(|| chunk.to_string());
    let value: serde_yaml::Value = serde_yaml::from_str(&body).ok()?;
    let mapping = value.as_mapping()?;
    if mapping.get("kind")?.as_str()? != "Agent" {
        return None;
    }
    let spec = mapping.get("spec")?.as_mapping()?;
    let name = mapping
        .get("metadata")
        .and_then(|m| m.as_mapping())
        .and_then(|m| m.get("name"))
        .and_then(|n| n.as_str())
        .unwrap_or("?")
        .to_string();
    let exempt_reason = chunk.lines().find_map(|line| {
        line.trim_start()
            .strip_prefix(EXEMPT_MARK)
            .map(|reason| reason.trim().to_string())
    });
    Some(AgentDoc {
        path: path.to_string(),
        name,
        has_driver: spec.contains_key("driver"),
        exempt_reason,
    })
}

/// Compares what the corpus contains against what the convention allows.
/// Returns every violation rather than the first.
fn evaluate(docs: &[AgentDoc], excluded_prefix_hits: &[(&str, usize)]) -> Vec<String> {
    let mut violations = Vec::new();
    for (prefix, hits) in excluded_prefix_hits {
        if *hits == 0 {
            violations.push(format!(
                "stale exclusion: no tracked yaml file starts with '{prefix}' — the excluded \
                 subtree is gone, delete the exclusion"
            ));
        }
    }
    for doc in docs {
        match (doc.has_driver, doc.exempt_reason.as_deref()) {
            (false, None) => violations.push(format!(
                "driverless Agent '{}' in {} has no `{EXEMPT_MARK} <reason>` comment: add a \
                 typed driver (driver: {{provider: shell, transport: cli}} for script agents) \
                 or, only when a live gate asserts the legacy warning on this document, an \
                 exemption comment naming that gate",
                doc.name, doc.path
            )),
            (false, Some("")) => violations.push(format!(
                "driverless Agent '{}' in {} carries an exemption with an empty reason",
                doc.name, doc.path
            )),
            (true, Some(_)) => violations.push(format!(
                "stale exemption: Agent '{}' in {} has both `driver:` and the exempt comment — \
                 delete the comment so it stops authorizing a legacy form the document no \
                 longer has",
                doc.name, doc.path
            )),
            _ => {}
        }
    }
    violations
}

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("core/ must have a parent")
        .to_path_buf()
}

/// The corpus, derived from the git index. Every failure is an assertion
/// failure; a skip would leave the suite green having compared nothing.
fn tracked_yaml(root: &Path) -> Vec<String> {
    let output = std::process::Command::new("git")
        .args(["ls-files", "-z", "*.yaml", "*.yml"])
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
        "no tracked yaml files: the corpus moved and this check is now scanning nothing"
    );
    paths
}

/// Reads the real corpus into (docs, exclusion hit counts).
fn observe(root: &Path) -> (Vec<AgentDoc>, Vec<(&'static str, usize)>) {
    let mut hits: Vec<(&'static str, usize)> = EXCLUDED_PREFIXES
        .iter()
        .map(|(prefix, _)| (*prefix, 0usize))
        .collect();
    let mut docs = Vec::new();
    for path in tracked_yaml(root) {
        if let Some(hit) = hits
            .iter_mut()
            .find(|(prefix, _)| path.starts_with(*prefix))
        {
            hit.1 += 1;
            continue;
        }
        let content = std::fs::read_to_string(root.join(&path))
            .unwrap_or_else(|error| panic!("cannot read {path}: {error}"));
        for chunk in split_documents(&content) {
            if let Some(doc) = agent_doc(&path, &chunk) {
                docs.push(doc);
            }
        }
    }
    (docs, hits)
}

/// The gate: every Agent document in the tracked corpus is typed or exempt.
#[test]
fn every_agent_fixture_is_typed_or_exempt() {
    let root = repo_root();
    let (docs, hits) = observe(&root);
    assert!(
        !docs.is_empty(),
        "no kind: Agent documents found in the tracked corpus — the scan is reading nothing"
    );
    let violations = evaluate(&docs, &hits);
    assert!(
        violations.is_empty(),
        "{} of {} Agent documents violate the driver convention:\n  {}",
        violations.len(),
        docs.len(),
        violations.join("\n  ")
    );
}

/// Negative fixture, target derived rather than named: take the first *typed*
/// Agent document the real corpus yields and comment out its `driver:` block —
/// commenting out, not deleting, because an author neutralizing the block
/// while iterating is the regression shape a deletion-tuned check would miss.
/// The mutated document must be rejected, and the diagnostic must name it.
#[test]
fn a_commented_out_driver_block_is_a_violation() {
    let root = repo_root();
    let (docs, _) = observe(&root);
    let victim = docs
        .iter()
        .find(|doc| doc.has_driver && doc.exempt_reason.is_none())
        .expect("the migrated corpus must contain at least one typed Agent document");

    let content = std::fs::read_to_string(root.join(&victim.path))
        .unwrap_or_else(|error| panic!("cannot read {}: {error}", victim.path));
    let mutated_docs: Vec<AgentDoc> = split_documents(&content)
        .iter()
        .map(|chunk| {
            if agent_doc(&victim.path, chunk)
                .map(|doc| doc.name == victim.name)
                .unwrap_or(false)
            {
                comment_out_driver_block(chunk)
            } else {
                chunk.clone()
            }
        })
        .filter_map(|chunk| agent_doc(&victim.path, &chunk))
        .collect();

    let mutated = mutated_docs
        .iter()
        .find(|doc| doc.name == victim.name)
        .expect("the mutated document must still parse as an Agent");
    assert!(
        !mutated.has_driver,
        "premise failed: commenting out the driver block of '{}' in {} did not remove it — \
         the mutation never applied and everything below would pass vacuously",
        victim.name, victim.path
    );

    let violations = evaluate(&mutated_docs, &[]);
    assert!(
        violations
            .iter()
            .any(|v| v.contains(&victim.name) && v.contains(&victim.path)),
        "a driverless, non-exempt Agent survived the gate; violations were:\n  {}",
        violations.join("\n  ")
    );
}

/// Comments out the `driver:` block of one document chunk: the `  driver:`
/// line and every following line more deeply indented than it.
fn comment_out_driver_block(chunk: &str) -> String {
    let mut out = Vec::new();
    let mut in_block = false;
    for line in chunk.lines() {
        if line.starts_with("  driver:") {
            in_block = true;
            out.push(format!("  # {}", line.trim_start()));
            continue;
        }
        if in_block {
            let indent = line.len() - line.trim_start().len();
            if !line.trim().is_empty() && indent > 2 {
                out.push(format!("  # {}", line.trim_start()));
                continue;
            }
            in_block = false;
        }
        out.push(line.to_string());
    }
    out.join("\n")
}

/// An exemption whose reason is empty authorizes nothing.
#[test]
fn an_empty_exemption_reason_is_a_violation() {
    let doc = AgentDoc {
        path: "fixtures/synthetic.yaml".to_string(),
        name: "empty-reason".to_string(),
        has_driver: false,
        exempt_reason: Some(String::new()),
    };
    let violations = evaluate(&[doc], &[]);
    assert!(
        violations.iter().any(|v| v.contains("empty reason")),
        "an empty exemption reason was accepted: {violations:?}"
    );
}

/// A document that gained `driver:` while keeping the exempt comment is
/// reported, so the comment cannot keep authorizing a form that is gone.
#[test]
fn a_typed_document_with_an_exempt_comment_is_a_violation() {
    let doc = AgentDoc {
        path: "fixtures/synthetic.yaml".to_string(),
        name: "typed-but-exempt".to_string(),
        has_driver: true,
        exempt_reason: Some("some historical reason".to_string()),
    };
    let violations = evaluate(&[doc], &[]);
    assert!(
        violations.iter().any(|v| v.contains("stale exemption")),
        "a typed document with an exempt comment was accepted: {violations:?}"
    );
}

/// An exclusion whose subtree no longer matches any tracked file is itself a
/// violation — the blanket must not outlive its reason (§4.4 shape 8).
#[test]
fn an_exclusion_matching_nothing_is_a_violation() {
    let violations = evaluate(&[], &[("gone-subtree/", 0)]);
    assert!(
        violations.iter().any(|v| v.contains("stale exclusion")),
        "an exclusion matching zero files was accepted: {violations:?}"
    );
}

/// The real exclusions must currently match tracked files, and the exempt
/// documents in the real corpus must each carry a non-empty reason naming
/// their asserting gate. This is the positive control for the negative tests
/// above: if it fails, their PASS results are void.
#[test]
fn real_exclusions_and_exemptions_are_live() {
    let root = repo_root();
    let (docs, hits) = observe(&root);
    for (prefix, count) in &hits {
        assert!(
            *count > 0,
            "exclusion '{prefix}' matches no tracked yaml file; delete it"
        );
    }
    for doc in docs.iter().filter(|d| d.exempt_reason.is_some()) {
        assert!(
            !doc.has_driver,
            "exempt Agent '{}' in {} is typed; the exemption is stale",
            doc.name, doc.path
        );
        assert!(
            !doc.exempt_reason.as_deref().unwrap_or("").is_empty(),
            "exempt Agent '{}' in {} has an empty reason",
            doc.name,
            doc.path
        );
    }
}
