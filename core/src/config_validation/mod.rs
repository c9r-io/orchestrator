//! Configuration validation module.
//!
//! Provides comprehensive validation for YAML configuration files with:
//! - YAML syntax pre-checking
//! - Schema validation (required fields, types, references)
//! - Path validation (existence, security)
//! - Error/warning aggregation with detailed reporting

pub mod path_resolver;
pub mod precheck;
pub mod schema;
pub mod validator;

use serde::{Deserialize, Serialize};
use std::fmt;

/// Validation result with error/warning aggregation
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ValidationResult {
    /// Errors that prevent loading
    #[serde(default)]
    pub errors: Vec<ValidationError>,
    /// Warnings for suggested fixes
    #[serde(default)]
    pub warnings: Vec<ValidationWarning>,
    /// Whether validation passed (no errors)
    #[serde(skip)]
    pub is_valid: bool,
}

impl ValidationResult {
    pub fn new() -> Self {
        Self {
            errors: Vec::new(),
            warnings: Vec::new(),
            is_valid: true,
        }
    }

    pub fn add_error(&mut self, error: ValidationError) {
        self.is_valid = false;
        self.errors.push(error);
    }

    pub fn add_warning(&mut self, warning: ValidationWarning) {
        self.warnings.push(warning);
    }

    /// Merge another result into this one
    pub fn merge(&mut self, other: ValidationResult) {
        for err in other.errors {
            self.add_error(err);
        }
        for warn in other.warnings {
            self.add_warning(warn);
        }
    }

    /// Get human-readable report
    pub fn report(&self) -> String {
        let mut output = String::new();

        if !self.errors.is_empty() {
            output.push_str("Errors:\n");
            for (i, err) in self.errors.iter().enumerate() {
                output.push_str(&format!("  {}. {}\n", i + 1, err));
            }
        }

        if !self.warnings.is_empty() {
            if !output.is_empty() {
                output.push('\n');
            }
            output.push_str("Warnings:\n");
            for (i, warn) in self.warnings.iter().enumerate() {
                output.push_str(&format!("  {}. {}\n", i + 1, warn));
            }
        }

        if self.is_valid && self.warnings.is_empty() {
            output.push_str("Validation passed");
        } else if self.is_valid {
            output.push_str("Validation passed with warnings");
        }

        output
    }

    /// Get count of total issues
    pub fn issue_count(&self) -> usize {
        self.errors.len() + self.warnings.len()
    }
}

/// Validation error that prevents loading
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidationError {
    pub code: ErrorCode,
    pub message: String,
    pub field: Option<String>,
    pub context: Option<String>,
}

impl fmt::Display for ValidationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(field) = &self.field {
            write!(f, "[{}] {}", field, self.message)?;
        } else {
            write!(f, "{}", self.message)?;
        }
        if let Some(ctx) = &self.context {
            write!(f, " ({})", ctx)?;
        }
        Ok(())
    }
}

impl std::error::Error for ValidationError {}

/// Error codes for categorization
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum ErrorCode {
    // YAML layer
    YamlSyntaxError,
    YamlStructureError,

    // Schema layer
    MissingRequiredField,
    InvalidFieldType,
    EmptyCollection,
    InvalidReference,

    // Path layer
    PathNotFound,
    PathOutsideWorkspace,
    PathNotDirectory,

    // Semantic layer
    DuplicateEntry,
    CyclicDependency,
    InvalidConfiguration,
}

/// Validation warning for suggested fixes
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidationWarning {
    pub code: WarningCode,
    pub message: String,
    pub field: Option<String>,
    pub suggestion: Option<String>,
}

impl fmt::Display for ValidationWarning {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(field) = &self.field {
            write!(f, "[{}] {}", field, self.message)?;
        } else {
            write!(f, "{}", self.message)?;
        }
        if let Some(suggestion) = &self.suggestion {
            write!(f, " - Suggestion: {}", suggestion)?;
        }
        Ok(())
    }
}

/// Warning codes for categorization
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum WarningCode {
    DeprecatedField,
    MissingRecommendedField,
    PathNotExists,
    EmptyConfiguration,
    DefaultValueUsed,
    UnusedField,
}

/// Validation levels
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ValidationLevel {
    /// Only YAML syntax check
    SyntaxOnly,
    /// Syntax + Schema validation
    Schema,
    /// Full validation (includes paths)
    Full,
}

impl Default for ValidationLevel {
    fn default() -> Self {
        Self::Full
    }
}

/// Path validation options
#[derive(Debug, Clone, Default)]
pub struct PathValidationOptions {
    /// Treat missing paths as errors (default: false = warning)
    pub missing_path_is_error: bool,
    /// Check path escape attempts
    pub check_path_escape: bool,
}

/// API response for validation
#[derive(Debug, Serialize, Deserialize)]
pub struct ValidationReport {
    pub valid: bool,
    pub normalized_yaml: String,
    pub errors: Vec<ValidationError>,
    pub warnings: Vec<ValidationWarning>,
    pub summary: String,
}

impl ValidationReport {
    pub fn from_result(result: &ValidationResult, normalized: String) -> Self {
        let is_valid = result.is_valid;
        let errors = result.errors.clone();
        let warnings = result.warnings.clone();
        let summary = result.report();
        Self {
            valid: is_valid,
            normalized_yaml: normalized,
            errors,
            warnings,
            summary,
        }
    }

    pub fn error_only(message: String) -> Self {
        Self {
            valid: false,
            normalized_yaml: String::new(),
            errors: vec![ValidationError {
                code: ErrorCode::InvalidConfiguration,
                message,
                field: None,
                context: None,
            }],
            warnings: vec![],
            summary: "Validation failed".to_string(),
        }
    }
}
