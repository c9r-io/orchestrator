//! Agent Collaboration Module
//!
//! Provides structured agent-to-agent communication, message bus,
//! shared context, and DAG-based workflow execution.

#![cfg_attr(not(test), deny(clippy::panic, clippy::unwrap_used, clippy::expect_used))]
#![deny(missing_docs)]

/// Artifact models and shared-state helpers for multi-agent execution.
pub mod artifact;
/// Agent execution context types and template-rendering helpers.
pub mod context;
/// DAG primitives used by collaboration planning flows.
pub mod dag;
/// Message envelope types exchanged between collaborating agents.
pub mod message;
/// Structured agent output payloads and metrics.
pub mod output;

pub use artifact::*;
pub use context::*;
pub use dag::*;
pub use message::*;
pub use output::*;

/// Escape a string for safe embedding inside a bash double-quoted string.
///
/// Inside bash double quotes, the characters `\`, `$`, `` ` ``, `"`, and `!`
/// are special. This function escapes them so that the shell passes the
/// literal content to the target program without interpretation.
pub(crate) fn escape_for_bash_dquote(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + s.len() / 8);
    for ch in s.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '$' => out.push_str("\\$"),
            '`' => out.push_str("\\`"),
            '"' => out.push_str("\\\""),
            '!' => out.push_str("\\!"),
            _ => out.push(ch),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_escape_for_bash_dquote() {
        assert_eq!(escape_for_bash_dquote("`resource.rs`"), "\\`resource.rs\\`");
        assert_eq!(escape_for_bash_dquote("$HOME"), "\\$HOME");
        assert_eq!(escape_for_bash_dquote(r#"say "hello""#), r#"say \"hello\""#);
        assert_eq!(escape_for_bash_dquote(r"path\to"), r"path\\to");
        assert_eq!(escape_for_bash_dquote("wow!"), "wow\\!");
        assert_eq!(escape_for_bash_dquote("hello world"), "hello world");

        let plan = "| `mod.rs` | ~200 | Core types, `pub(super)` |\n| $cost | ~$5 |";
        let escaped = escape_for_bash_dquote(plan);
        assert!(escaped.contains("\\`mod.rs\\`"));
        assert!(escaped.contains("\\`pub(super)\\`"));
        assert!(escaped.contains("\\$cost"));
        assert!(escaped.contains("\\$5"));
        assert!(!escaped.contains(" `m"));
    }
}
