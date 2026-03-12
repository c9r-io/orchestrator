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

impl From<serde_yml::Error> for OrchestratorError {
    fn from(value: serde_yml::Error) -> Self {
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
}
