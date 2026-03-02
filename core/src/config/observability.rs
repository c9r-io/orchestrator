use serde::{Deserialize, Serialize};
use tracing::Level;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum LogLevel {
    Error,
    Warn,
    #[default]
    Info,
    Debug,
    Trace,
}

impl LogLevel {
    pub fn as_tracing_level(self) -> Level {
        match self {
            Self::Error => Level::ERROR,
            Self::Warn => Level::WARN,
            Self::Info => Level::INFO,
            Self::Debug => Level::DEBUG,
            Self::Trace => Level::TRACE,
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "error" => Some(Self::Error),
            "warn" | "warning" => Some(Self::Warn),
            "info" => Some(Self::Info),
            "debug" => Some(Self::Debug),
            "trace" => Some(Self::Trace),
            _ => None,
        }
    }

    pub fn max(self, other: Self) -> Self {
        use LogLevel::*;
        match (self, other) {
            (Trace, _) | (_, Trace) => Trace,
            (Debug, _) | (_, Debug) => Debug,
            (Info, _) | (_, Info) => Info,
            (Warn, _) | (_, Warn) => Warn,
            (Error, Error) => Error,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum LoggingFormat {
    #[default]
    Pretty,
    Json,
}

impl LoggingFormat {
    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "pretty" | "compact" | "text" => Some(Self::Pretty),
            "json" => Some(Self::Json),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ConsoleLoggingConfig {
    #[serde(default = "default_enabled")]
    pub enabled: bool,
    #[serde(default)]
    pub format: LoggingFormat,
    #[serde(default = "default_enabled")]
    pub ansi: bool,
}

impl Default for ConsoleLoggingConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            format: LoggingFormat::Pretty,
            ansi: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FileLoggingConfig {
    #[serde(default = "default_enabled")]
    pub enabled: bool,
    #[serde(default = "default_file_format")]
    pub format: LoggingFormat,
    #[serde(default = "default_log_directory")]
    pub directory: String,
}

impl Default for FileLoggingConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            format: LoggingFormat::Json,
            directory: default_log_directory(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LoggingConfig {
    #[serde(default)]
    pub level: LogLevel,
    #[serde(default)]
    pub console: ConsoleLoggingConfig,
    #[serde(default)]
    pub file: FileLoggingConfig,
    #[serde(default = "default_enabled")]
    pub event_bridge: bool,
}

impl Default for LoggingConfig {
    fn default() -> Self {
        Self {
            level: LogLevel::Info,
            console: ConsoleLoggingConfig::default(),
            file: FileLoggingConfig::default(),
            event_bridge: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct ObservabilityConfig {
    #[serde(default)]
    pub logging: LoggingConfig,
}

fn default_enabled() -> bool {
    true
}

fn default_file_format() -> LoggingFormat {
    LoggingFormat::Json
}

fn default_log_directory() -> String {
    "data/logs/system".to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn observability_defaults_are_safe() {
        let cfg = ObservabilityConfig::default();
        assert_eq!(cfg.logging.level, LogLevel::Info);
        assert!(cfg.logging.console.enabled);
        assert_eq!(cfg.logging.console.format, LoggingFormat::Pretty);
        assert!(cfg.logging.file.enabled);
        assert_eq!(cfg.logging.file.format, LoggingFormat::Json);
        assert_eq!(cfg.logging.file.directory, "data/logs/system");
        assert!(cfg.logging.event_bridge);
    }

    #[test]
    fn observability_serde_defaults_missing_fields() {
        let cfg: ObservabilityConfig = serde_json::from_str("{}").expect("deserialize defaults");
        assert_eq!(cfg, ObservabilityConfig::default());
    }

    #[test]
    fn level_parse_accepts_common_variants() {
        assert_eq!(LogLevel::parse("warning"), Some(LogLevel::Warn));
        assert_eq!(LogLevel::parse("TRACE"), Some(LogLevel::Trace));
        assert_eq!(LogLevel::parse("bogus"), None);
    }

    #[test]
    fn format_parse_accepts_common_variants() {
        assert_eq!(LoggingFormat::parse("text"), Some(LoggingFormat::Pretty));
        assert_eq!(LoggingFormat::parse("json"), Some(LoggingFormat::Json));
        assert_eq!(LoggingFormat::parse("xml"), None);
    }
}
