use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use agent_orchestrator::db::{insert_control_plane_audit, ControlPlaneAuditRecord};
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use tonic::{Request, Status};

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

    pub fn protect_unary<T>(
        &self,
        request: &Request<T>,
        rpc: &'static str,
    ) -> Result<ProtectionLease, Status> {
        self.acquire(request, rpc, false)
    }

    pub fn protect_stream<T>(
        &self,
        request: &Request<T>,
        rpc: &'static str,
    ) -> Result<ProtectionLease, Status> {
        self.acquire(request, rpc, true)
    }

    fn acquire<T>(
        &self,
        request: &Request<T>,
        rpc: &'static str,
        stream_mode: bool,
    ) -> Result<ProtectionLease, Status> {
        let resolved = self.resolve_context(request);
        let effective = self.effective_policy(rpc);
        let limiter = self.states.for_class(effective.class);

        self.check_rate(
            &limiter.rate_buckets,
            &resolved,
            rpc,
            effective.class,
            LimitScope::Subject,
            &resolved.subject_key,
            &effective.subject,
        )?;
        self.check_rate(
            &limiter.rate_buckets,
            &resolved,
            rpc,
            effective.class,
            LimitScope::Global,
            "global",
            &effective.global,
        )?;

        let subject_guard = if stream_mode {
            self.acquire_counter(
                limiter.active_streams.clone(),
                &resolved,
                rpc,
                effective.class,
                LimitScope::Subject,
                &resolved.subject_key,
                effective.subject.max_active_streams,
                "stream_limit_exceeded",
            )?
        } else {
            self.acquire_counter(
                limiter.in_flight.clone(),
                &resolved,
                rpc,
                effective.class,
                LimitScope::Subject,
                &resolved.subject_key,
                effective.subject.max_in_flight,
                "concurrency_limited",
            )?
        };

        let global_guard = if stream_mode {
            self.acquire_counter(
                limiter.active_streams.clone(),
                &resolved,
                rpc,
                effective.class,
                LimitScope::Global,
                "global",
                effective.global.max_active_streams,
                "stream_limit_exceeded",
            )?
        } else {
            self.acquire_counter(
                limiter.in_flight.clone(),
                &resolved,
                rpc,
                effective.class,
                LimitScope::Global,
                "global",
                effective.global.max_in_flight,
                "concurrency_limited",
            )?
        };

        Ok(ProtectionLease {
            _subject_guard: subject_guard,
            _global_guard: global_guard,
        })
    }

    fn resolve_context<T>(&self, request: &Request<T>) -> ResolvedContext {
        let remote_addr = request.remote_addr().map(|addr| addr.to_string());
        let subject_id = control_plane::subject_id_from_request(request);
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
        resolved: &ResolvedContext,
        rpc: &'static str,
        traffic_class: TrafficClass,
        scope: LimitScope,
        key: &str,
        policy: &BudgetPolicy,
    ) -> Result<(), Status> {
        let mut buckets = buckets.lock().map_err(|_| {
            self.status_and_audit(
                resolved,
                rpc,
                traffic_class,
                scope,
                "load_shed",
                Status::unavailable(format!(
                    "{rpc} rejected: traffic_class={} reason_code=load_shed",
                    traffic_class.as_str()
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
                resolved,
                rpc,
                traffic_class,
                scope,
                "rate_limited",
                Status::resource_exhausted(format!(
                    "{rpc} rejected: traffic_class={} reason_code=rate_limited",
                    traffic_class.as_str()
                )),
            ))
        }
    }

    fn acquire_counter(
        &self,
        counters: Arc<Mutex<HashMap<String, usize>>>,
        resolved: &ResolvedContext,
        rpc: &'static str,
        traffic_class: TrafficClass,
        scope: LimitScope,
        key: &str,
        max_allowed: Option<u32>,
        reason_code: &'static str,
    ) -> Result<Option<CounterGuard>, Status> {
        let Some(limit) = max_allowed else {
            return Ok(None);
        };
        let counter_map = counters.clone();
        let mut counters = counters.lock().map_err(|_| {
            self.status_and_audit(
                resolved,
                rpc,
                traffic_class,
                scope,
                "load_shed",
                Status::unavailable(format!(
                    "{rpc} rejected: traffic_class={} reason_code=load_shed",
                    traffic_class.as_str()
                )),
            )
        })?;
        let current = counters.entry(key.to_string()).or_insert(0);
        if *current >= limit as usize {
            return Err(self.status_and_audit(
                resolved,
                rpc,
                traffic_class,
                scope,
                reason_code,
                Status::resource_exhausted(format!(
                    "{rpc} rejected: traffic_class={} reason_code={reason_code}",
                    traffic_class.as_str()
                )),
            ));
        }
        *current += 1;
        Ok(Some(CounterGuard {
            map: counter_map,
            key: key.to_string(),
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
}
