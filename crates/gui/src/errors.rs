use serde::Serialize;
use tonic::{Code, Status};

/// A bounded error envelope safe to expose to the Process Console.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct SafeGrpcError {
    pub category: String,
    pub message: String,
    pub request_id: Option<String>,
}

impl From<String> for SafeGrpcError {
    fn from(_: String) -> Self {
        Self {
            category: "unavailable".into(),
            message: "The daemon is unavailable. Check that it is running, then retry.".into(),
            request_id: None,
        }
    }
}

/// Convert a gRPC status into an allowlisted category and copy. Raw provider,
/// persistence, and transport messages never cross this boundary.
pub fn safe_grpc_error(status: &Status) -> SafeGrpcError {
    let (category, message) = match status.code() {
        Code::Aborted => (
            "conflict",
            "This item changed in another session. The latest state must be confirmed before retrying.",
        ),
        Code::AlreadyExists => (
            "already_applied",
            "This request was already handled. Confirm the latest state before continuing.",
        ),
        Code::NotFound => (
            "not_found",
            "This attention item is no longer available. Confirm the latest queue state.",
        ),
        Code::InvalidArgument | Code::FailedPrecondition => (
            "invalid_request",
            "This action is no longer valid for the current item state.",
        ),
        Code::PermissionDenied | Code::Unauthenticated => (
            "permission",
            "Your current role does not allow this action.",
        ),
        Code::Unavailable => (
            "unavailable",
            "The daemon is unavailable. Check that it is running, then retry.",
        ),
        Code::DeadlineExceeded => (
            "timeout",
            "The operation timed out. Confirm the latest state before retrying.",
        ),
        _ => (
            "internal",
            "The operation failed without a confirmed state change. Retry the state check.",
        ),
    };
    SafeGrpcError {
        category: category.into(),
        message: message.into(),
        request_id: request_id(status).map(str::to_string),
    }
}

fn request_id(status: &Status) -> Option<&str> {
    ["x-request-id", "request-id"]
        .into_iter()
        .find_map(|key| status.metadata().get(key))
        .and_then(|value| value.to_str().ok())
}

/// Convert a gRPC error into a user-friendly Chinese message.
pub fn humanize_grpc_error(status: &Status) -> String {
    let message = match status.code() {
        Code::Unavailable => "无法连接到服务器，请检查 daemon 是否运行".into(),
        Code::PermissionDenied => "权限不足，需要更高级别的访问权限".into(),
        Code::NotFound => "未找到请求的资源".into(),
        Code::InvalidArgument => format!("输入内容不符合要求: {}", status.message()),
        Code::DeadlineExceeded => "操作超时，请稍后重试".into(),
        Code::AlreadyExists => "资源已存在".into(),
        Code::Aborted => format!(
            "资源已被其他操作更新，请重新加载后再试: {}",
            status.message()
        ),
        Code::Unauthenticated => "认证失败，请检查证书配置".into(),
        Code::ResourceExhausted => "资源耗尽，请稍后重试".into(),
        Code::Unimplemented => "此功能暂未实现".into(),
        Code::Internal => format!("内部错误: {}", status.message()),
        _ => "操作失败，请稍后重试".into(),
    };
    match request_id(status) {
        Some(request_id) => format!("{message}（请求 ID: {request_id}）"),
        None => message,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn humanized_error_preserves_request_id_for_daemon_correlation() {
        let mut status = Status::internal("projection failed");
        status.metadata_mut().insert(
            "x-request-id",
            "req-console-100".parse().expect("valid metadata"),
        );
        let message = humanize_grpc_error(&status);
        assert!(message.contains("内部错误"));
        assert!(message.contains("req-console-100"));
    }

    #[test]
    fn humanized_error_remains_readable_without_request_id() {
        assert_eq!(
            humanize_grpc_error(&Status::permission_denied("denied")),
            "权限不足，需要更高级别的访问权限"
        );
    }

    #[test]
    fn safe_error_uses_allowlisted_copy_and_category() {
        let status = Status::aborted(
            "provider token=secret and Slack message body must never reach the console",
        );
        let error = safe_grpc_error(&status);
        assert_eq!(error.category, "conflict");
        assert!(error.message.contains("changed in another session"));
        assert!(!error.message.contains("secret"));
        assert!(!error.message.contains("Slack"));
    }

    #[test]
    fn safe_error_retains_only_the_request_correlation_identifier() {
        let mut status = Status::internal("database path /private/state.db");
        status.metadata_mut().insert(
            "x-request-id",
            "req-attention-121".parse().expect("valid metadata"),
        );
        let error = safe_grpc_error(&status);
        assert_eq!(error.category, "internal");
        assert_eq!(error.request_id.as_deref(), Some("req-attention-121"));
        assert!(!error.message.contains("private"));
    }
}
