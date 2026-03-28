//! Data-driven step convention registry.
//!
//! Instead of hardcoding SDLC step defaults in Rust match arms, this module
//! loads conventions from a compiled-in YAML file and exposes them through a
//! singleton [`CONVENTIONS`] registry.  The framework accepts *any* step ID;
//! if the ID has no entry here and no explicit configuration, the universal
//! fallback rule applies: `required_capability = step_id`.

use super::{CaptureDecl, CaptureSource, PostAction, StepScope};
use serde::Deserialize;
use std::collections::HashMap;
use std::sync::LazyLock;

/// Singleton convention registry — immutable compile-time data.
pub static CONVENTIONS: LazyLock<StepConventionRegistry> =
    LazyLock::new(StepConventionRegistry::builtin);

// ── Convention types ─────────────────────────────────────────────────

/// Defaults for a well-known step ID, loaded from convention YAML.
#[derive(Debug, Clone, Default)]
pub struct StepConvention {
    /// Builtin name (only for true framework builtins with Rust impls).
    pub builtin: Option<String>,
    /// Default execution scope.
    pub scope: Option<StepScope>,
    /// Default is_guard flag.
    pub is_guard: bool,
    /// Default collect_artifacts flag.
    pub collect_artifacts: bool,
    /// Default captures to inject when user hasn't configured them.
    pub captures: Vec<CaptureDecl>,
    /// Default post_actions to inject when user hasn't configured them.
    pub post_actions: Vec<PostAction>,
}

/// Registry of step conventions, keyed by step ID.
#[derive(Debug, Default)]
pub struct StepConventionRegistry {
    conventions: HashMap<String, StepConvention>,
}

impl StepConventionRegistry {
    /// Build the registry from the compiled-in SDLC conventions YAML.
    fn builtin() -> Self {
        let yaml = include_str!("sdlc_conventions.yaml");
        let raw: RawConventions = match serde_yaml::from_str(yaml) {
            Ok(v) => v,
            // Compiled-in YAML — parse failure means a build-time bug.
            Err(_) => return Self::default(),
        };

        let mut conventions = HashMap::new();
        for (id, entry) in raw.steps {
            let scope = entry.scope.as_deref().map(|s| match s {
                "item" => StepScope::Item,
                _ => StepScope::Task,
            });

            let captures = entry
                .captures
                .into_iter()
                .filter_map(|c| {
                    let source = match c.source.as_str() {
                        "failed_flag" => CaptureSource::FailedFlag,
                        "success_flag" => CaptureSource::SuccessFlag,
                        "stdout" => CaptureSource::Stdout,
                        "stderr" => CaptureSource::Stderr,
                        "exit_code" => CaptureSource::ExitCode,
                        _ => return None,
                    };
                    Some(CaptureDecl {
                        var: c.var,
                        source,
                        json_path: None,
                    })
                })
                .collect();

            let post_actions = entry
                .post_actions
                .into_iter()
                .filter_map(|a| match a.as_str() {
                    "create_ticket" => Some(PostAction::CreateTicket),
                    "scan_tickets" => Some(PostAction::ScanTickets),
                    _ => None,
                })
                .collect();

            conventions.insert(
                id,
                StepConvention {
                    builtin: entry.builtin,
                    scope,
                    is_guard: entry.is_guard,
                    collect_artifacts: entry.collect_artifacts,
                    captures,
                    post_actions,
                },
            );
        }

        Self { conventions }
    }

    /// Look up a convention entry by step ID.
    pub fn lookup(&self, step_id: &str) -> Option<&StepConvention> {
        self.conventions.get(step_id)
    }

    /// Returns the default scope for a step ID.
    /// Falls back to `StepScope::Task` when no convention entry exists.
    pub fn default_scope(&self, step_id: &str) -> StepScope {
        self.conventions
            .get(step_id)
            .and_then(|c| c.scope)
            .unwrap_or(StepScope::Task)
    }

    /// Returns the builtin name for a step ID, if it maps to a framework builtin.
    pub fn builtin_name(&self, step_id: &str) -> Option<String> {
        self.conventions
            .get(step_id)
            .and_then(|c| c.builtin.clone())
    }

    /// Returns `true` when the step ID maps to a framework builtin with a Rust impl.
    pub fn is_known_builtin(&self, step_id: &str) -> bool {
        self.conventions
            .get(step_id)
            .and_then(|c| c.builtin.as_ref())
            .is_some()
    }
}

// ── Raw serde types for the YAML file ────────────────────────────────

#[derive(Deserialize)]
struct RawConventions {
    steps: HashMap<String, RawStepConvention>,
}

#[derive(Deserialize)]
struct RawStepConvention {
    #[serde(default)]
    builtin: Option<String>,
    #[serde(default)]
    scope: Option<String>,
    #[serde(default)]
    is_guard: bool,
    #[serde(default)]
    collect_artifacts: bool,
    #[serde(default)]
    captures: Vec<RawCapture>,
    #[serde(default)]
    post_actions: Vec<String>,
}

#[derive(Deserialize)]
struct RawCapture {
    var: String,
    source: String,
}

// ── Tests ────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtin_conventions_parse() {
        let registry = StepConventionRegistry::builtin();
        // All 22 well-known steps are present
        assert!(registry.lookup("init_once").is_some());
        assert!(registry.lookup("plan").is_some());
        assert!(registry.lookup("qa").is_some());
        assert!(registry.lookup("ticket_scan").is_some());
        assert!(registry.lookup("fix").is_some());
        assert!(registry.lookup("retest").is_some());
        assert!(registry.lookup("loop_guard").is_some());
        assert!(registry.lookup("build").is_some());
        assert!(registry.lookup("test").is_some());
        assert!(registry.lookup("lint").is_some());
        assert!(registry.lookup("implement").is_some());
        assert!(registry.lookup("review").is_some());
        assert!(registry.lookup("git_ops").is_some());
        assert!(registry.lookup("qa_doc_gen").is_some());
        assert!(registry.lookup("qa_testing").is_some());
        assert!(registry.lookup("ticket_fix").is_some());
        assert!(registry.lookup("doc_governance").is_some());
        assert!(registry.lookup("align_tests").is_some());
        assert!(registry.lookup("self_test").is_some());
        assert!(registry.lookup("self_restart").is_some());
        assert!(registry.lookup("smoke_chain").is_some());
        assert!(registry.lookup("evaluate").is_some());
        assert!(registry.lookup("item_select").is_some());
    }

    #[test]
    fn framework_builtins_detected() {
        let registry = StepConventionRegistry::builtin();
        for name in &[
            "init_once",
            "loop_guard",
            "ticket_scan",
            "self_test",
            "self_restart",
            "item_select",
        ] {
            assert!(
                registry.is_known_builtin(name),
                "{name} should be a known builtin"
            );
        }
        // SDLC agent steps are NOT builtins
        for name in &["plan", "qa", "fix", "qa_doc_gen", "ticket_fix"] {
            assert!(
                !registry.is_known_builtin(name),
                "{name} should NOT be a builtin"
            );
        }
    }

    #[test]
    fn scope_defaults() {
        let registry = StepConventionRegistry::builtin();
        assert_eq!(registry.default_scope("plan"), StepScope::Task);
        assert_eq!(registry.default_scope("qa"), StepScope::Item);
        assert_eq!(registry.default_scope("qa_testing"), StepScope::Item);
        assert_eq!(registry.default_scope("ticket_fix"), StepScope::Item);
        assert_eq!(registry.default_scope("fix"), StepScope::Item);
        assert_eq!(registry.default_scope("retest"), StepScope::Item);
        assert_eq!(registry.default_scope("implement"), StepScope::Task);
        // Unknown step ID falls back to Task
        assert_eq!(registry.default_scope("my_custom_step"), StepScope::Task);
    }

    #[test]
    fn qa_step_has_captures_and_post_actions() {
        let registry = StepConventionRegistry::builtin();
        let qa = registry.lookup("qa").unwrap();
        assert!(qa.collect_artifacts);
        assert_eq!(qa.captures.len(), 1);
        assert_eq!(qa.captures[0].var, "qa_failed");
        assert_eq!(qa.captures[0].source, CaptureSource::FailedFlag);
        assert_eq!(qa.post_actions.len(), 1);
        assert_eq!(qa.post_actions[0], PostAction::CreateTicket);
    }

    #[test]
    fn fix_step_has_captures() {
        let registry = StepConventionRegistry::builtin();
        let fix = registry.lookup("fix").unwrap();
        assert!(!fix.collect_artifacts);
        assert_eq!(fix.captures.len(), 1);
        assert_eq!(fix.captures[0].var, "fix_success");
        assert_eq!(fix.captures[0].source, CaptureSource::SuccessFlag);
        assert!(fix.post_actions.is_empty());
    }

    #[test]
    fn unknown_step_returns_none() {
        let registry = StepConventionRegistry::builtin();
        assert!(registry.lookup("my_custom_deploy").is_none());
    }
}
