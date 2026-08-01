//! FR-154: the single serialization chokepoint for CLI output.
//!
//! Every json/yaml byte the CLI prints is produced by [`encode`]. Commands
//! build exactly one `serde_json::Value` per payload and hand it here, so the
//! two machine-readable encodings cannot observe different data. Table
//! rendering stays at the call sites (a table may omit columns), which is why
//! [`Encoding`] deliberately has no `Table` variant.

use anyhow::{Context, Result};
use serde_json::Value;

/// Machine-readable encodings. Callers must resolve `OutputFormat::Table`
/// before reaching this module — see [`crate::OutputFormat::encoding`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum Encoding {
    /// Pretty-printed JSON, newline-terminated. The default for `-o json`.
    JsonPretty,
    /// Single-line JSON, newline-terminated. For streaming deltas (NDJSON).
    JsonCompact,
    /// YAML document. `serde_yaml` output is already newline-terminated.
    Yaml,
}

impl crate::OutputFormat {
    /// `None` means the caller renders a table; `Some` is fed to [`emit`].
    pub(crate) fn encoding(self) -> Option<Encoding> {
        match self {
            crate::OutputFormat::Table => None,
            crate::OutputFormat::Json => Some(Encoding::JsonPretty),
            crate::OutputFormat::Yaml => Some(Encoding::Yaml),
        }
    }
}

/// Serialize `value`. This is the only place in the crate allowed to call
/// `serde_json::to_string*` / `serde_yaml::to_string` for output; failures
/// propagate instead of degrading to an empty string.
pub(crate) fn encode(value: &Value, encoding: Encoding) -> Result<String> {
    let rendered = match encoding {
        Encoding::JsonPretty => {
            let mut text = serde_json::to_string_pretty(value)
                .context("failed to serialize output as JSON")?;
            text.push('\n');
            text
        }
        Encoding::JsonCompact => {
            let mut text =
                serde_json::to_string(value).context("failed to serialize output as JSON")?;
            text.push('\n');
            text
        }
        Encoding::Yaml => {
            serde_yaml::to_string(value).context("failed to serialize output as YAML")?
        }
    };
    Ok(rendered)
}

/// Encode and write to stdout.
pub(crate) fn emit(value: &Value, encoding: Encoding) -> Result<()> {
    print!("{}", encode(value, encoding)?);
    Ok(())
}

/// Generic KEY/value table for single-object payloads, derived from the same
/// `Value` the machine encodings use, so it cannot show data they do not.
/// Arrays and nested objects render as single-line compact JSON.
pub(crate) fn kv_table(value: &Value) -> String {
    fn cell(v: &Value) -> String {
        match v {
            Value::Null => "-".to_string(),
            Value::String(s) => s.clone(),
            other => other.to_string(),
        }
    }

    match value {
        Value::Object(map) => {
            let width = map.keys().map(|k| k.len()).max().unwrap_or(0);
            let mut out = String::new();
            for (key, val) in map {
                out.push_str(&format!(
                    "{:<width$}  {}\n",
                    key.to_uppercase(),
                    cell(val),
                    width = width
                ));
            }
            out
        }
        other => format!("{}\n", cell(other)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn adversarial_corpus() -> Vec<Value> {
        vec![
            Value::Null,
            json!({}),
            json!([]),
            // YAML-1.1 trap strings must survive as strings, not booleans.
            json!({"answers": ["no", "yes", "on", "off", "true", "1.0", "~"]}),
            // Absent options and explicit nulls.
            json!({"present": "x", "absent": null}),
            // u64 beyond i64::MAX, negative, float.
            json!({"big": u64::MAX, "neg": -42, "float": 1.5}),
            // Unicode, embedded colons, leading '#', multi-line strings.
            json!({"cjk": "时间线: 条目", "hash": "#not-a-comment", "ml": "a\nb\n"}),
            // Deep nesting with arrays of objects.
            json!({"items": [{"id": 1, "tags": ["a", "b"]}, {"id": 2, "tags": []}]}),
        ]
    }

    #[test]
    fn json_yaml_round_trip_identity() {
        for value in adversarial_corpus() {
            for encoding in [Encoding::JsonPretty, Encoding::JsonCompact] {
                let text = encode(&value, encoding).expect("json encode");
                let back: Value = serde_json::from_str(&text).expect("json parse");
                assert_eq!(back, value, "json round trip diverged for {value}");
            }
            let text = encode(&value, Encoding::Yaml).expect("yaml encode");
            let back: Value = serde_yaml::from_str(&text).expect("yaml parse");
            assert_eq!(back, value, "yaml round trip diverged for {value}");
        }
    }

    #[test]
    fn encodings_are_newline_terminated() {
        let value = json!({"k": "v"});
        for encoding in [Encoding::JsonPretty, Encoding::JsonCompact, Encoding::Yaml] {
            let text = encode(&value, encoding).expect("encode");
            assert!(
                text.ends_with('\n'),
                "{encoding:?} output not newline-terminated"
            );
            assert!(
                !text.ends_with("\n\n"),
                "{encoding:?} output double-terminated"
            );
        }
    }

    #[test]
    fn kv_table_covers_every_top_level_key() {
        let value = json!({
            "id": "abc",
            "count": 3,
            "missing": null,
            "nested": {"a": 1},
            "list": ["x", "y"],
        });
        let table = kv_table(&value);
        for key in ["ID", "COUNT", "MISSING", "NESTED", "LIST"] {
            assert!(table.contains(key), "kv_table missing key {key}: {table}");
        }
        assert!(
            table.contains("{\"a\":1}"),
            "nested object not compact JSON"
        );
    }

    /// FR-154: the chokepoint property, asserted over the crate's source.
    /// Output serialization must have exactly one home; a reintroduced
    /// per-format projection or a swallowed serialization failure fails here.
    /// This is a structural check deliberately PAIRED with the behavioral
    /// parity tests in `output::format_parity` (§4.4: a proxy may be an
    /// additional condition, never the only one).
    #[test]
    fn chokepoint_no_stray_serializers() {
        // Named files, never subtrees (§4.4 shape 8), each with a reason.
        const YAML_OUT_ALLOWED: &[&str] = &[
            "output/render.rs", // the chokepoint itself
            "commands/tool.rs", // manifest re-serialization for gRPC apply, not stdout
        ];
        const JSON_OUT_ALLOWED: &[&str] = &[
            "output/render.rs",    // the chokepoint itself
            "commands/guide.rs",   // `guide --format json`, a documented non -o mechanism
            "commands/version.rs", // migrates to -o in FR-154 C4; remove then
        ];

        let src = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let mut violations: Vec<String> = Vec::new();
        let mut scanned = 0usize;
        visit(&src, &mut |rel, content| {
            scanned += 1;
            if content.contains("serde_yaml::to_string") && !YAML_OUT_ALLOWED.contains(&rel) {
                violations.push(format!(
                    "{rel}: serde_yaml::to_string outside the chokepoint"
                ));
            }
            if (content.contains("serde_json::to_string(")
                || content.contains("serde_json::to_string_pretty("))
                && !JSON_OUT_ALLOWED.contains(&rel)
            {
                violations.push(format!(
                    "{rel}: serde_json::to_string* outside the chokepoint"
                ));
            }
            if serialization_unwrapped_to_default(content) {
                violations.push(format!(
                    "{rel}: serialization failure swallowed by unwrap_or_default"
                ));
            }
        });
        assert!(
            scanned > 10,
            "source scan read only {scanned} files — scan is broken"
        );
        assert!(
            violations.is_empty(),
            "chokepoint violations:\n{}",
            violations.join("\n")
        );
    }

    fn visit(dir: &std::path::Path, f: &mut impl FnMut(&str, &str)) {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        for entry in std::fs::read_dir(dir).expect("read_dir") {
            let path = entry.expect("dir entry").path();
            if path.is_dir() {
                visit(&path, f);
            } else if path.extension().is_some_and(|e| e == "rs") {
                let rel = path
                    .strip_prefix(&root)
                    .expect("under src")
                    .to_string_lossy()
                    .replace('\\', "/");
                let content = std::fs::read_to_string(&path).expect("read source file");
                f(&rel, &content);
            }
        }
    }

    /// True when a `serde_json/yaml::to_string*` call and `unwrap_or_default`
    /// occur in the same statement (no `;` between them).
    fn serialization_unwrapped_to_default(content: &str) -> bool {
        let flat = content.replace('\n', " ");
        for (idx, _) in flat.match_indices("unwrap_or_default") {
            let start = flat[..idx].rfind(';').map(|p| p + 1).unwrap_or(0);
            let stmt = &flat[start..idx];
            if stmt.contains("serde_json::to_string") || stmt.contains("serde_yaml::to_string") {
                return true;
            }
        }
        false
    }
}
