use tonic::{Code, Status};

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
    let request_id = ["x-request-id", "request-id"]
        .into_iter()
        .find_map(|key| status.metadata().get(key))
        .and_then(|value| value.to_str().ok());
    match request_id {
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
}
