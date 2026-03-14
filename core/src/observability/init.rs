use crate::config::{LogLevel, LoggingConfig, LoggingFormat, OrchestratorConfig};
use crate::config_ext::OrchestratorConfigExt as _;
use anyhow::{Context, Result};
use std::io::IsTerminal;
use std::path::{Path, PathBuf};
use tracing_subscriber::filter::LevelFilter;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::Layer;

#[derive(Debug, Clone, Copy, Default)]
/// CLI flags that override logging configuration resolved from runtime policy.
pub struct CliLoggingOverrides {
    /// Forces at least debug-level logging when set.
    pub verbose: bool,
    /// Explicit log level override.
    pub level: Option<LogLevel>,
    /// Explicit console log format override.
    pub format: Option<LoggingFormat>,
}

#[derive(Debug, Clone)]
/// Fully resolved logging configuration used to initialize tracing subscribers.
pub struct ResolvedLoggingConfig {
    /// Effective minimum log level.
    pub level: LogLevel,
    /// Whether console logging is enabled.
    pub console_enabled: bool,
    /// Console log output format.
    pub console_format: LoggingFormat,
    /// Whether ANSI styling is enabled for console output.
    pub console_ansi: bool,
    /// Whether file logging is enabled.
    pub file_enabled: bool,
    /// File log output format.
    pub file_format: LoggingFormat,
    /// Directory where rolling log files are written.
    pub file_dir: PathBuf,
}

#[derive(Debug, Default)]
/// Holds background logging guards that must live for the lifetime of observability.
pub struct ObservabilityGuard {
    _file_guard: Option<tracing_appender::non_blocking::WorkerGuard>,
}

/// Initializes tracing subscribers using config and CLI/environment overrides.
pub fn init_observability(
    app_root: &Path,
    config: Option<&OrchestratorConfig>,
    overrides: CliLoggingOverrides,
) -> Result<ObservabilityGuard> {
    let resolved = resolve_logging_config(app_root, config, overrides);
    let level_filter = LevelFilter::from_level(resolved.level.as_tracing_level());
    let mut file_guard = None;

    if resolved.file_enabled {
        std::fs::create_dir_all(&resolved.file_dir).with_context(|| {
            format!(
                "failed to create system log directory {}",
                resolved.file_dir.display()
            )
        })?;
        let appender = tracing_appender::rolling::daily(&resolved.file_dir, "orchestrator.log");
        let (non_blocking, guard) = tracing_appender::non_blocking(appender);
        file_guard = Some(guard);

        match (
            resolved.console_enabled,
            resolved.console_format,
            resolved.file_format,
        ) {
            (true, LoggingFormat::Pretty, LoggingFormat::Pretty) => tracing_subscriber::registry()
                .with(
                    tracing_subscriber::fmt::layer()
                        .compact()
                        .with_writer(std::io::stderr)
                        .with_ansi(resolved.console_ansi)
                        .with_filter(level_filter),
                )
                .with(
                    tracing_subscriber::fmt::layer()
                        .compact()
                        .with_ansi(false)
                        .with_writer(non_blocking)
                        .with_filter(level_filter),
                )
                .try_init()
                .context("failed to initialize structured logging")?,
            (true, LoggingFormat::Pretty, LoggingFormat::Json) => tracing_subscriber::registry()
                .with(
                    tracing_subscriber::fmt::layer()
                        .compact()
                        .with_writer(std::io::stderr)
                        .with_ansi(resolved.console_ansi)
                        .with_filter(level_filter),
                )
                .with(
                    tracing_subscriber::fmt::layer()
                        .json()
                        .with_writer(non_blocking)
                        .with_filter(level_filter),
                )
                .try_init()
                .context("failed to initialize structured logging")?,
            (true, LoggingFormat::Json, LoggingFormat::Pretty) => tracing_subscriber::registry()
                .with(
                    tracing_subscriber::fmt::layer()
                        .json()
                        .with_writer(std::io::stderr)
                        .with_filter(level_filter),
                )
                .with(
                    tracing_subscriber::fmt::layer()
                        .compact()
                        .with_ansi(false)
                        .with_writer(non_blocking)
                        .with_filter(level_filter),
                )
                .try_init()
                .context("failed to initialize structured logging")?,
            (true, LoggingFormat::Json, LoggingFormat::Json) => tracing_subscriber::registry()
                .with(
                    tracing_subscriber::fmt::layer()
                        .json()
                        .with_writer(std::io::stderr)
                        .with_filter(level_filter),
                )
                .with(
                    tracing_subscriber::fmt::layer()
                        .json()
                        .with_writer(non_blocking)
                        .with_filter(level_filter),
                )
                .try_init()
                .context("failed to initialize structured logging")?,
            (false, _, LoggingFormat::Pretty) => tracing_subscriber::registry()
                .with(
                    tracing_subscriber::fmt::layer()
                        .compact()
                        .with_ansi(false)
                        .with_writer(non_blocking)
                        .with_filter(level_filter),
                )
                .try_init()
                .context("failed to initialize structured logging")?,
            (false, _, LoggingFormat::Json) => tracing_subscriber::registry()
                .with(
                    tracing_subscriber::fmt::layer()
                        .json()
                        .with_writer(non_blocking)
                        .with_filter(level_filter),
                )
                .try_init()
                .context("failed to initialize structured logging")?,
        }
    } else if resolved.console_enabled {
        match resolved.console_format {
            LoggingFormat::Pretty => tracing_subscriber::registry()
                .with(
                    tracing_subscriber::fmt::layer()
                        .compact()
                        .with_writer(std::io::stderr)
                        .with_ansi(resolved.console_ansi)
                        .with_filter(level_filter),
                )
                .try_init()
                .context("failed to initialize structured logging")?,
            LoggingFormat::Json => tracing_subscriber::registry()
                .with(
                    tracing_subscriber::fmt::layer()
                        .json()
                        .with_writer(std::io::stderr)
                        .with_filter(level_filter),
                )
                .try_init()
                .context("failed to initialize structured logging")?,
        }
    }

    Ok(ObservabilityGuard {
        _file_guard: file_guard,
    })
}

/// Resolves the effective logging configuration before subscriber initialization.
pub fn resolve_logging_config(
    app_root: &Path,
    config: Option<&OrchestratorConfig>,
    overrides: CliLoggingOverrides,
) -> ResolvedLoggingConfig {
    let logging = config
        .map(|cfg| cfg.runtime_policy().observability.logging.clone())
        .unwrap_or_default();

    let mut level = logging.level;
    if overrides.verbose {
        level = level.max(LogLevel::Debug);
    }
    if let Some(cli_level) = overrides.level {
        level = cli_level;
    }
    if let Some(env_level) = read_env_level() {
        level = env_level;
    }

    let mut console_format = logging.console.format;
    if let Some(cli_format) = overrides.format {
        console_format = cli_format;
    }
    if let Some(env_format) = read_env_format() {
        console_format = env_format;
    }

    let file_dir = resolve_log_dir(app_root, &logging);

    ResolvedLoggingConfig {
        level,
        console_enabled: logging.console.enabled,
        console_format,
        console_ansi: logging.console.ansi && std::io::stderr().is_terminal(),
        file_enabled: logging.file.enabled,
        file_format: logging.file.format,
        file_dir,
    }
}

fn read_env_level() -> Option<LogLevel> {
    std::env::var("ORCHESTRATOR_LOG")
        .ok()
        .or_else(|| std::env::var("RUST_LOG").ok())
        .and_then(|value| value.split(',').next().and_then(LogLevel::parse))
}

fn read_env_format() -> Option<LoggingFormat> {
    std::env::var("ORCHESTRATOR_LOG_FORMAT")
        .ok()
        .as_deref()
        .and_then(LoggingFormat::parse)
}

fn resolve_log_dir(app_root: &Path, logging: &LoggingConfig) -> PathBuf {
    let configured = Path::new(&logging.file.directory);
    if configured.is_absolute() {
        configured.to_path_buf()
    } else {
        app_root.join(configured)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::LoggingFormat;

    fn sample_config() -> OrchestratorConfig {
        OrchestratorConfig::default()
    }

    #[test]
    fn verbose_raises_default_level_to_debug() {
        let cfg = sample_config();
        let resolved = resolve_logging_config(
            Path::new("/tmp/app"),
            Some(&cfg),
            CliLoggingOverrides {
                verbose: true,
                ..CliLoggingOverrides::default()
            },
        );
        assert_eq!(resolved.level, LogLevel::Debug);
    }

    #[test]
    fn cli_level_overrides_config() {
        let cfg = sample_config();
        let resolved = resolve_logging_config(
            Path::new("/tmp/app"),
            Some(&cfg),
            CliLoggingOverrides {
                level: Some(LogLevel::Trace),
                ..CliLoggingOverrides::default()
            },
        );
        assert_eq!(resolved.level, LogLevel::Trace);
    }

    #[test]
    fn cli_format_overrides_console_format() {
        let cfg = sample_config();
        let resolved = resolve_logging_config(
            Path::new("/tmp/app"),
            Some(&cfg),
            CliLoggingOverrides {
                format: Some(LoggingFormat::Json),
                ..CliLoggingOverrides::default()
            },
        );
        assert_eq!(resolved.console_format, LoggingFormat::Json);
    }

    #[test]
    fn relative_file_path_is_resolved_from_app_root() {
        let cfg = sample_config();
        let resolved = resolve_logging_config(
            Path::new("/tmp/app"),
            Some(&cfg),
            CliLoggingOverrides::default(),
        );
        assert_eq!(resolved.file_dir, Path::new("/tmp/app/data/logs/system"));
    }
}
