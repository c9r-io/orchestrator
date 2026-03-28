use orchestrator_config::config::{RunnerConfig, RunnerPolicy};
use anyhow::{Result, anyhow};
use std::fmt;

/// Enforces runner shell-policy allowlists before command execution.
pub fn enforce_runner_policy(runner: &RunnerConfig, command: &str) -> Result<()> {
    if command.trim().is_empty() {
        return Err(anyhow!("runner command cannot be empty"));
    }
    if command.contains('\0') || command.contains('\r') {
        return Err(anyhow!(
            "runner command contains blocked control characters (NUL/CR)"
        ));
    }
    if command.len() > 131_072 {
        return Err(anyhow!("runner command too long (>131072 bytes)"));
    }

    if runner.policy == RunnerPolicy::Allowlist {
        if !runner
            .allowed_shells
            .iter()
            .any(|item| item == &runner.shell)
        {
            return Err(anyhow!(
                "runner.shell '{}' is not in runner.allowed_shells",
                runner.shell
            ));
        }
        if !runner
            .allowed_shell_args
            .iter()
            .any(|item| item == &runner.shell_arg)
        {
            return Err(anyhow!(
                "runner.shell_arg '{}' is not in runner.allowed_shell_args",
                runner.shell_arg
            ));
        }
    }
    Ok(())
}

/// Error returned when a command attempts to kill the daemon process in a
/// self-referential workspace.
#[derive(Debug)]
pub struct DaemonPidGuardBlocked {
    /// Human-readable explanation of why the command was blocked.
    pub reason: String,
    /// The pattern that triggered the block.
    pub matched_pattern: String,
}

impl fmt::Display for DaemonPidGuardBlocked {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "daemon PID guard blocked: {} (pattern: {})",
            self.reason, self.matched_pattern
        )
    }
}

impl std::error::Error for DaemonPidGuardBlocked {}

/// Checks whether a shell command attempts to kill the daemon process.
///
/// This guard is only called when the workspace is self-referential (i.e. the
/// daemon is executing tasks against its own source tree). It catches common
/// patterns that would terminate the daemon process:
///
/// 1. `kill ... $(cat ... daemon.pid)` — subshell reading the PID file
/// 2. `kill ... <literal PID>` — direct PID number targeting the daemon
/// 3. `kill ... $ORCHESTRATOR_DAEMON_PID` / `${ORCHESTRATOR_DAEMON_PID}` — env var reference
/// 4. `pkill ... orchestratord` — process-name targeting
/// 5. `killall ... orchestratord` — process-name targeting
/// 6. `orchestrator daemon stop` — CLI sub-command that sends SIGTERM
pub fn guard_daemon_pid_kill(command: &str, daemon_pid: u32) -> Result<(), DaemonPidGuardBlocked> {
    let pid_str = daemon_pid.to_string();

    // Pattern 6: orchestrator daemon stop (CLI sends SIGTERM via nix::sys::signal::kill)
    if contains_daemon_stop_subcommand(command) {
        return Err(DaemonPidGuardBlocked {
            reason: format!(
                "command uses 'orchestrator daemon stop' which sends SIGTERM to daemon (PID {})",
                daemon_pid
            ),
            matched_pattern: "orchestrator daemon stop".to_string(),
        });
    }

    // Pattern 1: kill ... $(cat ... daemon.pid)
    if command.contains("daemon.pid") && command.contains("kill") {
        return Err(DaemonPidGuardBlocked {
            reason: format!(
                "command references daemon.pid in a kill context (daemon PID {})",
                daemon_pid
            ),
            matched_pattern: "kill + daemon.pid".to_string(),
        });
    }

    // Pattern 3: kill ... $ORCHESTRATOR_DAEMON_PID or ${ORCHESTRATOR_DAEMON_PID}
    if command.contains("kill")
        && (command.contains("$ORCHESTRATOR_DAEMON_PID")
            || command.contains("${ORCHESTRATOR_DAEMON_PID}"))
    {
        return Err(DaemonPidGuardBlocked {
            reason: format!(
                "command uses ORCHESTRATOR_DAEMON_PID env var in a kill context (daemon PID {})",
                daemon_pid
            ),
            matched_pattern: "kill + $ORCHESTRATOR_DAEMON_PID".to_string(),
        });
    }

    // Pattern 4 & 5: pkill/killall orchestratord
    for cmd_prefix in &["pkill", "killall"] {
        if contains_process_kill(command, cmd_prefix, "orchestratord") {
            return Err(DaemonPidGuardBlocked {
                reason: format!(
                    "command uses {} to target orchestratord (daemon PID {})",
                    cmd_prefix, daemon_pid
                ),
                matched_pattern: format!("{} orchestratord", cmd_prefix),
            });
        }
    }

    // Pattern 2: kill ... <literal daemon PID>
    // Check each "kill" invocation for the literal daemon PID number.
    // We look for `kill` followed (possibly with flags) by the daemon PID as a
    // standalone token (word boundary).
    if contains_kill_pid(command, &pid_str) {
        return Err(DaemonPidGuardBlocked {
            reason: format!(
                "command contains kill targeting literal daemon PID {}",
                daemon_pid
            ),
            matched_pattern: format!("kill {}", pid_str),
        });
    }

    Ok(())
}

/// Returns true if the command contains `kill` (possibly with flags) followed
/// by the literal PID as a word-boundary token.
fn contains_kill_pid(command: &str, pid_str: &str) -> bool {
    // Find each occurrence of "kill" that looks like a command (preceded by
    // start-of-string, whitespace, semicolon, pipe, or other shell separators).
    for (idx, _) in command.match_indices("kill") {
        // Ensure "kill" is at a word boundary (not part of "pkill" or "killall")
        if idx > 0 {
            let prev = command.as_bytes()[idx - 1];
            if prev.is_ascii_alphanumeric() || prev == b'_' {
                continue; // part of another word like "pkill"
            }
        }
        // Check that the character after "kill" is not alphanumeric (i.e. not "killall")
        let after_kill = idx + 4;
        if after_kill < command.len() {
            let next = command.as_bytes()[after_kill];
            if next.is_ascii_alphanumeric() || next == b'_' {
                continue;
            }
        }
        // Look for the PID in the remainder of the command after "kill"
        let remainder = &command[after_kill..];
        // Check if the PID appears as a standalone token
        for (pid_idx, _) in remainder.match_indices(pid_str) {
            // Ensure PID is at word boundaries
            if pid_idx > 0 {
                let prev = remainder.as_bytes()[pid_idx - 1];
                if prev.is_ascii_digit() {
                    continue;
                }
            }
            let end = pid_idx + pid_str.len();
            if end < remainder.len() {
                let next = remainder.as_bytes()[end];
                if next.is_ascii_digit() {
                    continue;
                }
            }
            return true;
        }
    }
    false
}

/// Returns true if the command contains an `orchestrator daemon stop` invocation.
///
/// Matches patterns like:
/// - `orchestrator daemon stop`
/// - `./target/release/orchestrator daemon stop`
/// - `orchestrator daemon stop 2>&1`
fn contains_daemon_stop_subcommand(command: &str) -> bool {
    // Split on shell separators to handle compound commands like `echo foo && orchestrator daemon stop`
    for segment in command.split([';', '&', '|']) {
        let tokens: Vec<&str> = segment.split_whitespace().collect();
        // Find a token ending with "orchestrator" (handles path prefixes) followed by "daemon" then "stop"
        for window in tokens.windows(3) {
            if window[0].ends_with("orchestrator") && window[1] == "daemon" && window[2] == "stop" {
                return true;
            }
        }
    }
    false
}

/// Returns true if the command contains `{cmd_prefix} ... {target}` as a
/// process-name kill operation.
fn contains_process_kill(command: &str, cmd_prefix: &str, target: &str) -> bool {
    for (idx, _) in command.match_indices(cmd_prefix) {
        // Ensure it's at a word boundary
        if idx > 0 {
            let prev = command.as_bytes()[idx - 1];
            if prev.is_ascii_alphanumeric() || prev == b'_' {
                continue;
            }
        }
        let remainder = &command[idx + cmd_prefix.len()..];
        if remainder.contains(target) {
            return true;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blocks_kill_cat_daemon_pid() {
        let result = guard_daemon_pid_kill("kill $(cat data/daemon.pid)", 12345);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.matched_pattern.contains("daemon.pid"));
    }

    #[test]
    fn blocks_kill_literal_daemon_pid() {
        let result = guard_daemon_pid_kill("kill -9 12345", 12345);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.matched_pattern.contains("12345"));
    }

    #[test]
    fn allows_kill_different_pid() {
        let result = guard_daemon_pid_kill("kill -9 99999", 12345);
        assert!(result.is_ok());
    }

    #[test]
    fn blocks_kill_env_var() {
        let result = guard_daemon_pid_kill("kill $ORCHESTRATOR_DAEMON_PID", 12345);
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .matched_pattern
                .contains("ORCHESTRATOR_DAEMON_PID")
        );
    }

    #[test]
    fn blocks_kill_env_var_braced() {
        let result = guard_daemon_pid_kill("kill ${ORCHESTRATOR_DAEMON_PID}", 12345);
        assert!(result.is_err());
    }

    #[test]
    fn blocks_pkill_orchestratord() {
        let result = guard_daemon_pid_kill("pkill orchestratord", 12345);
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .matched_pattern
                .contains("pkill orchestratord")
        );
    }

    #[test]
    fn blocks_killall_orchestratord() {
        let result = guard_daemon_pid_kill("killall orchestratord", 12345);
        assert!(result.is_err());
    }

    #[test]
    fn allows_normal_command() {
        let result = guard_daemon_pid_kill("echo hello world", 12345);
        assert!(result.is_ok());
    }

    #[test]
    fn allows_kill_word_in_echo() {
        // "kill" in a string context should not trigger if PID doesn't match
        let result = guard_daemon_pid_kill("echo 'kill the process'", 12345);
        assert!(result.is_ok());
    }

    #[test]
    fn blocks_kill_pid_in_compound_command() {
        let result = guard_daemon_pid_kill(
            "echo start && kill $(cat data/daemon.pid) && echo done",
            12345,
        );
        assert!(result.is_err());
    }

    #[test]
    fn allows_kill_pid_not_matching_daemon() {
        let result = guard_daemon_pid_kill("kill 54321", 12345);
        assert!(result.is_ok());
    }

    #[test]
    fn blocks_kill_signal_then_pid() {
        let result = guard_daemon_pid_kill("kill -TERM 12345", 12345);
        assert!(result.is_err());
    }

    #[test]
    fn does_not_false_positive_on_pid_substring() {
        // PID 123 should not match inside 12345
        let result = guard_daemon_pid_kill("kill 12345", 123);
        assert!(result.is_ok());
    }

    #[test]
    fn does_not_false_positive_on_pid_prefix() {
        // PID 12345 should not match 123456
        let result = guard_daemon_pid_kill("kill 123456", 12345);
        assert!(result.is_ok());
    }

    #[test]
    fn blocks_orchestrator_daemon_stop() {
        let result = guard_daemon_pid_kill("orchestrator daemon stop", 12345);
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .matched_pattern
                .contains("orchestrator daemon stop")
        );
    }

    #[test]
    fn blocks_orchestrator_daemon_stop_with_path() {
        let result = guard_daemon_pid_kill("./target/release/orchestrator daemon stop", 12345);
        assert!(result.is_err());
    }

    #[test]
    fn blocks_orchestrator_daemon_stop_with_redirect() {
        let result = guard_daemon_pid_kill("orchestrator daemon stop 2>&1", 12345);
        assert!(result.is_err());
    }

    #[test]
    fn blocks_orchestrator_daemon_stop_in_compound() {
        let result =
            guard_daemon_pid_kill("echo start && orchestrator daemon stop && echo done", 12345);
        assert!(result.is_err());
    }

    #[test]
    fn allows_orchestrator_daemon_status() {
        let result = guard_daemon_pid_kill("orchestrator daemon status", 12345);
        assert!(result.is_ok());
    }

    #[test]
    fn allows_orchestrator_task_stop() {
        let result = guard_daemon_pid_kill("orchestrator task stop abc123", 12345);
        assert!(result.is_ok());
    }
}
