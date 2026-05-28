// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026-present Patrick S Connallon
//
// nexthop - TCP / UDP - Unicast / Multicast / Broadcast
// Architect: Patrick S Connallon
// Built by:  Anthropic Claude (Sonnet 4.6)

use crate::error::{RelayError, Result};
use serde::{Deserialize, Serialize};
use std::net::SocketAddr;

// ── Top-level configuration ────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct RelayConfig {
    pub general: GeneralConfig,
    pub source: EndpointConfig,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rate_limit: Option<RateLimitConfig>,
    pub destinations: Vec<DestConfig>,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct GeneralConfig {
    #[serde(default = "defaults::log_level")]
    pub log_level: String,
    #[serde(default = "defaults::stats_interval")]
    pub stats_interval_secs: u64,
    #[serde(default = "defaults::channel_capacity")]
    pub channel_capacity: usize,
    #[serde(default = "defaults::max_payload")]
    pub max_payload_size: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub health_port: Option<u16>,
    /// Bind address for the health/stats/metrics HTTP server.
    ///
    /// `None` (the default) binds **both** `0.0.0.0` and `::` so the
    /// server is reachable on every interface over IPv4 and IPv6.
    /// `Some("0.0.0.0")` restricts to IPv4 only; `Some("::")` restricts
    /// to IPv6 only; any specific IP (e.g. `"127.0.0.1"`) limits to
    /// that interface.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub health_bind_addr: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct RateLimitConfig {
    pub bytes_per_second: u64,
    #[serde(default = "defaults::burst")]
    pub burst_size: u64,
}

// ── Endpoint (source or destination) ───────────────────────────────

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct EndpointConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    pub protocol: Protocol,
    pub mode: EndpointMode,
    pub address: String,
    #[serde(default)]
    pub cast_mode: CastMode,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub multicast_interface: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub multicast_interface_index: Option<u32>,
    #[serde(default = "defaults::multicast_ttl")]
    pub multicast_ttl: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reconnect_delay_ms: Option<u64>,
}

/// A destination endpoint.  Extends [`EndpointConfig`] with `overflow_policy`,
/// which only makes sense for outbound queues, not for the source.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct DestConfig {
    #[serde(flatten)]
    pub base: EndpointConfig,
    #[serde(default)]
    pub overflow_policy: OverflowPolicy,
    /// Per-destination rate limit. When set, it gates writes to this
    /// destination only and overrides the global `[rate_limit]` for
    /// this destination. When unset, the destination falls back to the
    /// global limiter (shared across all destinations without their
    /// own override). See MANUAL.md "Rate limiting" for the
    /// precedence rules.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rate_limit: Option<RateLimitConfig>,
}

impl std::ops::Deref for DestConfig {
    type Target = EndpointConfig;
    fn deref(&self) -> &EndpointConfig {
        &self.base
    }
}

#[derive(Debug, Clone, Copy, Hash, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Protocol {
    Tcp,
    Udp,
}

#[derive(Debug, Clone, Copy, Hash, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum EndpointMode {
    Server,
    Client,
}

#[derive(Debug, Clone, Copy, Hash, PartialEq, Eq, Default, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum CastMode {
    #[default]
    Unicast,
    Broadcast,
    Multicast,
}

/// What to do when a destination's queue is full.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum OverflowPolicy {
    #[default]
    DropNewest,
    Block,
}

// ── Helpers ────────────────────────────────────────────────────────

impl EndpointConfig {
    pub fn socket_addr(&self) -> Result<SocketAddr> {
        self.address.parse::<SocketAddr>().map_err(Into::into)
    }

    pub fn display_name(&self) -> String {
        self.name.clone().unwrap_or_else(|| self.address.clone())
    }

    pub fn reconnect_delay(&self) -> std::time::Duration {
        std::time::Duration::from_millis(self.reconnect_delay_ms.unwrap_or(2000))
    }
}

impl RelayConfig {
    /// Load and validate from a TOML file.
    pub fn from_file(path: &str) -> Result<Self> {
        let content = std::fs::read_to_string(path)
            .map_err(|e| RelayError::Config(format!("cannot read '{path}': {e}")))?;
        let cfg: Self = toml::from_str(&content)?;
        cfg.validate()?;
        Ok(cfg)
    }

    pub fn validate(&self) -> Result<()> {
        self.source.socket_addr()?;

        if self.source.protocol == Protocol::Tcp && self.source.cast_mode != CastMode::Unicast {
            return Err(RelayError::Config(
                "source: broadcast/multicast requires protocol = \"udp\"".into(),
            ));
        }

        if self.source.cast_mode == CastMode::Multicast {
            if let Some(ref iface) = self.source.multicast_interface {
                if !iface.is_empty() && iface.parse::<std::net::Ipv4Addr>().is_err() {
                    return Err(RelayError::Config(format!(
                        "source: multicast_interface '{iface}' is not a valid IPv4 address \
                         (use an IP address, not an interface name)"
                    )));
                }
            }
        }

        if self.destinations.is_empty() {
            return Err(RelayError::Config(
                "at least one [[destinations]] entry is required".into(),
            ));
        }

        for (i, d) in self.destinations.iter().enumerate() {
            d.socket_addr().map_err(|e| {
                RelayError::Config(format!(
                    "destination[{i}]: invalid address '{}': {e}",
                    d.address
                ))
            })?;
            if d.protocol == Protocol::Tcp && d.cast_mode != CastMode::Unicast {
                return Err(RelayError::Config(format!(
                    "destination[{i}]: broadcast/multicast requires protocol = \"udp\""
                )));
            }
            if d.cast_mode == CastMode::Multicast {
                if let Some(ref iface) = d.multicast_interface {
                    if !iface.is_empty() && iface.parse::<std::net::Ipv4Addr>().is_err() {
                        return Err(RelayError::Config(format!(
                            "destination[{i}]: multicast_interface '{iface}' is not a valid \
                             IPv4 address (use an IP address, not an interface name)"
                        )));
                    }
                }
            }
            if let Some(ref rl) = d.rate_limit {
                if rl.bytes_per_second == 0 {
                    return Err(RelayError::Config(format!(
                        "destination[{i}]: rate_limit.bytes_per_second must be > 0"
                    )));
                }
            }
        }

        if let Some(ref rl) = self.rate_limit {
            if rl.bytes_per_second == 0 {
                return Err(RelayError::Config(
                    "rate_limit.bytes_per_second must be > 0".into(),
                ));
            }
        }

        if self.general.max_payload_size == 0 {
            return Err(RelayError::Config(
                "general.max_payload_size must be > 0".into(),
            ));
        }

        if let Some(ref addr) = self.general.health_bind_addr {
            addr.parse::<std::net::IpAddr>().map_err(|e| {
                RelayError::Config(format!(
                    "general.health_bind_addr '{addr}' is not a valid IP address: {e}"
                ))
            })?;
        }

        Ok(())
    }
}

// ── TOML serialisation ─────────────────────────────────────────────
//
// We build the TOML string manually instead of using toml::to_string_pretty
// because toml 0.8 does not reliably handle #[serde(flatten)] inside
// [[array-of-tables]] during serialisation.

fn toml_str(s: &str) -> String {
    let escaped = s
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
        .replace('\r', "\\r");
    format!("\"{escaped}\"")
}

fn write_endpoint_fields(out: &mut String, e: &EndpointConfig) {
    if let Some(ref name) = e.name {
        out.push_str(&format!("name = {}\n", toml_str(name)));
    }
    let proto = match e.protocol {
        Protocol::Tcp => "tcp",
        Protocol::Udp => "udp",
    };
    out.push_str(&format!("protocol = {}\n", toml_str(proto)));
    let mode = match e.mode {
        EndpointMode::Server => "server",
        EndpointMode::Client => "client",
    };
    out.push_str(&format!("mode = {}\n", toml_str(mode)));
    out.push_str(&format!("address = {}\n", toml_str(&e.address)));
    if e.cast_mode != CastMode::Unicast {
        let cast = match e.cast_mode {
            CastMode::Unicast => "unicast",
            CastMode::Broadcast => "broadcast",
            CastMode::Multicast => "multicast",
        };
        out.push_str(&format!("cast_mode = {}\n", toml_str(cast)));
    }
    if let Some(ref iface) = e.multicast_interface {
        out.push_str(&format!("multicast_interface = {}\n", toml_str(iface)));
    }
    if let Some(idx) = e.multicast_interface_index {
        out.push_str(&format!("multicast_interface_index = {idx}\n"));
    }
    if e.multicast_ttl != defaults::multicast_ttl() {
        out.push_str(&format!("multicast_ttl = {}\n", e.multicast_ttl));
    }
    if let Some(ms) = e.reconnect_delay_ms {
        out.push_str(&format!("reconnect_delay_ms = {ms}\n"));
    }
}

/// Serialise a [`RelayConfig`] to a TOML string suitable for writing to disk.
pub fn to_toml_string(cfg: &RelayConfig) -> String {
    let mut out = String::new();

    // [general]
    out.push_str("[general]\n");
    out.push_str(&format!(
        "log_level = {}\n",
        toml_str(&cfg.general.log_level)
    ));
    out.push_str(&format!(
        "stats_interval_secs = {}\n",
        cfg.general.stats_interval_secs
    ));
    out.push_str(&format!(
        "channel_capacity = {}\n",
        cfg.general.channel_capacity
    ));
    out.push_str(&format!(
        "max_payload_size = {}\n",
        cfg.general.max_payload_size
    ));
    if let Some(port) = cfg.general.health_port {
        out.push_str(&format!("health_port = {port}\n"));
    }
    if let Some(ref addr) = cfg.general.health_bind_addr {
        out.push_str(&format!("health_bind_addr = {}\n", toml_str(addr)));
    }
    out.push('\n');

    // [source]
    out.push_str("[source]\n");
    write_endpoint_fields(&mut out, &cfg.source);
    out.push('\n');

    // [rate_limit] (optional)
    if let Some(ref rl) = cfg.rate_limit {
        out.push_str("[rate_limit]\n");
        out.push_str(&format!("bytes_per_second = {}\n", rl.bytes_per_second));
        out.push_str(&format!("burst_size = {}\n", rl.burst_size));
        out.push('\n');
    }

    // [[destinations]]
    for dest in &cfg.destinations {
        out.push_str("[[destinations]]\n");
        write_endpoint_fields(&mut out, &dest.base);
        if dest.overflow_policy != OverflowPolicy::DropNewest {
            out.push_str("overflow_policy = \"block\"\n");
        }
        if let Some(ref rl) = dest.rate_limit {
            // Inline table keeps the dest grouped in array-of-tables.
            out.push_str(&format!(
                "rate_limit = {{ bytes_per_second = {}, burst_size = {} }}\n",
                rl.bytes_per_second, rl.burst_size
            ));
        }
        out.push('\n');
    }

    out
}

mod defaults {
    pub fn log_level() -> String {
        "info".into()
    }
    pub fn stats_interval() -> u64 {
        30
    }
    pub fn channel_capacity() -> usize {
        1024
    }
    pub fn max_payload() -> usize {
        65535
    }
    pub fn multicast_ttl() -> u32 {
        16
    }
    pub fn burst() -> u64 {
        131_072
    }
}

// ── Unit tests ─────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn minimal_cfg(src_proto: &str, dst_proto: &str) -> String {
        format!(
            r#"
[general]
[source]
protocol = "{src_proto}"
mode     = "server"
address  = "127.0.0.1:10000"
[[destinations]]
protocol = "{dst_proto}"
mode     = "client"
address  = "127.0.0.1:20000"
"#
        )
    }

    #[test]
    fn parse_minimal_tcp_tcp() {
        let cfg: RelayConfig = toml::from_str(&minimal_cfg("tcp", "tcp")).unwrap();
        cfg.validate().unwrap();
        assert_eq!(cfg.destinations.len(), 1);
    }

    #[test]
    fn parse_cross_protocol() {
        let cfg: RelayConfig = toml::from_str(&minimal_cfg("tcp", "udp")).unwrap();
        cfg.validate().unwrap();
    }

    #[test]
    fn reject_empty_destinations() {
        let raw = r#"
[general]
[source]
protocol = "tcp"
mode     = "server"
address  = "127.0.0.1:10000"
"#;
        let result: std::result::Result<RelayConfig, _> = toml::from_str(raw);
        assert!(result.is_err());
    }

    #[test]
    fn reject_tcp_multicast_destination() {
        let raw = r#"
[general]
[source]
protocol = "tcp"
mode     = "server"
address  = "127.0.0.1:10000"
[[destinations]]
protocol  = "tcp"
mode      = "client"
address   = "127.0.0.1:20000"
cast_mode = "multicast"
"#;
        let cfg: RelayConfig = toml::from_str(raw).unwrap();
        assert!(cfg.validate().is_err());
    }

    #[test]
    fn reject_tcp_multicast_source() {
        let raw = r#"
[general]
[source]
protocol  = "tcp"
mode      = "server"
address   = "127.0.0.1:10000"
cast_mode = "multicast"
[[destinations]]
protocol = "udp"
mode     = "client"
address  = "127.0.0.1:20000"
"#;
        let cfg: RelayConfig = toml::from_str(raw).unwrap();
        assert!(cfg.validate().is_err());
    }

    #[test]
    fn reject_zero_rate() {
        let raw = r#"
[general]
[source]
protocol = "tcp"
mode     = "server"
address  = "127.0.0.1:10000"
[rate_limit]
bytes_per_second = 0
[[destinations]]
protocol = "tcp"
mode     = "client"
address  = "127.0.0.1:20000"
"#;
        let cfg: RelayConfig = toml::from_str(raw).unwrap();
        assert!(cfg.validate().is_err());
    }

    #[test]
    fn default_values_applied() {
        let cfg: RelayConfig = toml::from_str(&minimal_cfg("udp", "udp")).unwrap();
        assert_eq!(cfg.general.max_payload_size, 65535);
        assert_eq!(cfg.general.channel_capacity, 1024);
        assert_eq!(cfg.general.stats_interval_secs, 30);
    }

    #[test]
    fn roundtrip_toml() {
        let cfg: RelayConfig = toml::from_str(&minimal_cfg("tcp", "udp")).unwrap();
        let toml_str = to_toml_string(&cfg);
        let cfg2: RelayConfig = toml::from_str(&toml_str).unwrap();
        assert_eq!(cfg, cfg2);
    }

    fn default_endpoint() -> EndpointConfig {
        EndpointConfig {
            name: None,
            protocol: Protocol::Tcp,
            mode: EndpointMode::Server,
            address: "127.0.0.1:10000".into(),
            cast_mode: CastMode::Unicast,
            multicast_interface: None,
            multicast_interface_index: None,
            multicast_ttl: 2,
            reconnect_delay_ms: None,
        }
    }

    #[test]
    fn display_name_uses_name_field() {
        let e = EndpointConfig {
            name: Some("my-source".into()),
            ..default_endpoint()
        };
        assert_eq!(e.display_name(), "my-source");
    }

    #[test]
    fn display_name_falls_back_to_address() {
        let e = EndpointConfig {
            name: None,
            ..default_endpoint()
        };
        assert_eq!(e.display_name(), e.address);
    }

    #[test]
    fn reconnect_delay_default_is_2000ms() {
        let e = EndpointConfig {
            reconnect_delay_ms: None,
            ..default_endpoint()
        };
        assert_eq!(e.reconnect_delay(), std::time::Duration::from_millis(2000));
    }

    #[test]
    fn reconnect_delay_custom() {
        let e = EndpointConfig {
            reconnect_delay_ms: Some(500),
            ..default_endpoint()
        };
        assert_eq!(e.reconnect_delay(), std::time::Duration::from_millis(500));
    }

    #[test]
    fn socket_addr_parses_ipv4() {
        let e = EndpointConfig {
            address: "127.0.0.1:8080".into(),
            ..default_endpoint()
        };
        assert!(e.socket_addr().is_ok());
    }

    #[test]
    fn socket_addr_parses_ipv6() {
        let e = EndpointConfig {
            address: "[::1]:8080".into(),
            ..default_endpoint()
        };
        assert!(e.socket_addr().is_ok());
    }

    #[test]
    fn socket_addr_rejects_invalid() {
        let e = EndpointConfig {
            address: "not-an-address".into(),
            ..default_endpoint()
        };
        assert!(e.socket_addr().is_err());
    }

    #[test]
    fn reject_zero_max_payload() {
        let raw = r#"
[general]
max_payload_size = 0
[source]
protocol = "tcp"
mode     = "server"
address  = "127.0.0.1:10000"
[[destinations]]
protocol = "tcp"
mode     = "client"
address  = "127.0.0.1:20000"
"#;
        let cfg: RelayConfig = toml::from_str(raw).unwrap();
        assert!(cfg.validate().is_err());
    }

    #[test]
    fn reject_invalid_multicast_iface_source() {
        let raw = r#"
[general]
[source]
protocol = "udp"
mode = "server"
address = "224.0.0.1:9000"
cast_mode = "multicast"
multicast_interface = "eth0"
[[destinations]]
protocol = "udp"
mode = "client"
address = "127.0.0.1:20000"
"#;
        let cfg: RelayConfig = toml::from_str(raw).unwrap();
        assert!(cfg.validate().is_err());
    }

    #[test]
    fn reject_invalid_multicast_iface_dest() {
        let raw = r#"
[general]
[source]
protocol = "tcp"
mode = "server"
address = "127.0.0.1:10000"
[[destinations]]
protocol = "udp"
mode = "client"
address = "224.0.0.1:9000"
cast_mode = "multicast"
multicast_interface = "eth0"
"#;
        let cfg: RelayConfig = toml::from_str(raw).unwrap();
        assert!(cfg.validate().is_err());
    }

    #[test]
    fn from_file_missing_returns_error() {
        assert!(RelayConfig::from_file("/nonexistent/path/nexthop_test.toml").is_err());
    }

    #[test]
    fn toml_string_escapes_quotes_in_name() {
        let mut cfg: RelayConfig = toml::from_str(&minimal_cfg("tcp", "tcp")).unwrap();
        cfg.source.name = Some(r#"my "named" source"#.into());
        let s = to_toml_string(&cfg);
        let cfg2: RelayConfig = toml::from_str(&s).unwrap();
        assert_eq!(cfg2.source.name.unwrap(), r#"my "named" source"#);
    }

    #[test]
    fn dest_config_deref_reaches_base() {
        let cfg: RelayConfig = toml::from_str(&minimal_cfg("tcp", "tcp")).unwrap();
        let dest = &cfg.destinations[0];
        assert!(dest.socket_addr().is_ok());
    }

    // ── Additional coverage ────────────────────────────────────────────

    #[test]
    fn cast_mode_default_is_unicast() {
        assert_eq!(CastMode::default(), CastMode::Unicast);
    }

    #[test]
    fn overflow_policy_default_is_drop_newest() {
        assert_eq!(OverflowPolicy::default(), OverflowPolicy::DropNewest);
    }

    #[test]
    fn reject_tcp_broadcast_source() {
        let raw = r#"
[general]
[source]
protocol  = "tcp"
mode      = "server"
address   = "0.0.0.0:10000"
cast_mode = "broadcast"
[[destinations]]
protocol = "udp"
mode     = "client"
address  = "127.0.0.1:20000"
"#;
        let cfg: RelayConfig = toml::from_str(raw).unwrap();
        assert!(cfg.validate().is_err());
    }

    #[test]
    fn reject_tcp_broadcast_destination() {
        let raw = r#"
[general]
[source]
protocol = "tcp"
mode     = "server"
address  = "127.0.0.1:10000"
[[destinations]]
protocol  = "tcp"
mode      = "client"
address   = "127.0.0.1:20000"
cast_mode = "broadcast"
"#;
        let cfg: RelayConfig = toml::from_str(raw).unwrap();
        assert!(cfg.validate().is_err());
    }

    #[test]
    fn valid_multicast_interface_ip_passes() {
        let raw = r#"
[general]
[source]
protocol             = "udp"
mode                 = "server"
address              = "224.0.0.1:9000"
cast_mode            = "multicast"
multicast_interface  = "192.168.1.1"
[[destinations]]
protocol = "udp"
mode     = "client"
address  = "127.0.0.1:20000"
"#;
        let cfg: RelayConfig = toml::from_str(raw).unwrap();
        assert!(cfg.validate().is_ok());
    }

    #[test]
    fn valid_multicast_dest_interface_ip_passes() {
        let raw = r#"
[general]
[source]
protocol = "tcp"
mode     = "server"
address  = "127.0.0.1:10000"
[[destinations]]
protocol             = "udp"
mode                 = "client"
address              = "224.0.0.1:9000"
cast_mode            = "multicast"
multicast_interface  = "10.0.0.1"
"#;
        let cfg: RelayConfig = toml::from_str(raw).unwrap();
        assert!(cfg.validate().is_ok());
    }

    #[test]
    fn toml_string_includes_rate_limit() {
        let raw = r#"
[general]
[source]
protocol = "tcp"
mode     = "server"
address  = "127.0.0.1:10000"
[rate_limit]
bytes_per_second = 1000000
burst_size       = 131072
[[destinations]]
protocol = "tcp"
mode     = "client"
address  = "127.0.0.1:20000"
"#;
        let cfg: RelayConfig = toml::from_str(raw).unwrap();
        let out = to_toml_string(&cfg);
        assert!(
            out.contains("bytes_per_second"),
            "missing bytes_per_second:\n{out}"
        );
        assert!(out.contains("burst_size"), "missing burst_size:\n{out}");
        let cfg2: RelayConfig = toml::from_str(&out).unwrap();
        assert_eq!(cfg, cfg2);
    }

    #[test]
    fn toml_string_includes_health_port() {
        let raw = r#"
[general]
health_port = 9090
[source]
protocol = "tcp"
mode     = "server"
address  = "127.0.0.1:10000"
[[destinations]]
protocol = "tcp"
mode     = "client"
address  = "127.0.0.1:20000"
"#;
        let cfg: RelayConfig = toml::from_str(raw).unwrap();
        let out = to_toml_string(&cfg);
        assert!(out.contains("health_port"), "missing health_port:\n{out}");
        let cfg2: RelayConfig = toml::from_str(&out).unwrap();
        assert_eq!(cfg.general.health_port, cfg2.general.health_port);
    }

    #[test]
    fn toml_string_includes_multicast_fields() {
        let raw = r#"
[general]
[source]
protocol              = "udp"
mode                  = "server"
address               = "224.0.0.1:9000"
cast_mode             = "multicast"
multicast_interface   = "192.168.1.1"
multicast_ttl         = 32
[[destinations]]
protocol = "udp"
mode     = "client"
address  = "127.0.0.1:20000"
"#;
        let cfg: RelayConfig = toml::from_str(raw).unwrap();
        let out = to_toml_string(&cfg);
        let cfg2: RelayConfig = toml::from_str(&out).unwrap();
        assert_eq!(cfg.source.cast_mode, cfg2.source.cast_mode);
        assert_eq!(
            cfg.source.multicast_interface,
            cfg2.source.multicast_interface
        );
        assert_eq!(cfg.source.multicast_ttl, cfg2.source.multicast_ttl);
    }

    #[test]
    fn toml_string_includes_reconnect_delay() {
        let raw = r#"
[general]
[source]
protocol           = "tcp"
mode               = "client"
address            = "127.0.0.1:10000"
reconnect_delay_ms = 500
[[destinations]]
protocol = "tcp"
mode     = "client"
address  = "127.0.0.1:20000"
"#;
        let cfg: RelayConfig = toml::from_str(raw).unwrap();
        let out = to_toml_string(&cfg);
        assert!(out.contains("reconnect_delay_ms"), "missing field:\n{out}");
        let cfg2: RelayConfig = toml::from_str(&out).unwrap();
        assert_eq!(
            cfg.source.reconnect_delay_ms,
            cfg2.source.reconnect_delay_ms
        );
    }

    #[test]
    fn toml_string_includes_overflow_block() {
        let raw = r#"
[general]
[source]
protocol = "tcp"
mode     = "server"
address  = "127.0.0.1:10000"
[[destinations]]
protocol         = "tcp"
mode             = "client"
address          = "127.0.0.1:20000"
overflow_policy  = "block"
"#;
        let cfg: RelayConfig = toml::from_str(raw).unwrap();
        let out = to_toml_string(&cfg);
        assert!(
            out.contains("overflow_policy"),
            "missing overflow_policy:\n{out}"
        );
        let cfg2: RelayConfig = toml::from_str(&out).unwrap();
        assert_eq!(
            cfg.destinations[0].overflow_policy,
            cfg2.destinations[0].overflow_policy
        );
    }

    #[test]
    fn burst_default_is_131072() {
        // Verify defaults::burst() matches documented constant.
        assert_eq!(defaults::burst(), 131_072);
    }

    #[test]
    fn multicast_ttl_default_is_16() {
        assert_eq!(defaults::multicast_ttl(), 16);
    }

    #[test]
    fn multiple_destinations_roundtrip() {
        let raw = r#"
[general]
[source]
protocol = "udp"
mode     = "server"
address  = "0.0.0.0:5000"
[[destinations]]
protocol = "udp"
mode     = "client"
address  = "10.0.0.1:5001"
[[destinations]]
protocol = "tcp"
mode     = "client"
address  = "10.0.0.2:5002"
[[destinations]]
protocol = "udp"
mode     = "client"
address  = "10.0.0.3:5003"
"#;
        let cfg: RelayConfig = toml::from_str(raw).unwrap();
        assert_eq!(cfg.destinations.len(), 3);
        let out = to_toml_string(&cfg);
        let cfg2: RelayConfig = toml::from_str(&out).unwrap();
        assert_eq!(cfg, cfg2);
    }

    #[test]
    fn dest_config_display_name_via_deref() {
        let raw = r#"
[general]
[source]
protocol = "tcp"
mode     = "server"
address  = "127.0.0.1:10000"
[[destinations]]
name     = "my-dest"
protocol = "tcp"
mode     = "client"
address  = "127.0.0.1:20000"
"#;
        let cfg: RelayConfig = toml::from_str(raw).unwrap();
        assert_eq!(cfg.destinations[0].display_name(), "my-dest");
    }

    #[test]
    fn dest_config_reconnect_delay_via_deref() {
        let raw = r#"
[general]
[source]
protocol = "tcp"
mode     = "server"
address  = "127.0.0.1:10000"
[[destinations]]
protocol           = "tcp"
mode               = "client"
address            = "127.0.0.1:20000"
reconnect_delay_ms = 750
"#;
        let cfg: RelayConfig = toml::from_str(raw).unwrap();
        assert_eq!(
            cfg.destinations[0].reconnect_delay(),
            std::time::Duration::from_millis(750)
        );
    }

    #[test]
    fn serde_protocol_lowercase() {
        let json = serde_json::to_string(&Protocol::Tcp).unwrap();
        assert_eq!(json, r#""tcp""#);
        let json = serde_json::to_string(&Protocol::Udp).unwrap();
        assert_eq!(json, r#""udp""#);
    }

    #[test]
    fn serde_endpoint_mode_lowercase() {
        assert_eq!(
            serde_json::to_string(&EndpointMode::Server).unwrap(),
            r#""server""#
        );
        assert_eq!(
            serde_json::to_string(&EndpointMode::Client).unwrap(),
            r#""client""#
        );
    }

    #[test]
    fn serde_cast_mode_lowercase() {
        assert_eq!(
            serde_json::to_string(&CastMode::Unicast).unwrap(),
            r#""unicast""#
        );
        assert_eq!(
            serde_json::to_string(&CastMode::Broadcast).unwrap(),
            r#""broadcast""#
        );
        assert_eq!(
            serde_json::to_string(&CastMode::Multicast).unwrap(),
            r#""multicast""#
        );
    }

    #[test]
    fn serde_overflow_policy_snake_case() {
        assert_eq!(
            serde_json::to_string(&OverflowPolicy::DropNewest).unwrap(),
            r#""drop_newest""#
        );
        assert_eq!(
            serde_json::to_string(&OverflowPolicy::Block).unwrap(),
            r#""block""#
        );
    }

    #[test]
    fn toml_str_helper_escapes_backslash_and_newline() {
        // Use to_toml_string to exercise escaping through a real round-trip.
        let mut cfg: RelayConfig = toml::from_str(&minimal_cfg("tcp", "tcp")).unwrap();
        cfg.source.name = Some("path\\to\nsomething".into());
        let s = to_toml_string(&cfg);
        let cfg2: RelayConfig = toml::from_str(&s).unwrap();
        assert_eq!(cfg2.source.name.unwrap(), "path\\to\nsomething");
    }

    #[test]
    fn omitting_rate_limit_produces_no_section() {
        let cfg: RelayConfig = toml::from_str(&minimal_cfg("tcp", "tcp")).unwrap();
        let out = to_toml_string(&cfg);
        assert!(!out.contains("[rate_limit]"), "unexpected section:\n{out}");
    }

    #[test]
    fn cast_mode_unicast_omitted_from_toml() {
        // unicast is default, should not appear in output
        let cfg: RelayConfig = toml::from_str(&minimal_cfg("udp", "udp")).unwrap();
        let out = to_toml_string(&cfg);
        assert!(!out.contains("cast_mode"), "unexpected field:\n{out}");
    }

    #[test]
    fn multicast_ttl_default_omitted_from_toml() {
        // default TTL (16) should be omitted (equality check in write_endpoint_fields)
        let raw = r#"
[general]
[source]
protocol   = "udp"
mode       = "server"
address    = "224.0.0.1:9000"
cast_mode  = "multicast"
[[destinations]]
protocol = "udp"
mode     = "client"
address  = "127.0.0.1:20000"
"#;
        let cfg: RelayConfig = toml::from_str(raw).unwrap();
        assert_eq!(cfg.source.multicast_ttl, 16);
        let out = to_toml_string(&cfg);
        assert!(
            !out.contains("multicast_ttl"),
            "default TTL should be omitted:\n{out}"
        );
    }

    // ── Property tests ─────────────────────────────────────────────
    //
    // The tabular tests above pin specific shapes; the strategies below
    // generate configs across the *valid* input domain and assert two
    // load-bearing invariants — round-trip stability of TOML
    // serialization, and parser robustness against arbitrary input.

    use proptest::prelude::*;

    fn log_level_strategy() -> impl Strategy<Value = String> {
        prop::sample::select(vec!["trace", "debug", "info", "warn", "error"])
            .prop_map(|s| s.to_string())
    }

    fn bind_addr_strategy() -> impl Strategy<Value = Option<String>> {
        prop::option::of(
            prop::sample::select(vec!["127.0.0.1", "0.0.0.0", "::1", "::"])
                .prop_map(|s| s.to_string()),
        )
    }

    fn general_strategy() -> impl Strategy<Value = GeneralConfig> {
        (
            log_level_strategy(),
            1u64..3600,
            1usize..2048,
            1usize..65_536,
            prop::option::of(1024u16..65535),
            bind_addr_strategy(),
        )
            .prop_map(
                |(
                    log_level,
                    stats_interval_secs,
                    channel_capacity,
                    max_payload_size,
                    health_port,
                    health_bind_addr,
                )| GeneralConfig {
                    log_level,
                    stats_interval_secs,
                    channel_capacity,
                    max_payload_size,
                    health_port,
                    health_bind_addr,
                },
            )
    }

    fn rate_limit_strategy() -> impl Strategy<Value = RateLimitConfig> {
        (1u64..1_000_000, 1u64..1_000_000).prop_map(|(bps, burst)| RateLimitConfig {
            bytes_per_second: bps,
            burst_size: burst,
        })
    }

    // Generate addresses we know the validator accepts: loopback IPv4 with
    // a non-zero port. Multicast / broadcast paths have a richer validation
    // surface and would need their own dedicated strategies to stay
    // round-trip-safe; the round-trip property here covers the common case.
    fn address_strategy() -> impl Strategy<Value = String> {
        (1u16..=u16::MAX).prop_map(|port| format!("127.0.0.1:{port}"))
    }

    fn name_strategy() -> impl Strategy<Value = Option<String>> {
        // ASCII letters/digits + a hyphen, so we never trip the TOML
        // string-escape edge cases that the manual serializer handles
        // (the round-trip property tests the parser/serializer pair, not
        // the escape implementation, which has dedicated tabular tests).
        prop::option::of("[A-Za-z0-9-]{1,16}".prop_map(|s| s.to_string()))
    }

    fn unicast_endpoint_strategy() -> impl Strategy<Value = EndpointConfig> {
        (
            name_strategy(),
            prop::sample::select(vec![Protocol::Tcp, Protocol::Udp]),
            prop::sample::select(vec![EndpointMode::Server, EndpointMode::Client]),
            address_strategy(),
            prop::option::of(100u64..10_000),
        )
            .prop_map(|(name, protocol, mode, address, reconnect_delay_ms)| {
                EndpointConfig {
                    name,
                    protocol,
                    mode,
                    address,
                    cast_mode: CastMode::Unicast,
                    multicast_interface: None,
                    multicast_interface_index: None,
                    multicast_ttl: defaults::multicast_ttl(),
                    reconnect_delay_ms,
                }
            })
    }

    fn dest_strategy() -> impl Strategy<Value = DestConfig> {
        (
            unicast_endpoint_strategy(),
            prop::sample::select(vec![OverflowPolicy::DropNewest, OverflowPolicy::Block]),
            prop::option::of(rate_limit_strategy()),
        )
            .prop_map(|(base, overflow_policy, rate_limit)| DestConfig {
                base,
                overflow_policy,
                rate_limit,
            })
    }

    fn relay_config_strategy() -> impl Strategy<Value = RelayConfig> {
        (
            general_strategy(),
            unicast_endpoint_strategy(),
            prop::option::of(rate_limit_strategy()),
            prop::collection::vec(dest_strategy(), 1..4),
        )
            .prop_map(|(general, source, rate_limit, destinations)| RelayConfig {
                general,
                source,
                rate_limit,
                destinations,
            })
    }

    proptest! {
        // Modest case count: the generators are cheap but each round-trip
        // does two TOML conversions, so 64 keeps the suite snappy.
        #![proptest_config(ProptestConfig { cases: 64, .. ProptestConfig::default() })]

        /// Any config produced by our strategy must survive
        /// `to_toml_string → toml::from_str` unchanged. This guards
        /// against drift between the hand-rolled emitter (`to_toml_string`)
        /// and the serde-derived parser.
        #[test]
        fn prop_valid_config_roundtrips_through_toml(cfg in relay_config_strategy()) {
            // Pre-condition: the strategy only emits configs the validator
            // accepts. Bail out cleanly if a future widening violates that
            // so the property failure points at the real issue.
            prop_assume!(cfg.validate().is_ok());
            let serialized = to_toml_string(&cfg);
            let parsed: RelayConfig = toml::from_str(&serialized)
                .map_err(|e| TestCaseError::fail(format!("re-parse failed: {e}\n--- TOML ---\n{serialized}")))?;
            prop_assert_eq!(cfg, parsed);
        }

        /// Arbitrary UTF-8 strings must never panic the parser. They will
        /// usually fail to parse — that's fine — but the failure must
        /// always come back as an `Err`, never an unwrap or arithmetic
        /// crash.
        #[test]
        fn prop_arbitrary_toml_does_not_panic(s in "\\PC{0,512}") {
            // Result intentionally discarded; the test passes if no
            // panic propagates out.
            let _ = toml::from_str::<RelayConfig>(&s);
        }

        /// `validate()` must reject every config whose
        /// `rate_limit.bytes_per_second` is zero, regardless of what
        /// the rest of the config looks like.
        #[test]
        fn prop_zero_global_rate_is_rejected(mut cfg in relay_config_strategy()) {
            cfg.rate_limit = Some(RateLimitConfig {
                bytes_per_second: 0,
                burst_size: 1,
            });
            prop_assert!(cfg.validate().is_err());
        }

        /// Same for per-destination rate limit: a zero rate on any
        /// destination must surface as a validation error.
        #[test]
        fn prop_zero_per_dest_rate_is_rejected(
            mut cfg in relay_config_strategy(),
            idx in 0usize..16,
        ) {
            let i = idx % cfg.destinations.len();
            cfg.destinations[i].rate_limit = Some(RateLimitConfig {
                bytes_per_second: 0,
                burst_size: 1,
            });
            prop_assert!(cfg.validate().is_err());
        }
    }
}
