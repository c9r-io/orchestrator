//! Path string expansion for `~` (home directory) and `$VAR`/`${VAR}` (environment variables).
//!
//! Used by execution profile resolution to support user-friendly paths in
//! `readable_paths` / `writable_paths` configuration.

use std::path::PathBuf;

/// Expand `~` and environment variable references in a path string.
///
/// Supported expansions:
/// - Leading `~` or `~/...` → user's home directory (from `$HOME`)
/// - `$NAME` and `${NAME}` → value of environment variable `NAME`
///
/// If an environment variable is unset, the placeholder is left in place
/// (best-effort expansion). Returns the resulting path as a `PathBuf`.
pub(crate) fn expand_path(input: &str) -> PathBuf {
    let after_tilde = expand_tilde(input);
    let after_vars = expand_env_vars(&after_tilde);
    PathBuf::from(after_vars)
}

fn expand_tilde(input: &str) -> String {
    if input == "~" {
        return std::env::var("HOME").unwrap_or_else(|_| input.to_string());
    }
    if let Some(rest) = input.strip_prefix("~/") {
        if let Ok(home) = std::env::var("HOME") {
            return format!("{}/{}", home.trim_end_matches('/'), rest);
        }
    }
    input.to_string()
}

fn expand_env_vars(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let bytes = input.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'$' && i + 1 < bytes.len() {
            // ${NAME}
            if bytes[i + 1] == b'{' {
                if let Some(end) = input[i + 2..].find('}') {
                    let name = &input[i + 2..i + 2 + end];
                    match std::env::var(name) {
                        Ok(val) => out.push_str(&val),
                        Err(_) => out.push_str(&input[i..i + 2 + end + 1]),
                    }
                    i += 2 + end + 1;
                    continue;
                }
            } else if is_var_start(bytes[i + 1]) {
                // $NAME
                let mut end = i + 2;
                while end < bytes.len() && is_var_continue(bytes[end]) {
                    end += 1;
                }
                let name = &input[i + 1..end];
                match std::env::var(name) {
                    Ok(val) => out.push_str(&val),
                    Err(_) => out.push_str(&input[i..end]),
                }
                i = end;
                continue;
            }
        }
        out.push(bytes[i] as char);
        i += 1;
    }
    out
}

fn is_var_start(b: u8) -> bool {
    b.is_ascii_alphabetic() || b == b'_'
}

fn is_var_continue(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::test_env::ENV_LOCK;

    /// Set `key` for the duration of `f` and restore it after.  The caller
    /// must already hold `ENV_LOCK` (typically via `EnvGuard::new`).
    /// Restoration uses an inner RAII guard so that a panicking `f` body
    /// still cleans up before the lock is released.
    fn with_env<F: FnOnce()>(key: &'static str, value: &str, f: F) {
        struct Restore {
            key: &'static str,
            prev: Option<std::ffi::OsString>,
        }
        impl Drop for Restore {
            fn drop(&mut self) {
                // SAFETY: caller holds ENV_LOCK for the lifetime of this
                // guard, so no other test in this crate can race with us.
                unsafe {
                    match &self.prev {
                        Some(v) => std::env::set_var(self.key, v),
                        None => std::env::remove_var(self.key),
                    }
                }
            }
        }
        let prev = std::env::var_os(key);
        // SAFETY: caller holds ENV_LOCK; the inner Restore drop will
        // run even if `f` panics.
        unsafe { std::env::set_var(key, value) };
        let _restore = Restore { key, prev };
        f();
    }

    #[test]
    fn no_expansion_when_no_specials() {
        let _g = ENV_LOCK
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        assert_eq!(expand_path("/abs/path"), PathBuf::from("/abs/path"));
        assert_eq!(expand_path("relative/path"), PathBuf::from("relative/path"));
    }

    #[test]
    fn tilde_only_expands_to_home() {
        let _g = ENV_LOCK
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        with_env("HOME", "/users/test", || {
            assert_eq!(expand_path("~"), PathBuf::from("/users/test"));
        });
    }

    #[test]
    fn tilde_slash_expands_to_home_slash_rest() {
        let _g = ENV_LOCK
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        with_env("HOME", "/users/test", || {
            assert_eq!(
                expand_path("~/.orchestratord/logs"),
                PathBuf::from("/users/test/.orchestratord/logs")
            );
        });
    }

    #[test]
    fn dollar_var_expands() {
        let _g = ENV_LOCK
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        with_env("FR093_TEST", "/var/cache", || {
            assert_eq!(
                expand_path("$FR093_TEST/items"),
                PathBuf::from("/var/cache/items")
            );
        });
    }

    #[test]
    fn dollar_brace_var_expands() {
        let _g = ENV_LOCK
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        with_env("FR093_TEST2", "/srv/data", || {
            assert_eq!(
                expand_path("${FR093_TEST2}/cache"),
                PathBuf::from("/srv/data/cache")
            );
        });
    }

    #[test]
    fn unset_var_left_in_place() {
        let _g = ENV_LOCK
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        unsafe {
            std::env::remove_var("FR093_DEFINITELY_UNSET");
        }
        assert_eq!(
            expand_path("$FR093_DEFINITELY_UNSET/x"),
            PathBuf::from("$FR093_DEFINITELY_UNSET/x")
        );
    }

    #[test]
    fn mixed_tilde_and_env_var() {
        let _g = ENV_LOCK
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        with_env("HOME", "/users/test", || {
            with_env("FR093_SUFFIX", "artifacts", || {
                assert_eq!(
                    expand_path("~/$FR093_SUFFIX/data"),
                    PathBuf::from("/users/test/artifacts/data")
                );
            });
        });
    }

    #[test]
    fn middle_tilde_not_expanded() {
        let _g = ENV_LOCK
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        // Tilde only expands at the start; middle ~ stays literal.
        assert_eq!(expand_path("/foo/~/bar"), PathBuf::from("/foo/~/bar"));
    }
}
