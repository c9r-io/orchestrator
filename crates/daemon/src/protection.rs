use std::collections::HashMap;
use std::future::Future;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::task::{Context as TaskContext, Poll};
use std::time::Instant;

use agent_orchestrator::db::{insert_control_plane_audit, ControlPlaneAuditRecord};
use anyhow::{Context, Result};
use http::{Request as HttpRequest, Response as HttpResponse};
use http_body::Body as HttpBody;
use pin_project_lite::pin_project;
use serde::{Deserialize, Serialize};
use tonic::Status;
use tower::{Layer, Service};

use crate::control_plane;

#[derive(Debug, Clone)]
pub struct ControlPlaneProtection {
    db_path: PathBuf,
    config: ProtectionConfig,
    states: Arc<LimiterStates>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProtectionConfig {
    #[serde(default)]
    pub defaults: TrafficPolicies,
    #[serde(default)]
    pub global: TrafficPolicies,
    #[serde(default)]
    pub overrides: HashMap<String, RpcProtectionOverride>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrafficPolicies {
    pub read: BudgetPolicy,
    pub write: BudgetPolicy,
    pub stream: BudgetPolicy,
    pub admin: BudgetPolicy,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BudgetPolicy {
    pub rate_per_sec: u32,
    pub burst: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_in_flight: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_active_streams: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct RpcProtectionOverride {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub class: Option<TrafficClass>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subject: Option<BudgetPolicyOverride>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub global: Option<BudgetPolicyOverride>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct BudgetPolicyOverride {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rate_per_sec: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub burst: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_in_flight: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_active_streams: Option<u32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TrafficClass {
    Read,
    Write,
    Stream,
    Admin,
}

impl TrafficClass {
    fn as_str(self) -> &'static str {
        match self {
            Self::Read => "read",
            Self::Write => "write",
            Self::Stream => "stream",
            Self::Admin => "admin",
        }
    }
}

#[derive(Debug, Clone, Copy)]
enum LimitScope {
    Subject,
    Global,
}

impl LimitScope {
    fn as_str(self) -> &'static str {
        match self {
            Self::Subject => "subject",
            Self::Global => "global",
        }
    }
}

#[derive(Debug)]
pub struct ProtectionLease {
    _subject_guard: Option<CounterGuard>,
    _global_guard: Option<CounterGuard>,
}

#[derive(Debug)]
struct CounterGuard {
    map: Arc<Mutex<HashMap<String, usize>>>,
    key: String,
}

impl Drop for CounterGuard {
    fn drop(&mut self) {
        if let Ok(mut map) = self.map.lock() {
            match map.get_mut(&self.key) {
                Some(value) if *value > 1 => *value -= 1,
                Some(_) => {
                    map.remove(&self.key);
                }
                None => {}
            }
        }
    }
}

#[derive(Debug, Default)]
struct LimiterStates {
    read: LimiterState,
    write: LimiterState,
    stream: LimiterState,
    admin: LimiterState,
}

#[derive(Debug)]
struct LimiterState {
    rate_buckets: Arc<Mutex<HashMap<String, TokenBucket>>>,
    in_flight: Arc<Mutex<HashMap<String, usize>>>,
    active_streams: Arc<Mutex<HashMap<String, usize>>>,
}

impl Default for LimiterState {
    fn default() -> Self {
        Self {
            rate_buckets: Arc::new(Mutex::new(HashMap::new())),
            in_flight: Arc::new(Mutex::new(HashMap::new())),
            active_streams: Arc::new(Mutex::new(HashMap::new())),
        }
    }
}

#[derive(Debug, Clone)]
struct ResolvedContext {
    transport: &'static str,
    remote_addr: Option<String>,
    subject_id: Option<String>,
    subject_key: String,
}

#[derive(Debug, Clone)]
struct EffectivePolicy {
    subject: BudgetPolicy,
    global: BudgetPolicy,
    class: TrafficClass,
}

#[derive(Debug, Clone, Copy)]
struct EnforcementContext<'a> {
    resolved: &'a ResolvedContext,
    rpc: &'static str,
    traffic_class: TrafficClass,
    scope: LimitScope,
}

#[derive(Debug, Clone, Copy)]
struct CounterRequest<'a> {
    max_allowed: Option<u32>,
    reason_code: &'static str,
    key: &'a str,
}

#[derive(Debug)]
struct TokenBucket {
    tokens: f64,
    burst: u32,
    rate_per_sec: f64,
    last_refill: Instant,
}

impl TokenBucket {
    fn new(rate_per_sec: u32, burst: u32) -> Self {
        Self {
            tokens: burst as f64,
            burst,
            rate_per_sec: rate_per_sec as f64,
            last_refill: Instant::now(),
        }
    }

    fn allow(&mut self) -> bool {
        let now = Instant::now();
        let elapsed = now.duration_since(self.last_refill).as_secs_f64();
        self.last_refill = now;
        self.tokens = (self.tokens + elapsed * self.rate_per_sec).min(self.burst as f64);
        if self.tokens >= 1.0 {
            self.tokens -= 1.0;
            true
        } else {
            false
        }
    }
}

impl ControlPlaneProtection {
    pub fn load_or_bootstrap(
        app_root: &Path,
        db_path: &Path,
        control_plane_dir: Option<&Path>,
    ) -> Result<Self> {
        let dir = control_plane_dir
            .map(Path::to_path_buf)
            .unwrap_or_else(|| app_root.join("data/control-plane"));
        std::fs::create_dir_all(&dir)
            .with_context(|| format!("failed to create {}", dir.display()))?;
        let config_path = dir.join("protection.yaml");
        let config = if config_path.exists() {
            let raw = std::fs::read_to_string(&config_path)
                .with_context(|| format!("failed to read {}", config_path.display()))?;
            serde_yml::from_str(&raw)
                .with_context(|| format!("failed to parse {}", config_path.display()))?
        } else {
            let config = ProtectionConfig::default();
            let raw = serde_yml::to_string(&config)
                .context("failed to serialize default protection config")?;
            std::fs::write(&config_path, raw)
                .with_context(|| format!("failed to write {}", config_path.display()))?;
            config
        };
        Ok(Self {
            db_path: db_path.to_path_buf(),
            config,
            states: Arc::new(LimiterStates::default()),
        })
    }

    pub fn protect_http<B>(
        &self,
        request: &HttpRequest<B>,
        rpc: &'static str,
    ) -> Result<ProtectionLease, Status> {
        self.acquire_http(request, rpc, is_streaming_rpc(rpc))
    }

    pub fn layer(self: Arc<Self>) -> ControlPlaneProtectionLayer {
        ControlPlaneProtectionLayer { protection: self }
    }

    fn acquire_http<B>(
        &self,
        request: &HttpRequest<B>,
        rpc: &'static str,
        stream_mode: bool,
    ) -> Result<ProtectionLease, Status> {
        let resolved = self.resolve_http_context(request);
        self.acquire_with_context(resolved, rpc, stream_mode)
    }

    fn acquire_with_context(
        &self,
        resolved: ResolvedContext,
        rpc: &'static str,
        stream_mode: bool,
    ) -> Result<ProtectionLease, Status> {
        let effective = self.effective_policy(rpc);
        let limiter = self.states.for_class(effective.class);
        let subject_context = EnforcementContext {
            resolved: &resolved,
            rpc,
            traffic_class: effective.class,
            scope: LimitScope::Subject,
        };
        let global_context = EnforcementContext {
            resolved: &resolved,
            rpc,
            traffic_class: effective.class,
            scope: LimitScope::Global,
        };

        self.check_rate(
            &limiter.rate_buckets,
            subject_context,
            &resolved.subject_key,
            &effective.subject,
        )?;
        self.check_rate(
            &limiter.rate_buckets,
            global_context,
            "global",
            &effective.global,
        )?;

        let subject_guard = if stream_mode {
            self.acquire_counter(
                limiter.active_streams.clone(),
                subject_context,
                CounterRequest {
                    max_allowed: effective.subject.max_active_streams,
                    reason_code: "stream_limit_exceeded",
                    key: &resolved.subject_key,
                },
            )?
        } else {
            self.acquire_counter(
                limiter.in_flight.clone(),
                subject_context,
                CounterRequest {
                    max_allowed: effective.subject.max_in_flight,
                    reason_code: "concurrency_limited",
                    key: &resolved.subject_key,
                },
            )?
        };

        let global_guard = if stream_mode {
            self.acquire_counter(
                limiter.active_streams.clone(),
                global_context,
                CounterRequest {
                    max_allowed: effective.global.max_active_streams,
                    reason_code: "stream_limit_exceeded",
                    key: "global",
                },
            )?
        } else {
            self.acquire_counter(
                limiter.in_flight.clone(),
                global_context,
                CounterRequest {
                    max_allowed: effective.global.max_in_flight,
                    reason_code: "concurrency_limited",
                    key: "global",
                },
            )?
        };

        Ok(ProtectionLease {
            _subject_guard: subject_guard,
            _global_guard: global_guard,
        })
    }

    fn resolve_http_context<B>(&self, request: &HttpRequest<B>) -> ResolvedContext {
        self.build_resolved_context(
            control_plane::remote_addr_from_extensions(request.extensions()),
            control_plane::subject_id_from_extensions(request.extensions()),
        )
    }

    fn build_resolved_context(
        &self,
        remote_addr: Option<String>,
        subject_id: Option<String>,
    ) -> ResolvedContext {
        let transport = if remote_addr.is_some() { "tcp" } else { "uds" };
        let subject_key = if let Some(subject_id) = &subject_id {
            format!("subject:{subject_id}")
        } else if let Some(remote_addr) = &remote_addr {
            format!("remote:{remote_addr}")
        } else {
            "local-process".to_string()
        };
        ResolvedContext {
            transport,
            remote_addr,
            subject_id,
            subject_key,
        }
    }

    fn effective_policy(&self, rpc: &'static str) -> EffectivePolicy {
        let override_config = self.config.overrides.get(rpc);
        let class = override_config
            .and_then(|item| item.class)
            .unwrap_or_else(|| classify_rpc(rpc));
        let subject = apply_override(
            self.config.defaults.policy_for(class),
            override_config.and_then(|item| item.subject.as_ref()),
        );
        let global = apply_override(
            self.config.global.policy_for(class),
            override_config.and_then(|item| item.global.as_ref()),
        );
        EffectivePolicy {
            subject,
            global,
            class,
        }
    }

    fn check_rate(
        &self,
        buckets: &Arc<Mutex<HashMap<String, TokenBucket>>>,
        context: EnforcementContext<'_>,
        key: &str,
        policy: &BudgetPolicy,
    ) -> Result<(), Status> {
        let mut buckets = buckets.lock().map_err(|_| {
            self.status_and_audit(
                context.resolved,
                context.rpc,
                context.traffic_class,
                context.scope,
                "load_shed",
                Status::unavailable(format!(
                    "{} rejected: traffic_class={} reason_code=load_shed",
                    context.rpc,
                    context.traffic_class.as_str()
                )),
            )
        })?;
        let bucket = buckets
            .entry(key.to_string())
            .or_insert_with(|| TokenBucket::new(policy.rate_per_sec, policy.burst));
        if bucket.allow() {
            Ok(())
        } else {
            Err(self.status_and_audit(
                context.resolved,
                context.rpc,
                context.traffic_class,
                context.scope,
                "rate_limited",
                Status::resource_exhausted(format!(
                    "{} rejected: traffic_class={} reason_code=rate_limited",
                    context.rpc,
                    context.traffic_class.as_str()
                )),
            ))
        }
    }

    fn acquire_counter(
        &self,
        counters: Arc<Mutex<HashMap<String, usize>>>,
        context: EnforcementContext<'_>,
        request: CounterRequest<'_>,
    ) -> Result<Option<CounterGuard>, Status> {
        let Some(limit) = request.max_allowed else {
            return Ok(None);
        };
        let counter_map = counters.clone();
        let mut counters = counters.lock().map_err(|_| {
            self.status_and_audit(
                context.resolved,
                context.rpc,
                context.traffic_class,
                context.scope,
                "load_shed",
                Status::unavailable(format!(
                    "{} rejected: traffic_class={} reason_code=load_shed",
                    context.rpc,
                    context.traffic_class.as_str()
                )),
            )
        })?;
        let current = counters.entry(request.key.to_string()).or_insert(0);
        if *current >= limit as usize {
            return Err(self.status_and_audit(
                context.resolved,
                context.rpc,
                context.traffic_class,
                context.scope,
                request.reason_code,
                Status::resource_exhausted(format!(
                    "{} rejected: traffic_class={} reason_code={}",
                    context.rpc,
                    context.traffic_class.as_str(),
                    request.reason_code
                )),
            ));
        }
        *current += 1;
        Ok(Some(CounterGuard {
            map: counter_map,
            key: request.key.to_string(),
        }))
    }

    fn status_and_audit(
        &self,
        resolved: &ResolvedContext,
        rpc: &'static str,
        traffic_class: TrafficClass,
        scope: LimitScope,
        reason_code: &'static str,
        status: Status,
    ) -> Status {
        tracing::warn!(
            rpc,
            transport = resolved.transport,
            remote_addr = resolved.remote_addr,
            subject_id = resolved.subject_id,
            traffic_class = traffic_class.as_str(),
            limit_scope = scope.as_str(),
            reason_code,
            "control plane protection rejected request"
        );
        let _ = insert_control_plane_audit(
            &self.db_path,
            &ControlPlaneAuditRecord {
                transport: resolved.transport.to_string(),
                remote_addr: resolved.remote_addr.clone(),
                rpc: rpc.to_string(),
                subject_id: resolved.subject_id.clone(),
                authn_result: "skipped".to_string(),
                authz_result: "skipped".to_string(),
                role: None,
                reason: Some(reason_code.to_string()),
                tls_fingerprint: None,
                rejection_stage: None,
                traffic_class: Some(traffic_class.as_str().to_string()),
                limit_scope: Some(scope.as_str().to_string()),
                decision: Some("rejected".to_string()),
                reason_code: Some(reason_code.to_string()),
            },
        );
        status
    }
}

#[derive(Clone)]
pub struct ControlPlaneProtectionLayer {
    protection: Arc<ControlPlaneProtection>,
}

impl<S> Layer<S> for ControlPlaneProtectionLayer {
    type Service = ControlPlaneProtectionService<S>;

    fn layer(&self, inner: S) -> Self::Service {
        ControlPlaneProtectionService {
            inner,
            protection: self.protection.clone(),
        }
    }
}

#[derive(Clone)]
pub struct ControlPlaneProtectionService<S> {
    inner: S,
    protection: Arc<ControlPlaneProtection>,
}

impl<S, ReqBody, ResBody> Service<HttpRequest<ReqBody>> for ControlPlaneProtectionService<S>
where
    S: Service<HttpRequest<ReqBody>, Response = HttpResponse<ResBody>> + Clone + Send + 'static,
    S::Future: Send + 'static,
    ReqBody: Send + 'static,
    ResBody: HttpBody + Send + 'static,
    ResBody::Error: Send + 'static,
{
    type Response = HttpResponse<ProtectedBody<ResBody>>;
    type Error = S::Error;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send>>;

    fn poll_ready(&mut self, cx: &mut TaskContext<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, request: HttpRequest<ReqBody>) -> Self::Future {
        let decision = rpc_from_path(request.uri().path()).map(|rpc| {
            self.protection
                .protect_http(&request, rpc)
                .map(|lease| (rpc, lease))
        });

        match decision {
            Some(Err(status)) => {
                let (parts, ()) = status.into_http::<()>().into_parts();
                let response = HttpResponse::from_parts(parts, ProtectedBody::empty());
                Box::pin(async move { Ok(response) })
            }
            Some(Ok((_rpc, lease))) => {
                let future = self.inner.call(request);
                Box::pin(async move {
                    future
                        .await
                        .map(|response| response.map(|body| ProtectedBody::new(body, Some(lease))))
                })
            }
            None => {
                let future = self.inner.call(request);
                Box::pin(async move {
                    future
                        .await
                        .map(|response| response.map(|body| ProtectedBody::new(body, None)))
                })
            }
        }
    }
}

pin_project! {
    #[derive(Debug)]
    pub struct ProtectedBody<B> {
        #[pin]
        kind: ProtectedBodyKind<B>,
        _lease: Option<ProtectionLease>,
    }
}

pin_project! {
    #[derive(Debug)]
    #[project = ProtectedBodyKindProj]
    enum ProtectedBodyKind<B> {
        Empty,
        Wrap { #[pin] body: B },
    }
}

impl<B> ProtectedBody<B> {
    fn new(body: B, lease: Option<ProtectionLease>) -> Self {
        Self {
            kind: ProtectedBodyKind::Wrap { body },
            _lease: lease,
        }
    }

    fn empty() -> Self {
        Self {
            kind: ProtectedBodyKind::Empty,
            _lease: None,
        }
    }
}

impl<B> HttpBody for ProtectedBody<B>
where
    B: HttpBody,
{
    type Data = B::Data;
    type Error = B::Error;

    fn poll_frame(
        self: Pin<&mut Self>,
        cx: &mut TaskContext<'_>,
    ) -> Poll<Option<Result<http_body::Frame<Self::Data>, Self::Error>>> {
        match self.project().kind.project() {
            ProtectedBodyKindProj::Empty => Poll::Ready(None),
            ProtectedBodyKindProj::Wrap { body } => body.poll_frame(cx),
        }
    }

    fn is_end_stream(&self) -> bool {
        match &self.kind {
            ProtectedBodyKind::Empty => true,
            ProtectedBodyKind::Wrap { body } => body.is_end_stream(),
        }
    }

    fn size_hint(&self) -> http_body::SizeHint {
        match &self.kind {
            ProtectedBodyKind::Empty => http_body::SizeHint::with_exact(0),
            ProtectedBodyKind::Wrap { body } => body.size_hint(),
        }
    }
}

impl LimiterStates {
    fn for_class(&self, class: TrafficClass) -> &LimiterState {
        match class {
            TrafficClass::Read => &self.read,
            TrafficClass::Write => &self.write,
            TrafficClass::Stream => &self.stream,
            TrafficClass::Admin => &self.admin,
        }
    }
}

impl Default for ProtectionConfig {
    fn default() -> Self {
        Self {
            defaults: TrafficPolicies {
                read: BudgetPolicy {
                    rate_per_sec: 20,
                    burst: 40,
                    max_in_flight: Some(32),
                    max_active_streams: None,
                },
                write: BudgetPolicy {
                    rate_per_sec: 5,
                    burst: 10,
                    max_in_flight: Some(8),
                    max_active_streams: None,
                },
                stream: BudgetPolicy {
                    rate_per_sec: 1,
                    burst: 2,
                    max_in_flight: None,
                    max_active_streams: Some(2),
                },
                admin: BudgetPolicy {
                    rate_per_sec: 1,
                    burst: 2,
                    max_in_flight: Some(1),
                    max_active_streams: None,
                },
            },
            global: TrafficPolicies {
                read: BudgetPolicy {
                    rate_per_sec: 100,
                    burst: 200,
                    max_in_flight: Some(128),
                    max_active_streams: None,
                },
                write: BudgetPolicy {
                    rate_per_sec: 25,
                    burst: 50,
                    max_in_flight: Some(32),
                    max_active_streams: None,
                },
                stream: BudgetPolicy {
                    rate_per_sec: 8,
                    burst: 16,
                    max_in_flight: None,
                    max_active_streams: Some(32),
                },
                admin: BudgetPolicy {
                    rate_per_sec: 5,
                    burst: 10,
                    max_in_flight: Some(4),
                    max_active_streams: None,
                },
            },
            overrides: HashMap::new(),
        }
    }
}

impl Default for TrafficPolicies {
    fn default() -> Self {
        Self {
            read: BudgetPolicy {
                rate_per_sec: 20,
                burst: 40,
                max_in_flight: Some(32),
                max_active_streams: None,
            },
            write: BudgetPolicy {
                rate_per_sec: 5,
                burst: 10,
                max_in_flight: Some(8),
                max_active_streams: None,
            },
            stream: BudgetPolicy {
                rate_per_sec: 1,
                burst: 2,
                max_in_flight: None,
                max_active_streams: Some(2),
            },
            admin: BudgetPolicy {
                rate_per_sec: 1,
                burst: 2,
                max_in_flight: Some(1),
                max_active_streams: None,
            },
        }
    }
}

impl TrafficPolicies {
    fn policy_for(&self, class: TrafficClass) -> &BudgetPolicy {
        match class {
            TrafficClass::Read => &self.read,
            TrafficClass::Write => &self.write,
            TrafficClass::Stream => &self.stream,
            TrafficClass::Admin => &self.admin,
        }
    }
}

fn apply_override(
    base: &BudgetPolicy,
    override_config: Option<&BudgetPolicyOverride>,
) -> BudgetPolicy {
    let mut merged = base.clone();
    if let Some(override_config) = override_config {
        if let Some(rate_per_sec) = override_config.rate_per_sec {
            merged.rate_per_sec = rate_per_sec;
        }
        if let Some(burst) = override_config.burst {
            merged.burst = burst;
        }
        if let Some(max_in_flight) = override_config.max_in_flight {
            merged.max_in_flight = Some(max_in_flight);
        }
        if let Some(max_active_streams) = override_config.max_active_streams {
            merged.max_active_streams = Some(max_active_streams);
        }
    }
    merged
}

fn classify_rpc(rpc: &str) -> TrafficClass {
    match rpc {
        "TaskFollow" | "TaskWatch" => TrafficClass::Stream,
        "Shutdown" | "Init" | "TaskDelete" | "Delete" | "StorePrune" | "SecretKeyRotate"
        | "SecretKeyRevoke" => TrafficClass::Admin,
        "TaskCreate" | "TaskStart" | "TaskPause" | "TaskResume" | "TaskRetry" | "Apply"
        | "StorePut" | "StoreDelete" => TrafficClass::Write,
        _ => TrafficClass::Read,
    }
}

fn is_streaming_rpc(rpc: &str) -> bool {
    matches!(rpc, "TaskFollow" | "TaskWatch")
}

fn rpc_from_path(path: &str) -> Option<&'static str> {
    match path {
        "/orchestrator.OrchestratorService/TaskCreate" => Some("TaskCreate"),
        "/orchestrator.OrchestratorService/TaskStart" => Some("TaskStart"),
        "/orchestrator.OrchestratorService/TaskPause" => Some("TaskPause"),
        "/orchestrator.OrchestratorService/TaskResume" => Some("TaskResume"),
        "/orchestrator.OrchestratorService/TaskDelete" => Some("TaskDelete"),
        "/orchestrator.OrchestratorService/TaskRetry" => Some("TaskRetry"),
        "/orchestrator.OrchestratorService/TaskList" => Some("TaskList"),
        "/orchestrator.OrchestratorService/TaskInfo" => Some("TaskInfo"),
        "/orchestrator.OrchestratorService/TaskLogs" => Some("TaskLogs"),
        "/orchestrator.OrchestratorService/TaskFollow" => Some("TaskFollow"),
        "/orchestrator.OrchestratorService/TaskWatch" => Some("TaskWatch"),
        "/orchestrator.OrchestratorService/Apply" => Some("Apply"),
        "/orchestrator.OrchestratorService/Get" => Some("Get"),
        "/orchestrator.OrchestratorService/Describe" => Some("Describe"),
        "/orchestrator.OrchestratorService/Delete" => Some("Delete"),
        "/orchestrator.OrchestratorService/StoreGet" => Some("StoreGet"),
        "/orchestrator.OrchestratorService/StorePut" => Some("StorePut"),
        "/orchestrator.OrchestratorService/StoreDelete" => Some("StoreDelete"),
        "/orchestrator.OrchestratorService/StoreList" => Some("StoreList"),
        "/orchestrator.OrchestratorService/StorePrune" => Some("StorePrune"),
        "/orchestrator.OrchestratorService/Ping" => Some("Ping"),
        "/orchestrator.OrchestratorService/Shutdown" => Some("Shutdown"),
        "/orchestrator.OrchestratorService/ConfigDebug" => Some("ConfigDebug"),
        "/orchestrator.OrchestratorService/WorkerStatus" => Some("WorkerStatus"),
        "/orchestrator.OrchestratorService/Check" => Some("Check"),
        "/orchestrator.OrchestratorService/Init" => Some("Init"),
        "/orchestrator.OrchestratorService/DbStatus" => Some("DbStatus"),
        "/orchestrator.OrchestratorService/DbMigrationsList" => Some("DbMigrationsList"),
        "/orchestrator.OrchestratorService/ManifestValidate" => Some("ManifestValidate"),
        "/orchestrator.OrchestratorService/ManifestExport" => Some("ManifestExport"),
        "/orchestrator.OrchestratorService/TaskTrace" => Some("TaskTrace"),
        "/orchestrator.OrchestratorService/SecretKeyStatus" => Some("SecretKeyStatus"),
        "/orchestrator.OrchestratorService/SecretKeyList" => Some("SecretKeyList"),
        "/orchestrator.OrchestratorService/SecretKeyRotate" => Some("SecretKeyRotate"),
        "/orchestrator.OrchestratorService/SecretKeyRevoke" => Some("SecretKeyRevoke"),
        "/orchestrator.OrchestratorService/SecretKeyHistory" => Some("SecretKeyHistory"),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rpc_classification_is_stable() {
        assert_eq!(classify_rpc("Ping"), TrafficClass::Read);
        assert_eq!(classify_rpc("TaskCreate"), TrafficClass::Write);
        assert_eq!(classify_rpc("TaskWatch"), TrafficClass::Stream);
        assert_eq!(classify_rpc("Shutdown"), TrafficClass::Admin);
    }

    #[test]
    fn token_bucket_denies_when_burst_is_exhausted() {
        let mut bucket = TokenBucket::new(1, 2);
        assert!(bucket.allow());
        assert!(bucket.allow());
        assert!(!bucket.allow());
    }

    #[test]
    fn override_merges_fields() {
        let merged = apply_override(
            &BudgetPolicy {
                rate_per_sec: 1,
                burst: 2,
                max_in_flight: Some(3),
                max_active_streams: None,
            },
            Some(&BudgetPolicyOverride {
                rate_per_sec: Some(5),
                burst: None,
                max_in_flight: Some(7),
                max_active_streams: None,
            }),
        );
        assert_eq!(merged.rate_per_sec, 5);
        assert_eq!(merged.burst, 2);
        assert_eq!(merged.max_in_flight, Some(7));
    }

    #[test]
    fn route_mapping_is_stable() {
        assert_eq!(
            rpc_from_path("/orchestrator.OrchestratorService/TaskList"),
            Some("TaskList")
        );
        assert_eq!(
            rpc_from_path("/orchestrator.OrchestratorService/TaskWatch"),
            Some("TaskWatch")
        );
        assert_eq!(
            rpc_from_path("/orchestrator.OrchestratorService/Unknown"),
            None
        );
    }
}
