//! FR-154: machine-readable dump of the clap command tree.
//!
//! `config/governance/cli-surface.json` is the committed projection of
//! `Cli::command()` that the documentation-parity gate
//! (`scripts/qa/test-cli-doc-parity.sh`) and the built-in guide consume, so
//! the three documentation surfaces derive from clap instead of hand-copying
//! it. `cli_surface_json_is_fresh` regenerates the JSON in memory and fails on
//! any drift; run with `ORCHESTRATOR_WRITE_CLI_SURFACE=1` to rewrite the file.

use clap::CommandFactory;
use serde_json::{Value, json};

/// Repo-relative path of the committed surface file.
pub(crate) const SURFACE_PATH: &str = "config/governance/cli-surface.json";

/// One node of the flattened command tree.
struct Node {
    path: String,
    hidden: bool,
    leaf: bool,
    bare_invocable: bool,
    about: String,
    aliases: Vec<String>,
    args: Vec<Value>,
}

/// Flatten the built clap tree, depth-first. `hidden` propagates: everything
/// under a hidden ancestor is hidden.
fn collect(cmd: &clap::Command, prefix: &str, hidden_ancestor: bool, out: &mut Vec<Node>) {
    for sub in cmd.get_subcommands() {
        // clap synthesizes a `help` subcommand at every level; it is not part
        // of the documented surface.
        if sub.get_name() == "help" {
            continue;
        }
        let hidden = hidden_ancestor || sub.is_hide_set();
        let path = if prefix.is_empty() {
            sub.get_name().to_string()
        } else {
            format!("{prefix} {}", sub.get_name())
        };
        let has_subcommands = sub.get_subcommands().any(|s| s.get_name() != "help");
        out.push(Node {
            path: path.clone(),
            hidden,
            leaf: !has_subcommands,
            bare_invocable: has_subcommands && !sub.is_subcommand_required_set(),
            about: sub.get_about().map(|s| s.to_string()).unwrap_or_default(),
            aliases: sub.get_visible_aliases().map(str::to_string).collect(),
            args: sub
                .get_arguments()
                .filter(|arg| {
                    arg.get_id() != "help" && arg.get_id() != "version" && !arg.is_global_set()
                })
                .map(arg_value)
                .collect(),
        });
        collect(sub, &path, hidden, out);
    }
}

fn arg_value(arg: &clap::Arg) -> Value {
    let possible_values: Vec<String> = arg
        .get_possible_values()
        .iter()
        .map(|v| v.get_name().to_string())
        .collect();
    json!({
        "name": arg.get_id().to_string(),
        "long": arg.get_long(),
        "short": arg.get_short().map(String::from),
        "value_name": arg.get_value_names().and_then(|names| names.first()).map(|v| v.to_string()),
        "possible_values": possible_values,
        "default": arg
            .get_default_values()
            .first()
            .map(|v| v.to_string_lossy().to_string()),
        "required": arg.is_required_set(),
        "positional": arg.is_positional(),
        "hidden": arg.is_hide_set(),
    })
}

/// Render the whole surface as the committed JSON text (sorted by path,
/// newline-terminated).
pub(crate) fn render_surface_json() -> anyhow::Result<String> {
    let mut cmd = crate::Cli::command();
    cmd.build();
    let mut nodes = Vec::new();
    collect(&cmd, "", false, &mut nodes);
    nodes.sort_by(|a, b| a.path.cmp(&b.path));

    let commands: Vec<Value> = nodes
        .iter()
        .map(|n| {
            json!({
                "path": n.path,
                "hidden": n.hidden,
                "leaf": n.leaf,
                "bare_invocable": n.bare_invocable,
                "about": n.about,
                "aliases": n.aliases,
                "args": n.args,
            })
        })
        .collect();

    let doc = json!({
        "version": 1,
        "description": "Machine-readable clap command tree (FR-154). Generated — do not edit. Regenerate: ORCHESTRATOR_WRITE_CLI_SURFACE=1 cargo test -p orchestrator-cli cli_surface_json_is_fresh",
        "commands": commands,
    });
    crate::output::render::encode(&doc, crate::output::render::Encoding::JsonPretty)
}

/// The set of visible invocable command paths: every non-hidden leaf, plus
/// non-hidden parents that are bare-invocable (e.g. `debug`).
#[cfg(test)]
pub(crate) fn visible_invocable_paths() -> Vec<String> {
    let mut cmd = crate::Cli::command();
    cmd.build();
    let mut nodes = Vec::new();
    collect(&cmd, "", false, &mut nodes);
    nodes
        .iter()
        .filter(|n| !n.hidden && (n.leaf || n.bare_invocable))
        .map(|n| n.path.clone())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn repo_surface_path() -> std::path::PathBuf {
        // crates/cli -> repo root is two levels up.
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .join(SURFACE_PATH)
    }

    #[test]
    fn cli_surface_json_is_fresh() {
        let generated = render_surface_json().expect("render surface");
        let path = repo_surface_path();
        if std::env::var_os("ORCHESTRATOR_WRITE_CLI_SURFACE").is_some() {
            std::fs::write(&path, &generated).expect("write surface file");
            return;
        }
        let committed = std::fs::read_to_string(&path).unwrap_or_default();
        assert_eq!(
            committed, generated,
            "config/governance/cli-surface.json is stale. Regenerate with:\n  \
             ORCHESTRATOR_WRITE_CLI_SURFACE=1 cargo test -p orchestrator-cli cli_surface_json_is_fresh"
        );
    }

    #[test]
    fn surface_covers_known_tree_shape() {
        let text = render_surface_json().expect("render surface");
        let doc: serde_json::Value = serde_json::from_str(&text).expect("surface parses");
        let commands = doc["commands"].as_array().expect("commands array");
        assert!(
            commands.len() > 120,
            "surface holds only {} commands — the walk is broken",
            commands.len()
        );
        let by_path = |p: &str| {
            commands
                .iter()
                .find(|c| c["path"] == p)
                .unwrap_or_else(|| panic!("missing path {p}"))
        };
        // Hidden propagation: the two hide=true leaves and nothing else.
        assert_eq!(by_path("debug child-idle")["hidden"], true);
        assert_eq!(by_path("debug sandbox-probe tcp-serve")["hidden"], true);
        assert_eq!(
            commands.iter().filter(|c| c["hidden"] == true).count(),
            2,
            "unexpected hidden command count"
        );
        // debug is bare-invocable with subcommands.
        assert_eq!(by_path("debug")["bare_invocable"], true);
        assert_eq!(by_path("debug")["leaf"], false);
        // A deep leaf with -o carries its possible values and default.
        let args = by_path("task list")["args"].as_array().expect("args");
        let output = args
            .iter()
            .find(|a| a["name"] == "output")
            .expect("task list -o");
        assert_eq!(output["default"], "table");
        assert_eq!(
            output["possible_values"],
            serde_json::json!(["table", "json", "yaml"])
        );
        // Invocable set = visible leaves + bare-invocable parents (debug).
        let invocable = visible_invocable_paths();
        let visible_leaves = commands
            .iter()
            .filter(|c| c["leaf"] == true && c["hidden"] == false)
            .count();
        assert_eq!(
            invocable.len(),
            visible_leaves + 1,
            "only debug is bare-invocable"
        );
        assert!(invocable.iter().any(|p| p == "debug"));
        // The deprecated hidden alias is recorded as hidden.
        let version_args = by_path("version")["args"].as_array().expect("version args");
        let json_alias = version_args
            .iter()
            .find(|a| a["name"] == "json")
            .expect("version --json");
        assert_eq!(json_alias["hidden"], true);
    }
}
