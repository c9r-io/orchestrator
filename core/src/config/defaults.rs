use super::{
    StepBehavior, StepHookEngine, WorkflowFinalizeConfig, WorkflowFinalizeRule, WorkflowStepConfig,
};

fn step_config(
    id: &str,
    required_capability: Option<&str>,
    builtin: Option<&str>,
    enabled: bool,
    repeatable: bool,
    tty: bool,
) -> WorkflowStepConfig {
    WorkflowStepConfig {
        id: id.to_string(),
        description: None,
        required_capability: required_capability.map(String::from),
        builtin: builtin.map(String::from),
        enabled,
        repeatable,
        is_guard: false,
        cost_preference: None,
        prehook: None,
        tty,
        template: None,
        outputs: Vec::new(),
        pipe_to: None,
        command: None,
        chain_steps: vec![],
        scope: None,
        behavior: StepBehavior::default(),
        max_parallel: None,
        timeout_secs: None,
        item_select_config: None,
        store_inputs: vec![],
        store_outputs: vec![],
    }
}

/// Default workflow steps builder
pub fn default_workflow_steps(
    qa: Option<&str>,
    ticket_scan: bool,
    fix: Option<&str>,
    retest: Option<&str>,
) -> Vec<WorkflowStepConfig> {
    vec![
        step_config("init_once", None, Some("init_once"), false, false, false),
        step_config("plan", Some("plan"), None, false, false, true),
        step_config("qa", Some("qa"), None, qa.is_some(), true, false),
        step_config(
            "ticket_scan",
            None,
            Some("ticket_scan"),
            ticket_scan,
            true,
            false,
        ),
        step_config("fix", Some("fix"), None, fix.is_some(), true, false),
        step_config(
            "retest",
            Some("retest"),
            None,
            retest.is_some(),
            true,
            false,
        ),
    ]
}

/// Default workflow finalize config
pub fn default_workflow_finalize_config() -> WorkflowFinalizeConfig {
    WorkflowFinalizeConfig {
        rules: vec![
            WorkflowFinalizeRule {
                id: "skip_without_tickets".to_string(),
                engine: StepHookEngine::Cel,
                when: "qa_skipped == true && active_ticket_count == 0 && is_last_cycle"
                    .to_string(),
                status: "skipped".to_string(),
                reason: Some("qa skipped and no tickets".to_string()),
            },
            WorkflowFinalizeRule {
                id: "qa_passed_without_tickets".to_string(),
                engine: StepHookEngine::Cel,
                when: "qa_ran == true && qa_exit_code == 0 && active_ticket_count == 0"
                    .to_string(),
                status: "qa_passed".to_string(),
                reason: Some("qa passed with no tickets".to_string()),
            },
            WorkflowFinalizeRule {
                id: "fix_disabled_with_tickets".to_string(),
                engine: StepHookEngine::Cel,
                when: "fix_enabled == false && active_ticket_count > 0".to_string(),
                status: "unresolved".to_string(),
                reason: Some("fix disabled by workflow".to_string()),
            },
            WorkflowFinalizeRule {
                id: "fix_failed".to_string(),
                engine: StepHookEngine::Cel,
                when: "fix_ran == true && fix_success == false".to_string(),
                status: "unresolved".to_string(),
                reason: Some("fix failed".to_string()),
            },
            WorkflowFinalizeRule {
                id: "fixed_without_retest".to_string(),
                engine: StepHookEngine::Cel,
                when: "fix_success == true && retest_enabled == false".to_string(),
                status: "fixed".to_string(),
                reason: Some("fixed without retest".to_string()),
            },
            WorkflowFinalizeRule {
                id: "fix_skipped_and_retest_disabled".to_string(),
                engine: StepHookEngine::Cel,
                when: "fix_enabled == true && fix_ran == false && fix_skipped == false && fix_success == false && retest_enabled == false && active_ticket_count > 0".to_string(),
                status: "unresolved".to_string(),
                reason: Some("fix did not run (not skipped by prehook) and retest disabled".to_string()),
            },
            WorkflowFinalizeRule {
                id: "fixed_retest_skipped_after_fix_success".to_string(),
                engine: StepHookEngine::Cel,
                when: "retest_enabled == true && retest_ran == false && fix_success == true"
                    .to_string(),
                status: "fixed".to_string(),
                reason: Some("retest skipped by prehook".to_string()),
            },
            WorkflowFinalizeRule {
                id: "unresolved_retest_skipped_without_fix".to_string(),
                engine: StepHookEngine::Cel,
                when: "retest_enabled == true && retest_ran == false && fix_success == false && active_ticket_count > 0".to_string(),
                status: "unresolved".to_string(),
                reason: Some("fix skipped by prehook and retest skipped by prehook".to_string()),
            },
            WorkflowFinalizeRule {
                id: "verified_after_retest".to_string(),
                engine: StepHookEngine::Cel,
                when: "retest_ran == true && retest_success == true && retest_new_ticket_count == 0"
                    .to_string(),
                status: "verified".to_string(),
                reason: Some("retest passed".to_string()),
            },
            WorkflowFinalizeRule {
                id: "unresolved_after_retest".to_string(),
                engine: StepHookEngine::Cel,
                when: "retest_ran == true && (retest_success == false || retest_new_ticket_count > 0)"
                    .to_string(),
                status: "unresolved".to_string(),
                reason: Some("retest still failing".to_string()),
            },
            WorkflowFinalizeRule {
                id: "fallback_unresolved_with_tickets".to_string(),
                engine: StepHookEngine::Cel,
                when: "active_ticket_count > 0".to_string(),
                status: "unresolved".to_string(),
                reason: Some("unresolved tickets remain".to_string()),
            },
            WorkflowFinalizeRule {
                id: "fallback_qa_passed".to_string(),
                engine: StepHookEngine::Cel,
                when: "active_ticket_count == 0".to_string(),
                status: "qa_passed".to_string(),
                reason: Some("no active tickets".to_string()),
            },
        ],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_workflow_steps_all_disabled() {
        let steps = default_workflow_steps(None, false, None, None);
        assert_eq!(steps.len(), 6);
        // qa disabled
        let qa = steps
            .iter()
            .find(|s| s.id == "qa")
            .expect("qa step should exist");
        assert!(!qa.enabled);
        // ticket_scan disabled
        let ts = steps
            .iter()
            .find(|s| s.id == "ticket_scan")
            .expect("ticket_scan step should exist");
        assert!(!ts.enabled);
        // fix disabled
        let fix = steps
            .iter()
            .find(|s| s.id == "fix")
            .expect("fix step should exist");
        assert!(!fix.enabled);
        // retest disabled
        let retest = steps
            .iter()
            .find(|s| s.id == "retest")
            .expect("retest step should exist");
        assert!(!retest.enabled);
    }

    #[test]
    fn test_default_workflow_steps_all_enabled() {
        let steps = default_workflow_steps(
            Some("qa_agent"),
            true,
            Some("fix_agent"),
            Some("retest_agent"),
        );
        let qa = steps
            .iter()
            .find(|s| s.id == "qa")
            .expect("qa step should exist");
        assert!(qa.enabled);
        let ts = steps
            .iter()
            .find(|s| s.id == "ticket_scan")
            .expect("ticket_scan step should exist");
        assert!(ts.enabled);
        let fix = steps
            .iter()
            .find(|s| s.id == "fix")
            .expect("fix step should exist");
        assert!(fix.enabled);
        let retest = steps
            .iter()
            .find(|s| s.id == "retest")
            .expect("retest step should exist");
        assert!(retest.enabled);
    }

    #[test]
    fn test_default_workflow_steps_tty_flags() {
        let steps = default_workflow_steps(None, false, None, None);
        // only plan should have tty=true
        let plan = steps
            .iter()
            .find(|s| s.id == "plan")
            .expect("plan step should exist");
        assert!(plan.tty);
        for s in steps.iter().filter(|s| s.id != "plan") {
            assert!(!s.tty, "step {} should not have tty", s.id);
        }
    }

    #[test]
    fn test_default_workflow_steps_repeatable() {
        let steps = default_workflow_steps(Some("qa"), true, Some("fix"), Some("retest"));
        let init = steps
            .iter()
            .find(|s| s.id == "init_once")
            .expect("init_once step should exist");
        assert!(!init.repeatable);
        let plan = steps
            .iter()
            .find(|s| s.id == "plan")
            .expect("plan step should exist");
        assert!(!plan.repeatable);
        // qa, ticket_scan, fix, retest are repeatable
        for id in &["qa", "ticket_scan", "fix", "retest"] {
            let s = steps
                .iter()
                .find(|s| s.id == *id)
                .expect("repeatable step should exist");
            assert!(s.repeatable, "step {} should be repeatable", id);
        }
    }

    #[test]
    fn test_default_workflow_finalize_config_rule_count() {
        let cfg = default_workflow_finalize_config();
        assert_eq!(cfg.rules.len(), 12);
    }

    #[test]
    fn test_default_workflow_finalize_config_skip_without_tickets_has_is_last_cycle() {
        let cfg = default_workflow_finalize_config();
        let rule = cfg
            .rules
            .iter()
            .find(|r| r.id == "skip_without_tickets")
            .expect("skip_without_tickets rule should exist");
        assert!(
            rule.when.contains("is_last_cycle"),
            "skip_without_tickets must include is_last_cycle guard"
        );
        assert_eq!(rule.status, "skipped");
    }

    #[test]
    fn test_default_workflow_finalize_config_rule_ids_unique() {
        let cfg = default_workflow_finalize_config();
        let mut ids: Vec<&str> = cfg.rules.iter().map(|r| r.id.as_str()).collect();
        let original_len = ids.len();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), original_len, "finalize rule IDs must be unique");
    }

    #[test]
    fn test_default_workflow_finalize_config_all_rules_have_reasons() {
        let cfg = default_workflow_finalize_config();
        for rule in &cfg.rules {
            assert!(
                rule.reason.is_some(),
                "rule {} should have a reason",
                rule.id
            );
        }
    }

    #[test]
    fn test_default_workflow_finalize_config_fallback_rules_last() {
        let cfg = default_workflow_finalize_config();
        let last_two: Vec<&str> = cfg
            .rules
            .iter()
            .rev()
            .take(2)
            .map(|r| r.id.as_str())
            .collect();
        assert!(last_two.contains(&"fallback_qa_passed"));
        assert!(last_two.contains(&"fallback_unresolved_with_tickets"));
    }

    #[test]
    fn test_step_config_helper() {
        let s = step_config("my_id", Some("build"), None, true, false, false);
        assert_eq!(s.id, "my_id");
        assert_eq!(s.required_capability, Some("build".to_string()));
        assert!(s.builtin.is_none());
        assert!(s.enabled);
        assert!(!s.repeatable);
        assert!(!s.is_guard);
        assert!(s.cost_preference.is_none());
        assert!(s.prehook.is_none());
        assert!(!s.tty);
        assert!(s.outputs.is_empty());
        assert!(s.pipe_to.is_none());
        assert!(s.command.is_none());
        assert!(s.chain_steps.is_empty());
        assert!(s.scope.is_none());
    }
}
