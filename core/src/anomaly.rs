use serde::Serialize;

/// Severity assigned to an anomaly detected in task traces or runtime events.
#[derive(Debug, Serialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    /// The anomaly indicates a correctness or safety problem.
    Error,
    /// The anomaly indicates degraded or suspicious behavior.
    Warning,
    /// The anomaly is informational and may not require action.
    Info,
}

/// Recommended operator response for an anomaly.
#[derive(Debug, Serialize, Clone, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum Escalation {
    /// Record the anomaly without interrupting execution.
    Notice,
    /// Bring the anomaly to operator attention for follow-up.
    Attention,
    /// Interrupt or actively intervene in execution.
    Intervene,
}

impl Escalation {
    /// Returns a stable uppercase label for operator-facing displays.
    pub fn label(&self) -> &'static str {
        match self {
            Escalation::Notice => "NOTICE",
            Escalation::Attention => "ATTENTION",
            Escalation::Intervene => "INTERVENE",
        }
    }
}

/// Canonical anomaly rules emitted by trace analysis.
#[derive(Debug, Clone, PartialEq)]
pub enum AnomalyRule {
    /// A step produced too little output to be considered trustworthy.
    LowOutput,
    /// A step or cycle ran longer than expected.
    LongRunning,
    /// A transient read error interrupted trace collection.
    TransientReadError,
    /// More than one runner processed the same logical work.
    DuplicateRunner,
    /// Multiple workflow cycles overlapped unexpectedly.
    OverlappingCycles,
    /// Multiple steps overlapped unexpectedly.
    OverlappingSteps,
    /// A step start event was observed without a matching end event.
    MissingStepEnd,
    /// A workflow cycle completed without processing any steps.
    EmptyCycle,
    /// A command was observed without a matching task-step context.
    OrphanCommand,
    /// A command exited with a non-zero status.
    NonzeroExit,
    /// A templated variable remained unexpanded in emitted output.
    UnexpandedTemplateVar,
}

impl AnomalyRule {
    /// Returns the stable machine-readable name for the rule.
    pub fn canonical_name(&self) -> &'static str {
        match self {
            AnomalyRule::LowOutput => "low_output",
            AnomalyRule::LongRunning => "long_running",
            AnomalyRule::TransientReadError => "transient_read_error",
            AnomalyRule::DuplicateRunner => "duplicate_runner",
            AnomalyRule::OverlappingCycles => "overlapping_cycles",
            AnomalyRule::OverlappingSteps => "overlapping_steps",
            AnomalyRule::MissingStepEnd => "missing_step_end",
            AnomalyRule::EmptyCycle => "empty_cycle",
            AnomalyRule::OrphanCommand => "orphan_command",
            AnomalyRule::NonzeroExit => "nonzero_exit",
            AnomalyRule::UnexpandedTemplateVar => "unexpanded_template_var",
        }
    }

    /// Returns the default severity associated with the rule.
    pub fn default_severity(&self) -> Severity {
        match self {
            AnomalyRule::DuplicateRunner
            | AnomalyRule::OverlappingCycles
            | AnomalyRule::OverlappingSteps => Severity::Error,

            AnomalyRule::LowOutput
            | AnomalyRule::MissingStepEnd
            | AnomalyRule::OrphanCommand
            | AnomalyRule::NonzeroExit
            | AnomalyRule::UnexpandedTemplateVar
            | AnomalyRule::TransientReadError => Severity::Warning,

            AnomalyRule::LongRunning | AnomalyRule::EmptyCycle => Severity::Info,
        }
    }

    /// Returns the default escalation policy associated with the rule.
    pub fn escalation(&self) -> Escalation {
        match self {
            AnomalyRule::LowOutput
            | AnomalyRule::DuplicateRunner
            | AnomalyRule::OverlappingCycles
            | AnomalyRule::OverlappingSteps => Escalation::Intervene,

            AnomalyRule::NonzeroExit
            | AnomalyRule::OrphanCommand
            | AnomalyRule::MissingStepEnd
            | AnomalyRule::UnexpandedTemplateVar
            | AnomalyRule::TransientReadError => Escalation::Attention,

            AnomalyRule::LongRunning | AnomalyRule::EmptyCycle => Escalation::Notice,
        }
    }

    /// Returns the uppercase display tag used in reports and logs.
    pub fn display_tag(&self) -> &'static str {
        match self {
            AnomalyRule::LowOutput => "LOW_OUTPUT",
            AnomalyRule::LongRunning => "LONG_RUNNING",
            AnomalyRule::TransientReadError => "TRANSIENT_READ_ERROR",
            AnomalyRule::DuplicateRunner => "DUPLICATE_RUNNER",
            AnomalyRule::OverlappingCycles => "OVERLAPPING_CYCLES",
            AnomalyRule::OverlappingSteps => "OVERLAPPING_STEPS",
            AnomalyRule::MissingStepEnd => "MISSING_STEP_END",
            AnomalyRule::EmptyCycle => "EMPTY_CYCLE",
            AnomalyRule::OrphanCommand => "ORPHAN_COMMAND",
            AnomalyRule::NonzeroExit => "NONZERO_EXIT",
            AnomalyRule::UnexpandedTemplateVar => "UNEXPANDED_TEMPLATE_VAR",
        }
    }

    /// Parses a canonical rule name back into an [`AnomalyRule`].
    pub fn from_canonical(name: &str) -> Option<AnomalyRule> {
        match name {
            "low_output" => Some(AnomalyRule::LowOutput),
            "long_running" => Some(AnomalyRule::LongRunning),
            "transient_read_error" => Some(AnomalyRule::TransientReadError),
            "duplicate_runner" => Some(AnomalyRule::DuplicateRunner),
            "overlapping_cycles" => Some(AnomalyRule::OverlappingCycles),
            "overlapping_steps" => Some(AnomalyRule::OverlappingSteps),
            "missing_step_end" => Some(AnomalyRule::MissingStepEnd),
            "empty_cycle" => Some(AnomalyRule::EmptyCycle),
            "orphan_command" => Some(AnomalyRule::OrphanCommand),
            "nonzero_exit" => Some(AnomalyRule::NonzeroExit),
            "unexpanded_template_var" => Some(AnomalyRule::UnexpandedTemplateVar),
            _ => None,
        }
    }
}

/// Serializable anomaly payload returned by trace analysis.
#[derive(Debug, Serialize, Clone)]
pub struct Anomaly {
    /// Canonical rule name.
    pub rule: String,
    /// Default severity for the detected rule.
    pub severity: Severity,
    /// Recommended escalation level.
    pub escalation: Escalation,
    /// Human-readable anomaly description.
    pub message: String,
    /// Optional timestamp or location associated with the anomaly.
    pub at: Option<String>,
}

impl Anomaly {
    /// Builds an anomaly payload from a rule and message.
    pub fn new(rule: AnomalyRule, message: String, at: Option<String>) -> Self {
        Anomaly {
            severity: rule.default_severity(),
            escalation: rule.escalation(),
            rule: rule.canonical_name().to_string(),
            message,
            at,
        }
    }
}

// ── Tests ───────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    const ALL_RULES: &[AnomalyRule] = &[
        AnomalyRule::LowOutput,
        AnomalyRule::LongRunning,
        AnomalyRule::TransientReadError,
        AnomalyRule::DuplicateRunner,
        AnomalyRule::OverlappingCycles,
        AnomalyRule::OverlappingSteps,
        AnomalyRule::MissingStepEnd,
        AnomalyRule::EmptyCycle,
        AnomalyRule::OrphanCommand,
        AnomalyRule::NonzeroExit,
        AnomalyRule::UnexpandedTemplateVar,
    ];

    #[test]
    fn canonical_name_roundtrip() {
        for rule in ALL_RULES {
            let name = rule.canonical_name();
            let parsed = AnomalyRule::from_canonical(name);
            assert_eq!(
                parsed.as_ref(),
                Some(rule),
                "roundtrip failed for {:?}",
                rule
            );
        }
    }

    #[test]
    fn severity_mapping() {
        assert_eq!(
            AnomalyRule::DuplicateRunner.default_severity(),
            Severity::Error
        );
        assert_eq!(
            AnomalyRule::OverlappingCycles.default_severity(),
            Severity::Error
        );
        assert_eq!(
            AnomalyRule::OverlappingSteps.default_severity(),
            Severity::Error
        );

        assert_eq!(AnomalyRule::LowOutput.default_severity(), Severity::Warning);
        assert_eq!(
            AnomalyRule::NonzeroExit.default_severity(),
            Severity::Warning
        );
        assert_eq!(
            AnomalyRule::MissingStepEnd.default_severity(),
            Severity::Warning
        );
        assert_eq!(
            AnomalyRule::OrphanCommand.default_severity(),
            Severity::Warning
        );
        assert_eq!(
            AnomalyRule::UnexpandedTemplateVar.default_severity(),
            Severity::Warning
        );
        assert_eq!(
            AnomalyRule::TransientReadError.default_severity(),
            Severity::Warning
        );

        assert_eq!(AnomalyRule::LongRunning.default_severity(), Severity::Info);
        assert_eq!(AnomalyRule::EmptyCycle.default_severity(), Severity::Info);
    }

    #[test]
    fn escalation_mapping() {
        assert_eq!(AnomalyRule::LowOutput.escalation(), Escalation::Intervene);
        assert_eq!(
            AnomalyRule::DuplicateRunner.escalation(),
            Escalation::Intervene
        );
        assert_eq!(
            AnomalyRule::OverlappingCycles.escalation(),
            Escalation::Intervene
        );
        assert_eq!(
            AnomalyRule::OverlappingSteps.escalation(),
            Escalation::Intervene
        );

        assert_eq!(AnomalyRule::NonzeroExit.escalation(), Escalation::Attention);
        assert_eq!(
            AnomalyRule::OrphanCommand.escalation(),
            Escalation::Attention
        );
        assert_eq!(
            AnomalyRule::MissingStepEnd.escalation(),
            Escalation::Attention
        );
        assert_eq!(
            AnomalyRule::UnexpandedTemplateVar.escalation(),
            Escalation::Attention
        );
        assert_eq!(
            AnomalyRule::TransientReadError.escalation(),
            Escalation::Attention
        );

        assert_eq!(AnomalyRule::LongRunning.escalation(), Escalation::Notice);
        assert_eq!(AnomalyRule::EmptyCycle.escalation(), Escalation::Notice);
    }

    #[test]
    fn display_tag_non_empty() {
        for rule in ALL_RULES {
            assert!(!rule.display_tag().is_empty(), "empty tag for {:?}", rule);
        }
    }

    #[test]
    fn anomaly_new_sets_defaults() {
        let a = Anomaly::new(
            AnomalyRule::LowOutput,
            "test message".to_string(),
            Some("2025-01-01".to_string()),
        );
        assert_eq!(a.rule, "low_output");
        assert_eq!(a.severity, Severity::Warning);
        assert_eq!(a.escalation, Escalation::Intervene);
        assert_eq!(a.message, "test message");
        assert_eq!(a.at.as_deref(), Some("2025-01-01"));
    }

    #[test]
    fn anomaly_serialization_includes_escalation() {
        let a = Anomaly::new(AnomalyRule::DuplicateRunner, "dup".to_string(), None);
        let json = serde_json::to_value(&a).expect("anomaly should serialize");
        assert_eq!(json["escalation"], "intervene");
        assert_eq!(json["severity"], "error");
        assert_eq!(json["rule"], "duplicate_runner");
    }

    #[test]
    fn from_canonical_returns_none_for_unknown() {
        assert_eq!(AnomalyRule::from_canonical("bogus_rule"), None);
    }
}
