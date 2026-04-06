use agent_orchestrator::config::{PIPELINE_VAR_INLINE_LIMIT, PipelineVariables};
use std::path::Path;
use tracing::warn;

/// Insert a pipeline variable, always writing the full content to a file and
/// setting a companion `{key}_path` variable.  When the value exceeds
/// [`PIPELINE_VAR_INLINE_LIMIT`] the inline value is truncated; otherwise the
/// full value is kept inline as well.
pub(crate) fn spill_large_var(
    artifacts_dir: &Path,
    task_id: &str,
    key: &str,
    value: String,
    pipeline: &mut PipelineVariables,
) {
    // Always write to file so downstream steps can reference {key}_path
    let dir = artifacts_dir.join(task_id);
    if let Err(e) = std::fs::create_dir_all(&dir) {
        warn!(task_id, key, error = %e, "pipeline var: failed to create spill directory");
    }
    let path = dir.join(format!("{}.txt", key));
    if let Err(e) = std::fs::write(&path, &value) {
        warn!(task_id, key, path = %path.display(), error = %e, "pipeline var: failed to write spill file");
    }
    pipeline
        .vars
        .insert(format!("{}_path", key), path.to_string_lossy().to_string());

    if value.len() <= PIPELINE_VAR_INLINE_LIMIT {
        pipeline.vars.insert(key.to_string(), value);
    } else {
        let safe_end = {
            let limit = PIPELINE_VAR_INLINE_LIMIT.min(value.len());
            let mut end = limit;
            while end > 0 && !value.is_char_boundary(end) {
                end -= 1;
            }
            end
        };
        let truncated = format!(
            "{}...\n[truncated — full content at {}]",
            &value[..safe_end],
            path.display()
        );
        pipeline.vars.insert(key.to_string(), truncated);
    }
}

/// Write a large value to a spill file and return `(truncated_value, path_string)`.
/// Returns `None` if the value fits within the inline limit.
pub(crate) fn spill_to_file(
    artifacts_dir: &Path,
    task_id: &str,
    key: &str,
    value: &str,
) -> Option<(String, String)> {
    if value.len() <= PIPELINE_VAR_INLINE_LIMIT {
        return None;
    }
    let dir = artifacts_dir.join(task_id);
    if let Err(e) = std::fs::create_dir_all(&dir) {
        warn!(task_id, key, error = %e, "pipeline var: failed to create spill directory");
    }
    let path = dir.join(format!("{}.txt", key));
    if let Err(e) = std::fs::write(&path, value.as_bytes()) {
        warn!(task_id, key, path = %path.display(), error = %e, "pipeline var: failed to write spill file");
    }

    let safe_end = {
        let limit = PIPELINE_VAR_INLINE_LIMIT.min(value.len());
        let mut end = limit;
        while end > 0 && !value.is_char_boundary(end) {
            end -= 1;
        }
        end
    };
    let truncated = format!(
        "{}...\n[truncated — full content at {}]",
        &value[..safe_end],
        path.display()
    );
    Some((truncated, path.to_string_lossy().to_string()))
}
