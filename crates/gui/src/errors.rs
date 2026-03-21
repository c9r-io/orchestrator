use tonic::{Code, Status};

/// Convert a gRPC error into a user-friendly Chinese message.
pub fn humanize_grpc_error(status: &Status) -> String {
    match status.code() {
        Code::Unavailable => "无法连接到服务器，请检查 daemon 是否运行".into(),
        Code::PermissionDenied => "权限不足，需要更高级别的访问权限".into(),
        Code::NotFound => "未找到请求的资源".into(),
        Code::InvalidArgument => format!("输入内容不符合要求: {}", status.message()),
        Code::DeadlineExceeded => "操作超时，请稍后重试".into(),
        Code::AlreadyExists => "资源已存在".into(),
        Code::Unauthenticated => "认证失败，请检查证书配置".into(),
        Code::ResourceExhausted => "资源耗尽，请稍后重试".into(),
        Code::Unimplemented => "此功能暂未实现".into(),
        Code::Internal => format!("内部错误: {}", status.message()),
        _ => "操作失败，请稍后重试".into(),
    }
}
