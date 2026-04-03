use anyhow::Error as AnyError;
use std::fmt;

/// High-level category assigned to orchestrator failures.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorCategory {
    /// The caller supplied invalid or incomplete input.
    UserInput,
    /// Configuration content failed validation.
    ConfigValidation,
    /// The requested object or resource was not found.
    NotFound,
    /// The system state does not permit the requested operation.
    InvalidState,
    /// Security policy denied the requested operation.
    SecurityDenied,
    /// An external dependency such as I/O, transport, or database failed.
    ExternalDependency,
    /// An internal invariant was violated.
    InternalInvariant,
}

impl ErrorCategory {
    /// Returns the stable machine-readable label for the category.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::UserInput => "user_input",
            Self::ConfigValidation => "config_validation",
            Self::NotFound => "not_found",
            Self::InvalidState => "invalid_state",
            Self::SecurityDenied => "security_denied",
            Self::ExternalDependency => "external_dependency",
            Self::InternalInvariant => "internal_invariant",
        }
    }
}

/// Canonical error type returned by public orchestrator APIs.
#[derive(Debug)]
pub struct OrchestratorError {
    category: ErrorCategory,
    operation: &'static str,
    subject: Option<String>,
    source: AnyError,
}

impl OrchestratorError {
    /// Builds an error with an explicit category and operation label.
    pub fn new(
        category: ErrorCategory,
        operation: &'static str,
        source: impl Into<AnyError>,
    ) -> Self {
        Self {
            category,
            operation,
            subject: None,
            source: source.into(),
        }
    }

    /// Attaches an optional resource or subject identifier to the error.
    pub fn with_subject(mut self, subject: impl Into<String>) -> Self {
        self.subject = Some(subject.into());
        self
    }

    /// Returns the assigned error category.
    pub fn category(&self) -> ErrorCategory {
        self.category
    }

    /// Returns the operation label associated with the error.
    pub fn operation(&self) -> &'static str {
        self.operation
    }

    /// Returns the optional subject attached to the error.
    pub fn subject(&self) -> Option<&str> {
        self.subject.as_deref()
    }

    /// Returns the formatted source error message.
    pub fn message(&self) -> String {
        self.source.to_string()
    }

    /// Builds a [`ErrorCategory::UserInput`] error.
    pub fn user_input(operation: &'static str, source: impl Into<AnyError>) -> Self {
        Self::new(ErrorCategory::UserInput, operation, source)
    }

    /// Builds a [`ErrorCategory::ConfigValidation`] error.
    pub fn config_validation(operation: &'static str, source: impl Into<AnyError>) -> Self {
        Self::new(ErrorCategory::ConfigValidation, operation, source)
    }

    /// Builds a [`ErrorCategory::NotFound`] error.
    pub fn not_found(operation: &'static str, source: impl Into<AnyError>) -> Self {
        Self::new(ErrorCategory::NotFound, operation, source)
    }

    /// Builds a [`ErrorCategory::InvalidState`] error.
    pub fn invalid_state(operation: &'static str, source: impl Into<AnyError>) -> Self {
        Self::new(ErrorCategory::InvalidState, operation, source)
    }

    /// Builds a [`ErrorCategory::SecurityDenied`] error.
    pub fn security_denied(operation: &'static str, source: impl Into<AnyError>) -> Self {
        Self::new(ErrorCategory::SecurityDenied, operation, source)
    }

    /// Builds a [`ErrorCategory::ExternalDependency`] error.
    pub fn external_dependency(operation: &'static str, source: impl Into<AnyError>) -> Self {
        Self::new(ErrorCategory::ExternalDependency, operation, source)
    }

    /// Builds a [`ErrorCategory::InternalInvariant`] error.
    pub fn internal_invariant(operation: &'static str, source: impl Into<AnyError>) -> Self {
        Self::new(ErrorCategory::InternalInvariant, operation, source)
    }
}

impl fmt::Display for OrchestratorError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.subject {
            Some(subject) => write!(f, "{} [{}]: {}", self.operation, subject, self.source),
            None => write!(f, "{}: {}", self.operation, self.source),
        }
    }
}

impl std::error::Error for OrchestratorError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(self.source.root_cause())
    }
}

/// Standard result type used by the public orchestrator API.
pub type Result<T> = std::result::Result<T, OrchestratorError>;

impl From<anyhow::Error> for OrchestratorError {
    fn from(value: anyhow::Error) -> Self {
        OrchestratorError::internal_invariant("internal", value)
    }
}

impl From<std::io::Error> for OrchestratorError {
    fn from(value: std::io::Error) -> Self {
        OrchestratorError::external_dependency("internal", value)
    }
}

impl From<rusqlite::Error> for OrchestratorError {
    fn from(value: rusqlite::Error) -> Self {
        OrchestratorError::external_dependency("internal", value)
    }
}

impl From<serde_json::Error> for OrchestratorError {
    fn from(value: serde_json::Error) -> Self {
        OrchestratorError::internal_invariant("internal", value)
    }
}

impl From<serde_yaml::Error> for OrchestratorError {
    fn from(value: serde_yaml::Error) -> Self {
        OrchestratorError::internal_invariant("internal", value)
    }
}

fn classify_by_message(operation: &'static str, error: AnyError) -> OrchestratorError {
    let message = error.to_string();
    let lower = message.to_ascii_lowercase();

    if lower.starts_with("[invalid_")
        || lower.contains(" category: validation")
        || lower.contains(" validation ")
        || lower.contains("must define at least one")
        || lower.contains("metadata.project=")
    {
        return OrchestratorError::config_validation(operation, error);
    }

    if lower.contains("not found")
        || lower.contains("does not exist")
        || lower.contains("no such")
        || lower.contains("unknown resource type")
        || lower.contains("unknown list resource type")
        || lower.contains("unknown builtin provider")
        || lower.contains("provider '") && lower.contains("not found")
    {
        return OrchestratorError::not_found(operation, error);
    }

    if lower.starts_with("use --force")
        || lower.contains("no resumable task found")
        || lower.contains("daemon is ")
        || lower.contains("no active encryption key")
        || lower.contains("no incomplete rotation found")
        || lower.contains("cannot begin rotation")
        || lower.contains("task-scoped workflow accepts at most one")
        || lower.contains("no qa/security markdown files found")
    {
        return OrchestratorError::invalid_state(operation, error);
    }

    if lower.contains("client certificate")
        || lower.contains("permission denied")
        || lower.contains("unauthenticated")
        || lower.contains("access denied")
    {
        return OrchestratorError::security_denied(operation, error);
    }

    if lower.contains("task_id or --latest required")
        || lower.contains("invalid ")
        || lower.contains("cannot be empty")
        || lower.contains("cannot include '..'")
        || lower.contains("must be a relative path")
        || lower.contains("label selector")
        || lower.contains("no valid --target-file entries found")
    {
        return OrchestratorError::user_input(operation, error);
    }

    if lower.contains("failed to")
        || lower.contains("sqlite")
        || lower.contains("database")
        || lower.contains("i/o")
        || lower.contains("io error")
        || lower.contains("connection")
        || lower.contains("timeout")
        || lower.contains("transport")
        || lower.contains("git ")
    {
        return OrchestratorError::external_dependency(operation, error);
    }

    OrchestratorError::internal_invariant(operation, error)
}

/// Classifies a task-related error into an [`OrchestratorError`].
pub fn classify_task_error(
    operation: &'static str,
    error: impl Into<AnyError>,
) -> OrchestratorError {
    classify_by_message(operation, error.into())
}

/// Classifies a resource-management error into an [`OrchestratorError`].
pub fn classify_resource_error(
    operation: &'static str,
    error: impl Into<AnyError>,
) -> OrchestratorError {
    classify_by_message(operation, error.into())
}

/// Classifies a store-backend error into an [`OrchestratorError`].
pub fn classify_store_error(
    operation: &'static str,
    error: impl Into<AnyError>,
) -> OrchestratorError {
    classify_by_message(operation, error.into())
}

/// Classifies a system-level error into an [`OrchestratorError`].
pub fn classify_system_error(
    operation: &'static str,
    error: impl Into<AnyError>,
) -> OrchestratorError {
    classify_by_message(operation, error.into())
}

/// Classifies a secret-management error into an [`OrchestratorError`].
pub fn classify_secret_error(
    operation: &'static str,
    error: impl Into<AnyError>,
) -> OrchestratorError {
    classify_by_message(operation, error.into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classify_task_latest_missing_as_invalid_state() {
        let err = classify_task_error("task.start", anyhow::anyhow!("no resumable task found"));
        assert_eq!(err.category(), ErrorCategory::InvalidState);
    }

    #[test]
    fn classify_manifest_policy_error_as_config_validation() {
        let err = classify_system_error(
            "system.manifest_validate",
            anyhow::anyhow!(
                "[INVALID_WORKSPACE] workspace 'default' qa_targets cannot be empty\n  category: validation"
            ),
        );
        assert_eq!(err.category(), ErrorCategory::ConfigValidation);
    }

    #[test]
    fn classify_missing_project_as_not_found() {
        let err = classify_resource_error(
            "resource.get",
            anyhow::anyhow!("project not found: missing-project"),
        );
        assert_eq!(err.category(), ErrorCategory::NotFound);
    }

    #[test]
    fn classify_invalid_target_file_as_user_input() {
        let err = classify_task_error(
            "task.create",
            anyhow::anyhow!("no valid --target-file entries found"),
        );
        assert_eq!(err.category(), ErrorCategory::UserInput);
    }

    #[test]
    fn classify_secret_rotation_without_key_as_invalid_state() {
        let err = classify_secret_error(
            "secret.rotate",
            anyhow::anyhow!(
                "SecretStore write blocked: no active encryption key (all keys revoked or retired)"
            ),
        );
        assert_eq!(err.category(), ErrorCategory::InvalidState);
    }

    // ---- ErrorCategory::as_str() for all variants ----

    #[test]
    fn error_category_as_str_all_variants() {
        assert_eq!(ErrorCategory::UserInput.as_str(), "user_input");
        assert_eq!(
            ErrorCategory::ConfigValidation.as_str(),
            "config_validation"
        );
        assert_eq!(ErrorCategory::NotFound.as_str(), "not_found");
        assert_eq!(ErrorCategory::InvalidState.as_str(), "invalid_state");
        assert_eq!(ErrorCategory::SecurityDenied.as_str(), "security_denied");
        assert_eq!(
            ErrorCategory::ExternalDependency.as_str(),
            "external_dependency"
        );
        assert_eq!(
            ErrorCategory::InternalInvariant.as_str(),
            "internal_invariant"
        );
    }

    // ---- Builder methods and accessors ----

    #[test]
    fn orchestrator_error_new_and_accessors() {
        let err = OrchestratorError::new(
            ErrorCategory::NotFound,
            "resource.get",
            anyhow::anyhow!("widget not found"),
        );
        assert_eq!(err.category(), ErrorCategory::NotFound);
        assert_eq!(err.operation(), "resource.get");
        assert_eq!(err.subject(), None);
        assert_eq!(err.message(), "widget not found");
    }

    #[test]
    fn orchestrator_error_with_subject() {
        let err = OrchestratorError::new(
            ErrorCategory::UserInput,
            "task.create",
            anyhow::anyhow!("bad input"),
        )
        .with_subject("my-task");
        assert_eq!(err.subject(), Some("my-task"));
        assert_eq!(err.category(), ErrorCategory::UserInput);
        assert_eq!(err.operation(), "task.create");
        assert_eq!(err.message(), "bad input");
    }

    // ---- Display impl ----

    #[test]
    fn display_without_subject() {
        let err = OrchestratorError::new(
            ErrorCategory::InternalInvariant,
            "op",
            anyhow::anyhow!("boom"),
        );
        let display = format!("{}", err);
        assert_eq!(display, "op: boom");
    }

    #[test]
    fn display_with_subject() {
        let err = OrchestratorError::new(
            ErrorCategory::NotFound,
            "resource.get",
            anyhow::anyhow!("missing"),
        )
        .with_subject("proj-1");
        let display = format!("{}", err);
        assert_eq!(display, "resource.get [proj-1]: missing");
    }

    // ---- From conversions ----

    #[test]
    fn from_anyhow_error() {
        let anyhow_err = anyhow::anyhow!("anyhow failure");
        let err: OrchestratorError = anyhow_err.into();
        assert_eq!(err.category(), ErrorCategory::InternalInvariant);
        assert_eq!(err.operation(), "internal");
    }

    #[test]
    fn from_io_error() {
        let io_err = std::io::Error::new(std::io::ErrorKind::NotFound, "file gone");
        let err: OrchestratorError = io_err.into();
        assert_eq!(err.category(), ErrorCategory::ExternalDependency);
        assert_eq!(err.operation(), "internal");
    }

    #[test]
    fn from_rusqlite_error() {
        let sqlite_err = rusqlite::Error::SqliteFailure(
            rusqlite::ffi::Error::new(1),
            Some("db locked".to_string()),
        );
        let err: OrchestratorError = sqlite_err.into();
        assert_eq!(err.category(), ErrorCategory::ExternalDependency);
        assert_eq!(err.operation(), "internal");
    }

    #[test]
    fn from_serde_json_error() {
        let json_err = serde_json::from_str::<serde_json::Value>("{{bad").unwrap_err();
        let err: OrchestratorError = json_err.into();
        assert_eq!(err.category(), ErrorCategory::InternalInvariant);
        assert_eq!(err.operation(), "internal");
    }

    #[test]
    fn from_serde_yaml_error() {
        let yaml_err = serde_yaml::from_str::<serde_yaml::Value>(":\n  :\n- -").unwrap_err();
        let err: OrchestratorError = yaml_err.into();
        assert_eq!(err.category(), ErrorCategory::InternalInvariant);
        assert_eq!(err.operation(), "internal");
    }

    // ---- classify_by_message branches ----

    #[test]
    fn classify_permission_denied_as_security() {
        let err = classify_task_error("op", anyhow::anyhow!("Permission denied for resource"));
        assert_eq!(err.category(), ErrorCategory::SecurityDenied);
    }

    #[test]
    fn classify_access_denied_as_security() {
        let err = classify_task_error("op", anyhow::anyhow!("Access denied by policy"));
        assert_eq!(err.category(), ErrorCategory::SecurityDenied);
    }

    #[test]
    fn classify_client_certificate_as_security() {
        let err = classify_task_error("op", anyhow::anyhow!("client certificate expired"));
        assert_eq!(err.category(), ErrorCategory::SecurityDenied);
    }

    #[test]
    fn classify_unauthenticated_as_security() {
        let err = classify_task_error("op", anyhow::anyhow!("unauthenticated request"));
        assert_eq!(err.category(), ErrorCategory::SecurityDenied);
    }

    #[test]
    fn classify_failed_to_as_external_dependency() {
        let err = classify_task_error("op", anyhow::anyhow!("failed to connect to server"));
        assert_eq!(err.category(), ErrorCategory::ExternalDependency);
    }

    #[test]
    fn classify_sqlite_as_external_dependency() {
        let err = classify_task_error("op", anyhow::anyhow!("sqlite error: disk full"));
        assert_eq!(err.category(), ErrorCategory::ExternalDependency);
    }

    #[test]
    fn classify_timeout_as_external_dependency() {
        let err = classify_task_error("op", anyhow::anyhow!("request timeout after 30s"));
        assert_eq!(err.category(), ErrorCategory::ExternalDependency);
    }

    #[test]
    fn classify_database_as_external_dependency() {
        let err = classify_task_error("op", anyhow::anyhow!("database connection lost"));
        assert_eq!(err.category(), ErrorCategory::ExternalDependency);
    }

    #[test]
    fn classify_io_error_message_as_external_dependency() {
        let err = classify_task_error("op", anyhow::anyhow!("i/o error reading file"));
        assert_eq!(err.category(), ErrorCategory::ExternalDependency);
    }

    #[test]
    fn classify_connection_as_external_dependency() {
        let err = classify_task_error("op", anyhow::anyhow!("connection refused"));
        assert_eq!(err.category(), ErrorCategory::ExternalDependency);
    }

    #[test]
    fn classify_transport_as_external_dependency() {
        let err = classify_task_error("op", anyhow::anyhow!("transport layer error"));
        assert_eq!(err.category(), ErrorCategory::ExternalDependency);
    }

    #[test]
    fn classify_git_as_external_dependency() {
        let err = classify_task_error("op", anyhow::anyhow!("git push failed"));
        assert_eq!(err.category(), ErrorCategory::ExternalDependency);
    }

    #[test]
    fn classify_invalid_prefix_as_user_input() {
        let err = classify_task_error("op", anyhow::anyhow!("invalid port number"));
        assert_eq!(err.category(), ErrorCategory::UserInput);
    }

    #[test]
    fn classify_cannot_be_empty_as_user_input() {
        let err = classify_task_error("op", anyhow::anyhow!("name cannot be empty"));
        assert_eq!(err.category(), ErrorCategory::UserInput);
    }

    #[test]
    fn classify_task_id_required_as_user_input() {
        let err = classify_task_error("op", anyhow::anyhow!("task_id or --latest required"));
        assert_eq!(err.category(), ErrorCategory::UserInput);
    }

    #[test]
    fn classify_cannot_include_dotdot_as_user_input() {
        let err = classify_task_error("op", anyhow::anyhow!("path cannot include '..'"));
        assert_eq!(err.category(), ErrorCategory::UserInput);
    }

    #[test]
    fn classify_must_be_relative_path_as_user_input() {
        let err = classify_task_error("op", anyhow::anyhow!("must be a relative path"));
        assert_eq!(err.category(), ErrorCategory::UserInput);
    }

    #[test]
    fn classify_label_selector_as_user_input() {
        let err = classify_task_error("op", anyhow::anyhow!("bad label selector syntax"));
        assert_eq!(err.category(), ErrorCategory::UserInput);
    }

    #[test]
    fn classify_fallback_as_internal_invariant() {
        let err = classify_task_error("op", anyhow::anyhow!("something completely unexpected"));
        assert_eq!(err.category(), ErrorCategory::InternalInvariant);
    }

    #[test]
    fn classify_validation_keyword_as_config_validation() {
        let err = classify_task_error(
            "op",
            anyhow::anyhow!("field has category: validation error"),
        );
        assert_eq!(err.category(), ErrorCategory::ConfigValidation);
    }

    #[test]
    fn classify_must_define_at_least_one_as_config_validation() {
        let err = classify_task_error("op", anyhow::anyhow!("must define at least one target"));
        assert_eq!(err.category(), ErrorCategory::ConfigValidation);
    }

    #[test]
    fn classify_metadata_project_as_config_validation() {
        let err = classify_task_error("op", anyhow::anyhow!("metadata.project=foo is invalid"));
        assert_eq!(err.category(), ErrorCategory::ConfigValidation);
    }

    #[test]
    fn classify_validation_word_as_config_validation() {
        let err = classify_task_error("op", anyhow::anyhow!("schema validation failed"));
        assert_eq!(err.category(), ErrorCategory::ConfigValidation);
    }

    #[test]
    fn classify_does_not_exist_as_not_found() {
        let err = classify_resource_error("op", anyhow::anyhow!("resource does not exist"));
        assert_eq!(err.category(), ErrorCategory::NotFound);
    }

    #[test]
    fn classify_no_such_as_not_found() {
        let err = classify_resource_error("op", anyhow::anyhow!("no such file or directory"));
        assert_eq!(err.category(), ErrorCategory::NotFound);
    }

    #[test]
    fn classify_unknown_resource_type_as_not_found() {
        let err = classify_resource_error("op", anyhow::anyhow!("unknown resource type 'Foo'"));
        assert_eq!(err.category(), ErrorCategory::NotFound);
    }

    #[test]
    fn classify_unknown_list_resource_type_as_not_found() {
        let err = classify_resource_error("op", anyhow::anyhow!("unknown list resource type 'X'"));
        assert_eq!(err.category(), ErrorCategory::NotFound);
    }

    #[test]
    fn classify_unknown_builtin_provider_as_not_found() {
        let err = classify_resource_error("op", anyhow::anyhow!("unknown builtin provider 'Y'"));
        assert_eq!(err.category(), ErrorCategory::NotFound);
    }

    #[test]
    fn classify_use_force_as_invalid_state() {
        let err = classify_task_error("op", anyhow::anyhow!("use --force to override"));
        assert_eq!(err.category(), ErrorCategory::InvalidState);
    }

    #[test]
    fn classify_daemon_is_as_invalid_state() {
        let err = classify_system_error("op", anyhow::anyhow!("daemon is not running"));
        assert_eq!(err.category(), ErrorCategory::InvalidState);
    }

    #[test]
    fn classify_no_incomplete_rotation_as_invalid_state() {
        let err = classify_secret_error("op", anyhow::anyhow!("no incomplete rotation found"));
        assert_eq!(err.category(), ErrorCategory::InvalidState);
    }

    #[test]
    fn classify_cannot_begin_rotation_as_invalid_state() {
        let err = classify_secret_error("op", anyhow::anyhow!("cannot begin rotation now"));
        assert_eq!(err.category(), ErrorCategory::InvalidState);
    }

    #[test]
    fn classify_task_scoped_workflow_limit_as_invalid_state() {
        let err = classify_task_error(
            "op",
            anyhow::anyhow!("task-scoped workflow accepts at most one step"),
        );
        assert_eq!(err.category(), ErrorCategory::InvalidState);
    }

    #[test]
    fn classify_no_qa_markdown_as_invalid_state() {
        let err = classify_system_error(
            "op",
            anyhow::anyhow!("no qa/security markdown files found in directory"),
        );
        assert_eq!(err.category(), ErrorCategory::InvalidState);
    }

    #[test]
    fn classify_io_error_keyword_as_external_dependency() {
        let err = classify_task_error("op", anyhow::anyhow!("io error on socket"));
        assert_eq!(err.category(), ErrorCategory::ExternalDependency);
    }

    // ---- Convenience builder helpers ----

    #[test]
    fn convenience_builders_set_correct_category() {
        let cases: Vec<(OrchestratorError, ErrorCategory)> = vec![
            (
                OrchestratorError::user_input("op", anyhow::anyhow!("e")),
                ErrorCategory::UserInput,
            ),
            (
                OrchestratorError::config_validation("op", anyhow::anyhow!("e")),
                ErrorCategory::ConfigValidation,
            ),
            (
                OrchestratorError::not_found("op", anyhow::anyhow!("e")),
                ErrorCategory::NotFound,
            ),
            (
                OrchestratorError::invalid_state("op", anyhow::anyhow!("e")),
                ErrorCategory::InvalidState,
            ),
            (
                OrchestratorError::security_denied("op", anyhow::anyhow!("e")),
                ErrorCategory::SecurityDenied,
            ),
            (
                OrchestratorError::external_dependency("op", anyhow::anyhow!("e")),
                ErrorCategory::ExternalDependency,
            ),
            (
                OrchestratorError::internal_invariant("op", anyhow::anyhow!("e")),
                ErrorCategory::InternalInvariant,
            ),
        ];
        for (err, expected) in cases {
            assert_eq!(err.category(), expected);
        }
    }

    // ---- std::error::Error impl ----

    #[test]
    fn std_error_source_returns_root_cause() {
        let err = OrchestratorError::new(
            ErrorCategory::InternalInvariant,
            "op",
            anyhow::anyhow!("root cause"),
        );
        let std_err: &dyn std::error::Error = &err;
        assert!(std_err.source().is_some());
    }

    // ---- classify wrappers delegate correctly ----

    #[test]
    fn classify_store_error_delegates() {
        let err = classify_store_error("store.put", anyhow::anyhow!("database locked"));
        assert_eq!(err.category(), ErrorCategory::ExternalDependency);
        assert_eq!(err.operation(), "store.put");
    }

    #[test]
    fn classify_system_error_delegates() {
        let err = classify_system_error("sys.check", anyhow::anyhow!("timeout waiting"));
        assert_eq!(err.category(), ErrorCategory::ExternalDependency);
        assert_eq!(err.operation(), "sys.check");
    }

    #[test]
    fn classify_resource_error_delegates() {
        let err =
            classify_resource_error("res.list", anyhow::anyhow!("unknown list resource type"));
        assert_eq!(err.category(), ErrorCategory::NotFound);
        assert_eq!(err.operation(), "res.list");
    }
}
