use anyhow::{anyhow, Result};
use std::net::{IpAddr, SocketAddr, ToSocketAddrs};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NetworkAllowlistHost {
    Hostname(String),
    Ip(IpAddr),
}

impl NetworkAllowlistHost {
    pub fn display(&self) -> String {
        match self {
            Self::Hostname(value) => value.clone(),
            Self::Ip(value) => value.to_string(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NetworkAllowlistEntry {
    pub original: String,
    pub host: NetworkAllowlistHost,
    pub port: Option<u16>,
}

impl NetworkAllowlistEntry {
    pub fn target_label(&self) -> String {
        match self.port {
            Some(port) => format!("{}:{port}", self.host.display()),
            None => self.host.display(),
        }
    }

    pub fn resolve_socket_addrs(&self) -> Result<Vec<SocketAddr>> {
        match &self.host {
            NetworkAllowlistHost::Ip(ip) => {
                let port = self.port.unwrap_or(0);
                Ok(vec![SocketAddr::new(*ip, port)])
            }
            NetworkAllowlistHost::Hostname(host) => {
                let port = self.port.unwrap_or(0);
                let addrs: Vec<SocketAddr> = (host.as_str(), port)
                    .to_socket_addrs()
                    .map_err(|err| {
                        anyhow!(
                            "failed to resolve network_allowlist entry '{}': {}",
                            self.original,
                            err
                        )
                    })?
                    .collect();
                if addrs.is_empty() {
                    return Err(anyhow!(
                        "network_allowlist entry '{}' resolved to no IP addresses",
                        self.original
                    ));
                }
                Ok(addrs)
            }
        }
    }
}

pub fn parse_network_allowlist_entry(raw: &str) -> Result<NetworkAllowlistEntry> {
    let value = raw.trim();
    if value.is_empty() {
        return Err(anyhow!("network_allowlist entries cannot be empty"));
    }
    if value.contains("://") {
        return Err(anyhow!(
            "network_allowlist entry '{}' must not include a URL scheme",
            raw
        ));
    }
    if value.contains('/') || value.contains('?') || value.contains('#') {
        return Err(anyhow!(
            "network_allowlist entry '{}' must not include path, query, or fragment data",
            raw
        ));
    }
    if value.contains('*') {
        return Err(anyhow!(
            "network_allowlist entry '{}' must not use wildcards",
            raw
        ));
    }

    if value.starts_with('[') {
        return parse_bracketed_ip_entry(raw, value);
    }

    if let Ok(ip) = value.parse::<IpAddr>() {
        return Ok(NetworkAllowlistEntry {
            original: value.to_string(),
            host: NetworkAllowlistHost::Ip(ip),
            port: None,
        });
    }

    if value.matches(':').count() == 1 {
        let (host_part, port_part) = value
            .rsplit_once(':')
            .ok_or_else(|| anyhow!("invalid network_allowlist entry '{}'", raw))?;
        let port = parse_port(raw, port_part)?;
        if let Ok(ip) = host_part.parse::<IpAddr>() {
            return Ok(NetworkAllowlistEntry {
                original: value.to_string(),
                host: NetworkAllowlistHost::Ip(ip),
                port: Some(port),
            });
        }
        validate_hostname(raw, host_part)?;
        return Ok(NetworkAllowlistEntry {
            original: value.to_string(),
            host: NetworkAllowlistHost::Hostname(host_part.to_string()),
            port: Some(port),
        });
    }

    if value.contains(':') {
        return Err(anyhow!(
            "network_allowlist entry '{}' must wrap IPv6 addresses in brackets when a port is present",
            raw
        ));
    }

    validate_hostname(raw, value)?;
    Ok(NetworkAllowlistEntry {
        original: value.to_string(),
        host: NetworkAllowlistHost::Hostname(value.to_string()),
        port: None,
    })
}

pub fn validate_network_allowlist(entries: &[String]) -> Result<Vec<NetworkAllowlistEntry>> {
    entries
        .iter()
        .map(|entry| parse_network_allowlist_entry(entry))
        .collect()
}

fn parse_bracketed_ip_entry(raw: &str, value: &str) -> Result<NetworkAllowlistEntry> {
    let end = value.find(']').ok_or_else(|| {
        anyhow!(
            "network_allowlist entry '{}' has an unterminated bracketed IPv6 address",
            raw
        )
    })?;
    let ip_part = &value[1..end];
    let ip = ip_part.parse::<IpAddr>().map_err(|_| {
        anyhow!(
            "network_allowlist entry '{}' does not contain a valid bracketed IPv6 address",
            raw
        )
    })?;
    let remainder = &value[end + 1..];
    let port = if remainder.is_empty() {
        None
    } else {
        let Some(port_part) = remainder.strip_prefix(':') else {
            return Err(anyhow!(
                "network_allowlist entry '{}' has invalid trailing data after IPv6 address",
                raw
            ));
        };
        Some(parse_port(raw, port_part)?)
    };
    Ok(NetworkAllowlistEntry {
        original: value.to_string(),
        host: NetworkAllowlistHost::Ip(ip),
        port,
    })
}

fn parse_port(raw: &str, value: &str) -> Result<u16> {
    value.parse::<u16>().map_err(|_| {
        anyhow!(
            "network_allowlist entry '{}' must use a valid TCP port between 1 and 65535",
            raw
        )
    })
}

fn validate_hostname(raw: &str, value: &str) -> Result<()> {
    if value.is_empty() {
        return Err(anyhow!(
            "network_allowlist entry '{}' has an empty host",
            raw
        ));
    }
    if value.len() > 253 {
        return Err(anyhow!(
            "network_allowlist entry '{}' exceeds the maximum hostname length",
            raw
        ));
    }
    if value.starts_with('.') || value.ends_with('.') || value.contains("..") {
        return Err(anyhow!(
            "network_allowlist entry '{}' is not a valid hostname",
            raw
        ));
    }
    for label in value.split('.') {
        if label.is_empty()
            || label.len() > 63
            || label.starts_with('-')
            || label.ends_with('-')
            || !label
                .chars()
                .all(|ch| ch.is_ascii_alphanumeric() || ch == '-')
        {
            return Err(anyhow!(
                "network_allowlist entry '{}' is not a valid hostname",
                raw
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_hostname_without_port() {
        let entry = parse_network_allowlist_entry("example.com").expect("parse hostname");
        assert_eq!(
            entry,
            NetworkAllowlistEntry {
                original: "example.com".to_string(),
                host: NetworkAllowlistHost::Hostname("example.com".to_string()),
                port: None,
            }
        );
    }

    #[test]
    fn parse_hostname_with_port() {
        let entry = parse_network_allowlist_entry("example.com:443").expect("parse hostname");
        assert_eq!(entry.target_label(), "example.com:443");
    }

    #[test]
    fn parse_ipv4_with_port() {
        let entry = parse_network_allowlist_entry("127.0.0.1:8080").expect("parse ipv4");
        assert_eq!(entry.target_label(), "127.0.0.1:8080");
    }

    #[test]
    fn parse_bracketed_ipv6_with_port() {
        let entry = parse_network_allowlist_entry("[::1]:8443").expect("parse ipv6");
        assert_eq!(entry.target_label(), "::1:8443");
    }

    #[test]
    fn reject_wildcard_entry() {
        let err = parse_network_allowlist_entry("*.example.com").expect_err("wildcard rejected");
        assert!(err.to_string().contains("wildcards"));
    }

    #[test]
    fn reject_scheme_entry() {
        let err =
            parse_network_allowlist_entry("https://example.com").expect_err("scheme rejected");
        assert!(err.to_string().contains("URL scheme"));
    }

    #[test]
    fn parse_unbracketed_ipv6_without_port() {
        let entry = parse_network_allowlist_entry("::1").expect("parse ipv6");
        assert_eq!(entry.target_label(), "::1");
    }
}
