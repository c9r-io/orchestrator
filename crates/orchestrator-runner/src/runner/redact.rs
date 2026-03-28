/// Redacts every configured pattern from the provided text.
pub fn redact_text(raw: &str, patterns: &[String]) -> String {
    let mut out = raw.to_string();
    for token in patterns {
        if token.trim().is_empty() {
            continue;
        }
        let lower_token = token.to_lowercase();
        let mut result = String::with_capacity(out.len());
        let lower_out = out.to_lowercase();
        let mut last = 0;
        for (idx, _) in lower_out.match_indices(lower_token.as_str()) {
            result.push_str(&out[last..idx]);
            result.push_str("[REDACTED]");
            last = idx + token.len();
        }
        result.push_str(&out[last..]);
        out = result;
    }
    out
}
