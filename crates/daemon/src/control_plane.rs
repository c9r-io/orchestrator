use std::collections::BTreeSet;
use std::net::{IpAddr, SocketAddr};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use agent_orchestrator::db::{insert_control_plane_audit, ControlPlaneAuditRecord};
use anyhow::{anyhow, bail, Context, Result};
use clap::ValueEnum;
use rcgen::{
    BasicConstraints, CertificateParams, DistinguishedName, DnType, IsCa, Issuer, KeyPair,
    KeyUsagePurpose, SanType,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tonic::transport::server::{TcpConnectInfo, TlsConnectInfo};
use tonic::transport::{Certificate as TonicCertificate, Identity, ServerTlsConfig};
use tonic::{Request, Status};
use x509_parser::extensions::GeneralName;
use x509_parser::prelude::{FromDer, X509Certificate};

/// Authorizes control-plane RPCs using mutual TLS identities and policy state.
#[derive(Debug, Clone)]
pub struct ControlPlaneSecurity {
    db_path: PathBuf,
    policy_path: PathBuf,
}

/// TLS server materials and authorization state for the secure gRPC listener.
#[derive(Debug, Clone)]
pub struct SecureServerConfig {
    /// TLS server configuration bound to the gRPC listener.
    pub tls: ServerTlsConfig,
    /// Shared authorization state used for request validation.
    pub security: Arc<ControlPlaneSecurity>,
}

/// Authorization failures returned while validating an incoming control-plane request.
#[derive(Debug)]
pub enum AuthzError {
    /// Client authentication failed.
    Unauthenticated(&'static str),
    /// Authenticated client lacks the required role.
    PermissionDenied(&'static str),
    /// Internal authorization error.
    Internal(String),
}

impl From<AuthzError> for Status {
    fn from(value: AuthzError) -> Self {
        match value {
            AuthzError::Unauthenticated(message) => Status::unauthenticated(message),
            AuthzError::PermissionDenied(message) => Status::permission_denied(message),
            AuthzError::Internal(message) => Status::internal(message),
        }
    }
}

struct AuditEvent<'a> {
    transport: &'a str,
    remote_addr: Option<String>,
    rpc: &'a str,
    subject_id: Option<String>,
    authn_result: &'a str,
    authz_result: &'a str,
    role: Option<String>,
    reason: Option<String>,
    tls_fingerprint: Option<String>,
    rejection_stage: Option<&'a str>,
}

/// Built-in control-plane roles ordered from least to most privileged.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ValueEnum)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    ReadOnly,
    Operator,
    Admin,
}

impl Role {
    /// Returns the stable storage label for the role.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ReadOnly => "read_only",
            Self::Operator => "operator",
            Self::Admin => "admin",
        }
    }

    fn rank(self) -> u8 {
        match self {
            Self::ReadOnly => 1,
            Self::Operator => 2,
            Self::Admin => 3,
        }
    }

    /// Returns `true` when `self` satisfies the required role.
    pub fn allows(self, required: Self) -> bool {
        self.rank() >= required.rank()
    }
}

/// A single authenticated subject that is allowed to call control-plane RPCs.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PolicySubject {
    /// Subject identifier bound to the client certificate SAN.
    pub id: String,
    /// Role granted to the subject.
    pub role: Role,
    #[serde(default)]
    /// Optional human-readable description of the subject.
    pub description: Option<String>,
    #[serde(default)]
    /// Whether the subject is disabled.
    pub disabled: bool,
}

/// Authorization policy persisted on disk for the control-plane listener.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AuthzPolicy {
    #[serde(default)]
    /// Subjects allowed to access the control-plane listener.
    pub subjects: Vec<PolicySubject>,
}

/// Kubeconfig-like client bundle written for remote control-plane access.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ControlPlaneConfig {
    /// Name of the currently selected context.
    pub current_context: String,
    /// Cluster entries available in the bundle.
    pub clusters: Vec<NamedCluster>,
    /// User entries available in the bundle.
    pub users: Vec<NamedUser>,
    /// Context entries available in the bundle.
    pub contexts: Vec<NamedContext>,
}

/// Named cluster entry inside a generated control-plane config file.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NamedCluster {
    /// Cluster entry name.
    pub name: String,
    /// Cluster reference payload.
    pub cluster: ClusterRef,
}

/// Server endpoint and CA bundle reference for a named cluster entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClusterRef {
    /// Server URL for the control-plane endpoint.
    pub server: String,
    /// Path to the CA bundle file.
    pub certificate_authority: String,
}

/// Named user entry inside a generated control-plane config file.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NamedUser {
    /// User entry name.
    pub name: String,
    /// User reference payload.
    pub user: UserRef,
}

/// Client certificate and key locations for a named user entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserRef {
    /// Path to the client certificate file.
    pub client_certificate: String,
    /// Path to the client private-key file.
    pub client_key: String,
}

/// Named context entry that binds a cluster and user together.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NamedContext {
    /// Context entry name.
    pub name: String,
    /// Context reference payload.
    pub context: ContextRef,
}

/// Cluster and user references selected by a named context entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextRef {
    /// Cluster name selected by the context.
    pub cluster: String,
    /// User name selected by the context.
    pub user: String,
}

impl ControlPlaneSecurity {
    /// Validate the presented client certificate and authorize a specific RPC.
    pub fn authorize<T>(
        &self,
        request: &Request<T>,
        rpc: &'static str,
    ) -> std::result::Result<(), AuthzError> {
        let required = required_role_for_rpc(rpc);
        let remote_addr = request.remote_addr().map(|addr| addr.to_string());
        let peer_cert = request
            .peer_certs()
            .and_then(|certs| certs.first().cloned())
            .ok_or_else(|| {
                let _ = self.audit(AuditEvent {
                    transport: "tcp",
                    remote_addr: remote_addr.clone(),
                    rpc,
                    subject_id: None,
                    authn_result: "failed",
                    authz_result: "denied",
                    role: None,
                    reason: Some("client certificate required".to_string()),
                    tls_fingerprint: None,
                    rejection_stage: Some("cert_validation_failed"),
                });
                AuthzError::Unauthenticated("client certificate required")
            })?;

        let fingerprint = sha256_fingerprint(peer_cert.as_ref());
        let subject_id = match subject_id_from_der(peer_cert.as_ref()) {
            Ok(subject) => subject,
            Err(error) => {
                let _ = self.audit(AuditEvent {
                    transport: "tcp",
                    remote_addr: remote_addr.clone(),
                    rpc,
                    subject_id: None,
                    authn_result: "failed",
                    authz_result: "denied",
                    role: None,
                    reason: Some(error.to_string()),
                    tls_fingerprint: Some(fingerprint.clone()),
                    rejection_stage: Some("cert_validation_failed"),
                });
                return Err(AuthzError::Unauthenticated(
                    "client certificate missing URI SAN",
                ));
            }
        };

        let policy = load_policy(&self.policy_path).map_err(|error| {
            AuthzError::Internal(format!("failed to load authz policy: {error}"))
        })?;
        let subject = match policy
            .subjects
            .into_iter()
            .find(|candidate| candidate.id == subject_id)
        {
            Some(subject) if !subject.disabled => subject,
            Some(_) => {
                let _ = self.audit(AuditEvent {
                    transport: "tcp",
                    remote_addr,
                    rpc,
                    subject_id: Some(subject_id),
                    authn_result: "succeeded",
                    authz_result: "denied",
                    role: None,
                    reason: Some("subject disabled".to_string()),
                    tls_fingerprint: Some(fingerprint),
                    rejection_stage: Some("subject_disabled"),
                });
                return Err(AuthzError::PermissionDenied("subject disabled"));
            }
            None => {
                let _ = self.audit(AuditEvent {
                    transport: "tcp",
                    remote_addr,
                    rpc,
                    subject_id: Some(subject_id),
                    authn_result: "succeeded",
                    authz_result: "denied",
                    role: None,
                    reason: Some("subject not present in policy".to_string()),
                    tls_fingerprint: Some(fingerprint),
                    rejection_stage: Some("subject_not_found"),
                });
                return Err(AuthzError::PermissionDenied("subject not authorized"));
            }
        };

        if !subject.role.allows(required) {
            let _ = self.audit(AuditEvent {
                transport: "tcp",
                remote_addr: request.remote_addr().map(|addr| addr.to_string()),
                rpc,
                subject_id: Some(subject.id.clone()),
                authn_result: "succeeded",
                authz_result: "denied",
                role: Some(subject.role.as_str().to_string()),
                reason: Some(format!(
                    "role {} cannot call {}",
                    subject.role.as_str(),
                    rpc
                )),
                tls_fingerprint: Some(fingerprint),
                rejection_stage: Some("role_insufficient"),
            });
            return Err(AuthzError::PermissionDenied("permission denied"));
        }

        let _ = self.audit(AuditEvent {
            transport: "tcp",
            remote_addr: request.remote_addr().map(|addr| addr.to_string()),
            rpc,
            subject_id: Some(subject.id.clone()),
            authn_result: "succeeded",
            authz_result: "allowed",
            role: Some(subject.role.as_str().to_string()),
            reason: None,
            tls_fingerprint: Some(fingerprint),
            rejection_stage: None,
        });
        if required == Role::Admin {
            let _ = self.audit(AuditEvent {
                transport: "tcp",
                remote_addr: request.remote_addr().map(|addr| addr.to_string()),
                rpc,
                subject_id: Some(subject.id.clone()),
                authn_result: "succeeded",
                authz_result: "admin_rpc_called",
                role: Some(subject.role.as_str().to_string()),
                reason: None,
                tls_fingerprint: None,
                rejection_stage: None,
            });
        }

        Ok(())
    }

    fn audit(&self, event: AuditEvent<'_>) -> Result<()> {
        insert_control_plane_audit(
            &self.db_path,
            &ControlPlaneAuditRecord {
                transport: event.transport.to_string(),
                remote_addr: event.remote_addr,
                rpc: event.rpc.to_string(),
                subject_id: event.subject_id,
                authn_result: event.authn_result.to_string(),
                authz_result: event.authz_result.to_string(),
                role: event.role,
                reason: event.reason,
                tls_fingerprint: event.tls_fingerprint,
                rejection_stage: event.rejection_stage.map(|s| s.to_string()),
                traffic_class: None,
                limit_scope: None,
                decision: None,
                reason_code: None,
            },
        )
    }
}

/// Load or bootstrap PKI material for the secure control-plane listener.
pub fn prepare_secure_server(
    data_dir: &Path,
    db_path: &Path,
    bind_addr: &SocketAddr,
    control_plane_dir: Option<&Path>,
) -> Result<SecureServerConfig> {
    let dir = control_plane_dir
        .map(Path::to_path_buf)
        .unwrap_or_else(|| data_dir.join("control-plane"));
    let paths = ControlPlanePaths::new(dir);
    bootstrap_control_plane(&paths, bind_addr)?;
    ensure_default_user_materials(data_dir, &paths, bind_addr)?;

    let server_cert = std::fs::read(&paths.server_cert)
        .with_context(|| format!("failed to read {}", paths.server_cert.display()))?;
    let server_key = std::fs::read(&paths.server_key)
        .with_context(|| format!("failed to read {}", paths.server_key.display()))?;
    let ca_cert = std::fs::read(&paths.ca_cert)
        .with_context(|| format!("failed to read {}", paths.ca_cert.display()))?;

    let tls = ServerTlsConfig::new()
        .identity(Identity::from_pem(server_cert, server_key))
        .client_ca_root(TonicCertificate::from_pem(ca_cert))
        .client_auth_optional(false);

    Ok(SecureServerConfig {
        tls,
        security: Arc::new(ControlPlaneSecurity {
            db_path: db_path.to_path_buf(),
            policy_path: paths.policy,
        }),
    })
}

/// Issue a client certificate bundle and kubeconfig-like file for one subject.
pub fn issue_client_materials(
    data_dir: &Path,
    bind_addr: &SocketAddr,
    control_plane_dir: Option<&Path>,
    home_dir: &Path,
    subject_id: &str,
    role: Role,
) -> Result<PathBuf> {
    let dir = control_plane_dir
        .map(Path::to_path_buf)
        .unwrap_or_else(|| data_dir.join("control-plane"));
    let paths = ControlPlanePaths::new(dir);
    bootstrap_control_plane(&paths, bind_addr)?;
    let username = subject_id
        .rsplit('/')
        .next()
        .filter(|value| !value.is_empty())
        .unwrap_or("client");
    let client_dir = client_home_dir(home_dir, Some(username));
    write_client_bundle(&paths, bind_addr, &client_dir, subject_id)?;
    upsert_policy_subject(
        &paths.policy,
        PolicySubject {
            id: subject_id.to_string(),
            role,
            description: Some("issued via control-plane command".to_string()),
            disabled: false,
        },
    )?;
    Ok(client_dir)
}

#[derive(Debug, Clone)]
struct ControlPlanePaths {
    base: PathBuf,
    ca_cert: PathBuf,
    ca_key: PathBuf,
    server_cert: PathBuf,
    server_key: PathBuf,
    policy: PathBuf,
}

impl ControlPlanePaths {
    fn new(base: PathBuf) -> Self {
        let pki = base.join("pki");
        Self {
            base: base.clone(),
            ca_cert: pki.join("ca.crt"),
            ca_key: pki.join("ca.key"),
            server_cert: pki.join("server.crt"),
            server_key: pki.join("server.key"),
            policy: base.join("policy.yaml"),
        }
    }
}

fn bootstrap_control_plane(paths: &ControlPlanePaths, bind_addr: &SocketAddr) -> Result<()> {
    agent_orchestrator::secure_files::ensure_dir(&paths.base, 0o700)?;
    agent_orchestrator::secure_files::ensure_dir(&paths.base.join("pki"), 0o700)?;

    if !paths.ca_cert.exists() || !paths.ca_key.exists() {
        let ca = generate_ca()?;
        agent_orchestrator::secure_files::write_atomic(
            &paths.ca_cert,
            ca.cert_pem.as_bytes(),
            0o644,
        )?;
        agent_orchestrator::secure_files::write_atomic(
            &paths.ca_key,
            ca.key_pem.as_bytes(),
            0o600,
        )?;
    }

    if !paths.server_cert.exists() || !paths.server_key.exists() {
        let server = sign_server_cert(
            &std::fs::read_to_string(&paths.ca_cert)?,
            &std::fs::read_to_string(&paths.ca_key)?,
            bind_addr,
        )?;
        agent_orchestrator::secure_files::write_atomic(
            &paths.server_cert,
            server.cert_pem.as_bytes(),
            0o644,
        )?;
        agent_orchestrator::secure_files::write_atomic(
            &paths.server_key,
            server.key_pem.as_bytes(),
            0o600,
        )?;
    }

    if !paths.policy.exists() {
        let policy = AuthzPolicy::default();
        let raw = serde_yaml::to_string(&policy).context("failed to serialize authz policy")?;
        agent_orchestrator::secure_files::write_atomic(&paths.policy, raw.as_bytes(), 0o644)?;
    }

    Ok(())
}

fn ensure_default_user_materials(
    data_dir: &Path,
    paths: &ControlPlanePaths,
    bind_addr: &SocketAddr,
) -> Result<()> {
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .ok_or_else(|| anyhow!("HOME is not set; cannot write client control-plane config"))?;
    let username = current_username();
    let subject_id = format!("spiffe://orchestrator/local-user/{username}");
    let client_dir = client_home_dir(&home, None);
    write_client_bundle(paths, bind_addr, &client_dir, &subject_id)?;
    upsert_policy_subject(
        &paths.policy,
        PolicySubject {
            id: subject_id,
            role: Role::Admin,
            description: Some(format!(
                "default local admin generated for {}",
                data_dir.display()
            )),
            disabled: false,
        },
    )
}

fn write_client_bundle(
    paths: &ControlPlanePaths,
    bind_addr: &SocketAddr,
    client_dir: &Path,
    subject_id: &str,
) -> Result<()> {
    agent_orchestrator::secure_files::ensure_dir(client_dir, 0o700)?;

    let client_cert = client_dir.join("client.crt");
    let client_key = client_dir.join("client.key");
    let ca_copy = client_dir.join("ca.crt");
    let config_path = client_dir.join("config.yaml");

    if !client_cert.exists() || !client_key.exists() {
        let client = sign_client_cert(
            &std::fs::read_to_string(&paths.ca_cert)?,
            &std::fs::read_to_string(&paths.ca_key)?,
            subject_id,
        )?;
        agent_orchestrator::secure_files::write_atomic(
            &client_cert,
            client.cert_pem.as_bytes(),
            0o644,
        )?;
        agent_orchestrator::secure_files::write_atomic(
            &client_key,
            client.key_pem.as_bytes(),
            0o600,
        )?;
    }

    agent_orchestrator::secure_files::write_atomic(
        &ca_copy,
        &std::fs::read(&paths.ca_cert)?,
        0o644,
    )?;

    let endpoint = preferred_endpoint(bind_addr);
    let config = ControlPlaneConfig {
        current_context: "default".to_string(),
        clusters: vec![NamedCluster {
            name: "default".to_string(),
            cluster: ClusterRef {
                server: format!("https://{endpoint}"),
                certificate_authority: ca_copy.display().to_string(),
            },
        }],
        users: vec![NamedUser {
            name: "default".to_string(),
            user: UserRef {
                client_certificate: client_cert.display().to_string(),
                client_key: client_key.display().to_string(),
            },
        }],
        contexts: vec![NamedContext {
            name: "default".to_string(),
            context: ContextRef {
                cluster: "default".to_string(),
                user: "default".to_string(),
            },
        }],
    };
    let raw =
        serde_yaml::to_string(&config).context("failed to serialize control-plane client config")?;
    agent_orchestrator::secure_files::write_atomic(&config_path, raw.as_bytes(), 0o644)?;
    Ok(())
}

fn upsert_policy_subject(policy_path: &Path, subject: PolicySubject) -> Result<()> {
    let mut policy = load_policy(policy_path)?;
    if let Some(existing) = policy
        .subjects
        .iter_mut()
        .find(|item| item.id == subject.id)
    {
        *existing = subject;
    } else {
        policy.subjects.push(subject);
        policy.subjects.sort_by(|a, b| a.id.cmp(&b.id));
    }
    let raw = serde_yaml::to_string(&policy).context("failed to serialize authz policy")?;
    agent_orchestrator::secure_files::write_atomic(policy_path, raw.as_bytes(), 0o644)
}

fn load_policy(policy_path: &Path) -> Result<AuthzPolicy> {
    let raw = std::fs::read_to_string(policy_path)
        .with_context(|| format!("failed to read {}", policy_path.display()))?;
    serde_yaml::from_str(&raw).context("failed to parse authz policy")
}

fn current_username() -> String {
    std::env::var("USER")
        .ok()
        .or_else(|| std::env::var("LOGNAME").ok())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "local".to_string())
}

fn client_home_dir(home: &Path, suffix: Option<&str>) -> PathBuf {
    let base = home.join(".orchestrator/control-plane");
    match suffix {
        Some(value) => base.join(value),
        None => base,
    }
}

fn preferred_endpoint(bind_addr: &SocketAddr) -> String {
    let host = match bind_addr.ip() {
        IpAddr::V4(ip) if ip.is_unspecified() => IpAddr::V4(std::net::Ipv4Addr::LOCALHOST),
        IpAddr::V6(ip) if ip.is_unspecified() => IpAddr::V6(std::net::Ipv6Addr::LOCALHOST),
        ip => ip,
    };
    format!("{host}:{}", bind_addr.port())
}

fn generate_ca() -> Result<PemBundle> {
    let mut params = CertificateParams::new(Vec::<String>::new())
        .context("failed to initialize CA certificate parameters")?;
    let mut dn = DistinguishedName::new();
    dn.push(DnType::CommonName, "orchestrator control-plane CA");
    params.distinguished_name = dn;
    params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
    params.key_usages = vec![
        KeyUsagePurpose::KeyCertSign,
        KeyUsagePurpose::DigitalSignature,
        KeyUsagePurpose::CrlSign,
    ];
    let key_pair = KeyPair::generate().context("failed to generate CA private key")?;
    let cert = params
        .self_signed(&key_pair)
        .context("failed to build CA certificate")?;
    Ok(PemBundle {
        cert_pem: cert.pem(),
        key_pem: key_pair.serialize_pem(),
    })
}

fn sign_server_cert(
    ca_cert_pem: &str,
    ca_key_pem: &str,
    bind_addr: &SocketAddr,
) -> Result<PemBundle> {
    let signer = signer_from_pem(ca_cert_pem, ca_key_pem)?;
    let mut params = CertificateParams::new(Vec::<String>::new())
        .context("failed to initialize server certificate parameters")?;
    let mut dn = DistinguishedName::new();
    dn.push(DnType::CommonName, "orchestrator-control-plane");
    params.distinguished_name = dn;
    params.key_usages = vec![
        KeyUsagePurpose::DigitalSignature,
        KeyUsagePurpose::KeyEncipherment,
    ];
    params.extended_key_usages = vec![rcgen::ExtendedKeyUsagePurpose::ServerAuth];
    for san in server_sans(bind_addr)? {
        params.subject_alt_names.push(san);
    }
    let key_pair = KeyPair::generate().context("failed to generate server private key")?;
    let cert = params
        .signed_by(&key_pair, &signer)
        .context("failed to build server certificate")?;
    Ok(PemBundle {
        cert_pem: cert.pem(),
        key_pem: key_pair.serialize_pem(),
    })
}

fn sign_client_cert(ca_cert_pem: &str, ca_key_pem: &str, subject_id: &str) -> Result<PemBundle> {
    let signer = signer_from_pem(ca_cert_pem, ca_key_pem)?;
    let mut params = CertificateParams::new(Vec::<String>::new())
        .context("failed to initialize client certificate parameters")?;
    let mut dn = DistinguishedName::new();
    dn.push(DnType::CommonName, subject_id);
    params.distinguished_name = dn;
    params.key_usages = vec![
        KeyUsagePurpose::DigitalSignature,
        KeyUsagePurpose::KeyEncipherment,
    ];
    params.extended_key_usages = vec![rcgen::ExtendedKeyUsagePurpose::ClientAuth];
    params
        .subject_alt_names
        .push(SanType::URI(subject_id.to_string().try_into()?));
    let key_pair = KeyPair::generate().context("failed to generate client private key")?;
    let cert = params
        .signed_by(&key_pair, &signer)
        .context("failed to build client certificate")?;
    Ok(PemBundle {
        cert_pem: cert.pem(),
        key_pem: key_pair.serialize_pem(),
    })
}

fn signer_from_pem(ca_cert_pem: &str, ca_key_pem: &str) -> Result<Issuer<'static, KeyPair>> {
    let key_pair = KeyPair::from_pem(ca_key_pem).context("failed to parse CA key")?;
    Issuer::from_ca_cert_pem(ca_cert_pem, key_pair).context("failed to parse CA certificate")
}

fn server_sans(bind_addr: &SocketAddr) -> Result<Vec<SanType>> {
    let mut ip_sans = BTreeSet::new();
    ip_sans.insert(IpAddr::V4(std::net::Ipv4Addr::LOCALHOST));
    ip_sans.insert(IpAddr::V6(std::net::Ipv6Addr::LOCALHOST));
    match bind_addr.ip() {
        IpAddr::V4(ip) if ip.is_unspecified() => {}
        IpAddr::V6(ip) if ip.is_unspecified() => {}
        ip => {
            ip_sans.insert(ip);
        }
    }

    let mut sans = vec![
        SanType::DnsName("localhost".try_into()?),
        SanType::DnsName("orchestrator.local".try_into()?),
    ];
    sans.extend(ip_sans.into_iter().map(SanType::IpAddress));
    Ok(sans)
}

fn required_role_for_rpc(rpc: &str) -> Role {
    match rpc {
        "Ping" | "TaskList" | "TaskInfo" | "TaskLogs" | "TaskFollow" | "TaskWatch" | "Get"
        | "Describe" | "StoreGet" | "StoreList" | "WorkerStatus" | "Check" | "ManifestExport" => {
            Role::ReadOnly
        }
        "TaskCreate" | "TaskStart" | "TaskPause" | "TaskResume" | "TaskRetry" | "Apply"
        | "StorePut" | "StoreDelete" | "StorePrune" | "ManifestValidate" | "Init" | "TaskTrace" => {
            Role::Operator
        }
        "Shutdown" | "TaskDelete" | "Delete" | "ConfigDebug" => Role::Admin,
        _ => Role::Admin,
    }
}

fn subject_id_from_der(der: &[u8]) -> Result<String> {
    let (_, cert) =
        X509Certificate::from_der(der).map_err(|_| anyhow!("invalid client certificate"))?;
    let san = cert
        .subject_alternative_name()
        .map_err(|_| anyhow!("failed to read subject alternative name"))?
        .ok_or_else(|| anyhow!("client certificate missing subject alternative name"))?;
    for name in &san.value.general_names {
        if let GeneralName::URI(uri) = name {
            return Ok(uri.to_string());
        }
    }
    bail!("client certificate missing URI SAN")
}

pub(crate) fn remote_addr_from_extensions(extensions: &http::Extensions) -> Option<String> {
    extensions
        .get::<TcpConnectInfo>()
        .and_then(|info| info.remote_addr())
        .or_else(|| {
            extensions
                .get::<TlsConnectInfo<TcpConnectInfo>>()
                .and_then(|info| info.get_ref().remote_addr())
        })
        .map(|addr| addr.to_string())
}

pub(crate) fn subject_id_from_extensions(extensions: &http::Extensions) -> Option<String> {
    extensions
        .get::<TlsConnectInfo<TcpConnectInfo>>()
        .and_then(|info| info.peer_certs())
        .and_then(|certs| certs.first().cloned())
        .and_then(|cert| subject_id_from_der(cert.as_ref()).ok())
}

fn sha256_fingerprint(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    digest
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<Vec<_>>()
        .join(":")
}

#[derive(Debug, Clone)]
struct PemBundle {
    cert_pem: String,
    key_pem: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_orchestrator::db::init_schema;

    #[test]
    fn required_role_mapping_is_stable() {
        assert_eq!(required_role_for_rpc("Ping"), Role::ReadOnly);
        assert_eq!(required_role_for_rpc("Apply"), Role::Operator);
        assert_eq!(required_role_for_rpc("Shutdown"), Role::Admin);
    }

    #[test]
    fn subject_id_round_trip_uses_uri_san() {
        let ca = generate_ca().expect("ca");
        let client = sign_client_cert(
            &ca.cert_pem,
            &ca.key_pem,
            "spiffe://orchestrator/local-user/test",
        )
        .expect("client");
        let pem = pem::parse(client.cert_pem).expect("parse pem");
        let subject = subject_id_from_der(pem.contents()).expect("subject");
        assert_eq!(subject, "spiffe://orchestrator/local-user/test");
    }

    #[test]
    fn prepare_secure_server_bootstraps_materials_and_policy() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = temp.path().join("home");
        std::fs::create_dir_all(&home).expect("home");
        // SAFETY: single-threaded test; no concurrent env reads.
        unsafe {
            std::env::set_var("HOME", &home);
            std::env::set_var("USER", "tester");
        }
        let db_path = temp.path().join("data/agent_orchestrator.db");
        std::fs::create_dir_all(db_path.parent().expect("db parent")).expect("data dir");
        init_schema(&db_path).expect("schema");
        let bind_addr: SocketAddr = "127.0.0.1:50051".parse().expect("addr");

        let secure =
            prepare_secure_server(temp.path(), &db_path, &bind_addr, None).expect("secure");
        assert!(secure.security.policy_path.exists());
        assert!(temp.path().join("control-plane/pki/ca.crt").exists());
        assert!(home
            .join(".orchestrator/control-plane/config.yaml")
            .exists());

        let policy = load_policy(&secure.security.policy_path).expect("policy");
        assert_eq!(policy.subjects.len(), 1);
        assert_eq!(policy.subjects[0].role, Role::Admin);
    }
}
