#[cfg(target_os = "linux")]
use super::profile::ResolvedExecutionProfile;
#[cfg(target_os = "linux")]
use super::sandbox::{SandboxBackend, SandboxBackendError, detect_linux_sandbox_support};
#[cfg(target_os = "linux")]
use crate::config::{ExecutionNetworkMode, RunnerConfig};
#[cfg(target_os = "linux")]
use crate::sandbox_network::{NetworkAllowlistEntry, validate_network_allowlist};
#[cfg(target_os = "linux")]
use anyhow::Result;
#[cfg(target_os = "linux")]
use std::env;
#[cfg(target_os = "linux")]
use std::net::{IpAddr, SocketAddr};

#[cfg(target_os = "linux")]
#[derive(Debug, Clone)]
pub(crate) struct ResolvedAllowlistRule {
    entry: NetworkAllowlistEntry,
    addrs: Vec<SocketAddr>,
}

#[cfg(target_os = "linux")]
pub(crate) fn build_linux_sandbox_command(
    runner: &RunnerConfig,
    command: &str,
    execution_profile: &ResolvedExecutionProfile,
) -> Result<tokio::process::Command> {
    let support = detect_linux_sandbox_support(execution_profile);
    if !support.available() {
        return Err(SandboxBackendError::backend_unavailable(
            execution_profile,
            support.backend,
            Some(&support.missing_requirements.join(", ")),
        )
        .into());
    }

    let allowlist_rules = resolve_allowlist_rules(execution_profile)?;
    let script = build_linux_sandbox_script(runner, command, execution_profile, &allowlist_rules);
    let mut cmd = tokio::process::Command::new("/bin/bash");
    cmd.arg("-lc").arg(script);
    Ok(cmd)
}

#[cfg(target_os = "linux")]
fn resolve_allowlist_rules(
    execution_profile: &ResolvedExecutionProfile,
) -> Result<Vec<ResolvedAllowlistRule>> {
    if execution_profile.network_mode != ExecutionNetworkMode::Allowlist {
        return Ok(Vec::new());
    }
    let entries = validate_network_allowlist(&execution_profile.network_allowlist)?;
    entries
        .into_iter()
        .map(|entry| {
            let addrs = entry.resolve_socket_addrs()?;
            Ok(ResolvedAllowlistRule { entry, addrs })
        })
        .collect()
}

#[cfg(target_os = "linux")]
fn read_resolv_conf_nameservers() -> Vec<IpAddr> {
    std::fs::read_to_string("/etc/resolv.conf")
        .ok()
        .map(|content| {
            content
                .lines()
                .filter_map(|line| {
                    let line = line.trim();
                    let value = line.strip_prefix("nameserver")?.trim();
                    value.parse::<IpAddr>().ok()
                })
                .collect()
        })
        .unwrap_or_default()
}

#[cfg(target_os = "linux")]
fn build_linux_sandbox_script(
    runner: &RunnerConfig,
    command: &str,
    execution_profile: &ResolvedExecutionProfile,
    allowlist_rules: &[ResolvedAllowlistRule],
) -> String {
    let token = format!(
        "orchestrator-sbx-{}-{}",
        std::process::id(),
        uuid::Uuid::new_v4().simple()
    );
    let netns = format!("{token}-ns");
    let veth_host = format!("{token}-h");
    let veth_ns = format!("{token}-n");
    let table = format!("orchestrator_{}", token.replace('-', "_"));
    let dns_servers = read_resolv_conf_nameservers();
    let allow_dns = execution_profile.network_mode == ExecutionNetworkMode::Allowlist
        && !dns_servers.is_empty();
    let host_addr = "10.203.0.1/30";
    let guest_addr = "10.203.0.2/30";
    let guest_gateway = "10.203.0.1";

    let mut lines = vec![
        "set -euo pipefail".to_string(),
        format!("NETNS={}", shell_quote(&netns)),
        format!("VETH_HOST={}", shell_quote(&veth_host)),
        format!("VETH_NS={}", shell_quote(&veth_ns)),
        format!("NFT_TABLE={}", shell_quote(&table)),
        "cleanup() {".to_string(),
        "  ip netns del \"$NETNS\" >/dev/null 2>&1 || true".to_string(),
        "  ip link del \"$VETH_HOST\" >/dev/null 2>&1 || true".to_string(),
        "  nft delete table inet \"$NFT_TABLE\" >/dev/null 2>&1 || true".to_string(),
        "}".to_string(),
        "trap cleanup EXIT".to_string(),
        "cleanup".to_string(),
        "ip netns add \"$NETNS\"".to_string(),
        "ip link add \"$VETH_HOST\" type veth peer name \"$VETH_NS\"".to_string(),
        "ip link set \"$VETH_NS\" netns \"$NETNS\"".to_string(),
        format!("ip addr add {host_addr} dev \"$VETH_HOST\""),
        "ip link set \"$VETH_HOST\" up".to_string(),
        format!("ip netns exec \"$NETNS\" ip addr add {guest_addr} dev \"$VETH_NS\""),
        "ip netns exec \"$NETNS\" ip link set lo up".to_string(),
        "ip netns exec \"$NETNS\" ip link set \"$VETH_NS\" up".to_string(),
        format!("ip netns exec \"$NETNS\" ip route add default via {guest_gateway}"),
        "sysctl -w net.ipv4.ip_forward=1 >/dev/null".to_string(),
        "nft add table inet \"$NFT_TABLE\"".to_string(),
        "nft add chain inet \"$NFT_TABLE\" postrouting '{ type nat hook postrouting priority 100; }'".to_string(),
        "nft add rule inet \"$NFT_TABLE\" postrouting oifname != \"lo\" masquerade".to_string(),
        "ip netns exec \"$NETNS\" nft add table inet sandbox".to_string(),
        "ip netns exec \"$NETNS\" nft add chain inet sandbox output '{ type filter hook output priority 0; policy drop; }'".to_string(),
        "ip netns exec \"$NETNS\" nft add rule inet sandbox output oifname 'lo' accept".to_string(),
        "ip netns exec \"$NETNS\" nft add rule inet sandbox output ct state established,related accept".to_string(),
    ];

    if execution_profile.network_mode == ExecutionNetworkMode::Allowlist {
        for rule in allowlist_rules {
            for addr in &rule.addrs {
                lines.push(build_linux_allowlist_rule(addr.ip(), rule.entry.port));
            }
        }
        if allow_dns {
            for server in &dns_servers {
                let family = if server.is_ipv4() { "ip" } else { "ip6" };
                lines.push(format!(
                    "ip netns exec \"$NETNS\" nft add rule inet sandbox output {family} daddr {} udp dport 53 accept",
                    server,
                ));
                lines.push(format!(
                    "ip netns exec \"$NETNS\" nft add rule inet sandbox output {family} daddr {} tcp dport 53 accept",
                    server,
                ));
            }
        }
    }

    let runner_shell = shell_quote(&runner.shell);
    let runner_shell_arg = shell_quote(&runner.shell_arg);
    let inner_command = shell_quote(command);
    lines.push(format!(
        "ip netns exec \"$NETNS\" {} {} {}",
        runner_shell, runner_shell_arg, inner_command
    ));
    lines.join("\n")
}

#[cfg(target_os = "linux")]
fn build_linux_allowlist_rule(ip: IpAddr, port: Option<u16>) -> String {
    let family = if ip.is_ipv4() { "ip" } else { "ip6" };
    match port {
        Some(port) => format!(
            "ip netns exec \"$NETNS\" nft add rule inet sandbox output {family} daddr {ip} tcp dport {port} accept"
        ),
        None => format!(
            "ip netns exec \"$NETNS\" nft add rule inet sandbox output {family} daddr {ip} accept"
        ),
    }
}

#[cfg(target_os = "linux")]
fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

#[cfg(target_os = "linux")]
pub(crate) fn command_exists(binary: &str) -> bool {
    env::var_os("PATH")
        .map(|paths| {
            env::split_paths(&paths).any(|dir| {
                let candidate = dir.join(binary);
                candidate.is_file()
            })
        })
        .unwrap_or(false)
}
