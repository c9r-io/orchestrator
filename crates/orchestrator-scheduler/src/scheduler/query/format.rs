//! Display formatting utilities for task status and metrics.

/// Colorize a task status string for terminal display.
pub fn colorize_status(status: &str) -> String {
    match status {
        "completed" => format!("\x1b[32m{}\x1b[0m", status),
        "failed" => format!("\x1b[31m{}\x1b[0m", status),
        "running" => format!("\x1b[33m{}\x1b[0m", status),
        "paused" => format!("\x1b[90m{}\x1b[0m", status),
        _ => status.to_string(),
    }
}

/// Format a duration in milliseconds to a human-readable string.
pub fn format_duration(ms: u64) -> String {
    if ms < 1000 {
        format!("{}ms", ms)
    } else if ms < 60_000 {
        format!("{:.1}s", ms as f64 / 1000.0)
    } else {
        let mins = ms / 60_000;
        let secs = (ms % 60_000) / 1000;
        format!("{}m {}s", mins, secs)
    }
}

/// Format a byte count to a human-readable string.
pub fn format_bytes(bytes: u64) -> String {
    if bytes < 1024 {
        format!("{}B", bytes)
    } else if bytes < 1024 * 1024 {
        format!("{:.1}KB", bytes as f64 / 1024.0)
    } else {
        format!("{:.1}MB", bytes as f64 / (1024.0 * 1024.0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_duration_milliseconds() {
        assert_eq!(format_duration(0), "0ms");
        assert_eq!(format_duration(500), "500ms");
        assert_eq!(format_duration(999), "999ms");
    }

    #[test]
    fn format_duration_seconds() {
        assert_eq!(format_duration(1000), "1.0s");
        assert_eq!(format_duration(1500), "1.5s");
        assert_eq!(format_duration(59_999), "60.0s");
    }

    #[test]
    fn format_duration_minutes() {
        assert_eq!(format_duration(60_000), "1m 0s");
        assert_eq!(format_duration(90_000), "1m 30s");
        assert_eq!(format_duration(3_661_000), "61m 1s");
    }

    #[test]
    fn format_bytes_bytes() {
        assert_eq!(format_bytes(0), "0B");
        assert_eq!(format_bytes(512), "512B");
        assert_eq!(format_bytes(1023), "1023B");
    }

    #[test]
    fn format_bytes_kilobytes() {
        assert_eq!(format_bytes(1024), "1.0KB");
        assert_eq!(format_bytes(1536), "1.5KB");
    }

    #[test]
    fn format_bytes_megabytes() {
        assert_eq!(format_bytes(1024 * 1024), "1.0MB");
        assert_eq!(format_bytes(1024 * 1024 * 5), "5.0MB");
    }

    #[test]
    fn colorize_status_completed() {
        let result = colorize_status("completed");
        assert!(result.contains("completed"));
        assert!(result.contains("\x1b[32m")); // green
    }

    #[test]
    fn colorize_status_failed() {
        let result = colorize_status("failed");
        assert!(result.contains("failed"));
        assert!(result.contains("\x1b[31m")); // red
    }

    #[test]
    fn colorize_status_running() {
        let result = colorize_status("running");
        assert!(result.contains("\x1b[33m")); // yellow
    }

    #[test]
    fn colorize_status_paused() {
        let result = colorize_status("paused");
        assert!(result.contains("\x1b[90m")); // gray
    }

    #[test]
    fn colorize_status_unknown_passes_through() {
        assert_eq!(colorize_status("pending"), "pending");
        assert_eq!(colorize_status("other"), "other");
    }
}
