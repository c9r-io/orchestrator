//! The file-size ceiling FR-157 requirement 2 sets, asserted against the directory
//! rather than against a list of module names.
//!
//! A hand-written list of the modules to check guards exactly the modules its author
//! remembered, and the next one lands outside it silently (`fr-governance` §4.4 shape 2).
//! This walks the tree instead, so a new submodule is covered the moment it exists.

use std::path::{Path, PathBuf};

/// Aligned with `server/`'s other modules: the largest non-source module is
/// `session.rs` at 1064 production lines and the median is 335.
const MAX_PRODUCTION_LINES: usize = 1000;

fn source_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("src/server")
}

/// Every production `.rs` file that makes up the `source_connection` module,
/// discovered recursively. Test sources under a `tests/` directory are not
/// production and are excluded — the same rule `coverage-governance.mjs` applies.
fn production_files() -> Vec<PathBuf> {
    let root = source_root();
    let mut found = Vec::new();
    let entry = root.join("source_connection.rs");
    if entry.is_file() {
        found.push(entry);
    }
    collect(&root.join("source_connection"), &mut found);
    found.sort();
    found
}

fn collect(directory: &Path, found: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(directory) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if path.file_name().is_some_and(|name| name == "tests") {
                continue;
            }
            collect(&path, found);
        } else if path.extension().is_some_and(|extension| extension == "rs") {
            found.push(path);
        }
    }
}

/// Production lines, counted the way FR-157 counts them: everything before the first
/// inline `#[cfg(test)]`.
fn production_lines(path: &Path) -> usize {
    let contents = std::fs::read_to_string(path).expect("read module source");
    contents
        .lines()
        .position(|line| line.trim_start() == "#[cfg(test)]")
        .unwrap_or_else(|| contents.lines().count())
}

#[test]
fn every_source_connection_module_stays_under_the_size_ceiling() {
    let files = production_files();

    // Without this, a scan that found nothing — a renamed directory, a moved module —
    // would report success having examined no input at all.
    assert!(
        !files.is_empty(),
        "no source_connection module was found under {}; the scan, not the code, is broken",
        source_root().display()
    );

    let oversized = files
        .iter()
        .map(|path| (path, production_lines(path)))
        .filter(|(_, lines)| *lines > MAX_PRODUCTION_LINES)
        .map(|(path, lines)| {
            format!(
                "{} has {lines} production lines (ceiling {MAX_PRODUCTION_LINES})",
                path.file_name().unwrap_or_default().to_string_lossy()
            )
        })
        .collect::<Vec<_>>();

    assert!(
        oversized.is_empty(),
        "source_connection modules exceed the size ceiling:\n  {}",
        oversized.join("\n  ")
    );
}

/// The ceiling is only meaningful if the module was actually decomposed; a single
/// 999-line file would satisfy the check above while changing nothing.
#[test]
fn the_module_is_decomposed_rather_than_merely_trimmed() {
    let files = production_files();
    assert!(
        files.len() >= 4,
        "expected the source_connection module to be split across submodules, found {:?}",
        files
            .iter()
            .map(|path| path.file_name().unwrap_or_default().to_string_lossy())
            .collect::<Vec<_>>()
    );
}
