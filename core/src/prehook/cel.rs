use crate::config::{
    ConvergenceContext, ItemFinalizeContext, StepPrehookContext, WorkflowFinalizeRule,
};
use anyhow::Result;
use cel_interpreter::{Program, Value as CelValue};

use super::context::{
    build_convergence_cel_context, build_finalize_cel_context, build_step_prehook_cel_context,
};

/// Evaluates a step prehook CEL expression against the provided context.
pub fn evaluate_step_prehook_expression(
    expression: &str,
    context: &StepPrehookContext,
) -> Result<bool> {
    let compiled = std::panic::catch_unwind(|| Program::compile(expression))
        .map_err(|_| anyhow::anyhow!("step '{}' prehook compilation panicked", context.step))?;
    let program = compiled.map_err(|err| {
        anyhow::anyhow!(
            "step '{}' prehook compilation failed: {}",
            context.step,
            err
        )
    })?;
    let cel_context = build_step_prehook_cel_context(context)?;
    let value = program.execute(&cel_context).map_err(|err| {
        anyhow::anyhow!("step '{}' prehook execution failed: {}", context.step, err)
    })?;
    match value {
        CelValue::Bool(v) => Ok(v),
        other => {
            anyhow::bail!(
                "step '{}' prehook must return bool, got {:?}",
                context.step,
                other
            );
        }
    }
}

/// Evaluates a finalize-rule CEL expression against the provided context.
pub fn evaluate_finalize_rule_expression(
    rule: &WorkflowFinalizeRule,
    context: &ItemFinalizeContext,
) -> Result<bool> {
    let expression = rule.when.trim();
    let compiled = std::panic::catch_unwind(|| Program::compile(expression))
        .map_err(|_| anyhow::anyhow!("finalize rule '{}' compilation panicked", rule.id))?;
    let program = compiled.map_err(|err| {
        anyhow::anyhow!("finalize rule '{}' compilation failed: {}", rule.id, err)
    })?;
    let cel_context = build_finalize_cel_context(context)?;
    let value = program
        .execute(&cel_context)
        .map_err(|err| anyhow::anyhow!("finalize rule '{}' execution failed: {}", rule.id, err))?;
    match value {
        CelValue::Bool(v) => Ok(v),
        other => anyhow::bail!(
            "finalize rule '{}' must return bool, got {:?}",
            rule.id,
            other
        ),
    }
}

/// Evaluates a webhook payload filter CEL expression.
///
/// The `payload` JSON value is injected as a CEL variable named `payload`.
/// Top-level string/number/bool fields are accessible as `payload.field_name`.
pub fn evaluate_webhook_filter(expression: &str, payload: &serde_json::Value) -> Result<bool> {
    let compiled = std::panic::catch_unwind(|| Program::compile(expression))
        .map_err(|_| anyhow::anyhow!("webhook filter compilation panicked"))?;
    let program =
        compiled.map_err(|err| anyhow::anyhow!("webhook filter compilation failed: {}", err))?;

    let mut cel_context = cel_interpreter::Context::default();
    // Inject the full payload as a JSON string variable for complex access.
    let payload_str = serde_json::to_string(payload).unwrap_or_default();
    cel_context
        .add_variable("payload_json", payload_str)
        .map_err(|e| anyhow::anyhow!("webhook filter context: {e}"))?;
    // Inject top-level fields as individual variables for direct access.
    if let serde_json::Value::Object(map) = payload {
        for (key, val) in map {
            let var_name = format!("payload_{key}");
            match val {
                serde_json::Value::String(s) => {
                    let _ = cel_context.add_variable(var_name, s.clone());
                }
                serde_json::Value::Number(n) => {
                    if let Some(i) = n.as_i64() {
                        let _ = cel_context.add_variable(var_name, i);
                    } else if let Some(f) = n.as_f64() {
                        let _ = cel_context.add_variable(var_name, f);
                    }
                }
                serde_json::Value::Bool(b) => {
                    let _ = cel_context.add_variable(var_name, *b);
                }
                _ => {
                    // Nested objects/arrays: serialize as JSON string
                    let s = serde_json::to_string(val).unwrap_or_default();
                    let _ = cel_context.add_variable(var_name, s);
                }
            }
        }
    }

    let value = program
        .execute(&cel_context)
        .map_err(|err| anyhow::anyhow!("webhook filter execution failed: {}", err))?;
    match value {
        CelValue::Bool(v) => Ok(v),
        other => anyhow::bail!("webhook filter must return bool, got {:?}", other),
    }
}

/// Evaluates a convergence CEL expression against the provided context.
pub fn evaluate_convergence_expression(
    expression: &str,
    context: &ConvergenceContext,
) -> Result<bool> {
    let compiled = std::panic::catch_unwind(|| Program::compile(expression))
        .map_err(|_| anyhow::anyhow!("convergence_expr compilation panicked"))?;
    let program =
        compiled.map_err(|err| anyhow::anyhow!("convergence_expr compilation failed: {}", err))?;
    let cel_context = build_convergence_cel_context(context)?;
    let value = program
        .execute(&cel_context)
        .map_err(|err| anyhow::anyhow!("convergence_expr execution failed: {}", err))?;
    match value {
        CelValue::Bool(v) => Ok(v),
        other => anyhow::bail!("convergence_expr must return bool, got {:?}", other),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn webhook_filter_matches_string_field() {
        let payload = serde_json::json!({"type": "message", "channel": "C123"});
        assert!(evaluate_webhook_filter("payload_type == 'message'", &payload).unwrap());
    }

    #[test]
    fn webhook_filter_rejects_non_matching() {
        let payload = serde_json::json!({"type": "reaction", "channel": "C123"});
        assert!(!evaluate_webhook_filter("payload_type == 'message'", &payload).unwrap());
    }

    #[test]
    fn webhook_filter_matches_bool_field() {
        let payload = serde_json::json!({"active": true, "count": 5});
        assert!(evaluate_webhook_filter("payload_active == true", &payload).unwrap());
    }

    #[test]
    fn webhook_filter_matches_number_field() {
        let payload = serde_json::json!({"count": 42});
        assert!(evaluate_webhook_filter("payload_count > 10", &payload).unwrap());
    }

    #[test]
    fn webhook_filter_empty_payload() {
        let payload = serde_json::json!({});
        // Expression referencing non-existent variable should fail
        assert!(evaluate_webhook_filter("payload_type == 'x'", &payload).is_err());
    }

    #[test]
    fn webhook_filter_invalid_expression() {
        let payload = serde_json::json!({"a": 1});
        assert!(evaluate_webhook_filter("invalid %%% syntax", &payload).is_err());
    }
}
