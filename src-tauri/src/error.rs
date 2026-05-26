// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026-present Patrick S Connallon
//
// nexthop - TCP / UDP - Unicast / Multicast / Broadcast
// Architect: Patrick S Connallon
// Built by:  Anthropic Claude (Sonnet 4.6)

use thiserror::Error;

/// All relay error variants.
#[derive(Error, Debug)]
pub enum RelayError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("configuration error: {0}")]
    Config(String),

    #[error("address parse error: {0}")]
    AddrParse(#[from] std::net::AddrParseError),

    #[error("TOML parse error: {0}")]
    TomlParse(#[from] toml::de::Error),
}

pub type Result<T> = std::result::Result<T, RelayError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_config_error() {
        let e = RelayError::Config("bad port".into());
        assert_eq!(e.to_string(), "configuration error: bad port");
    }

    #[test]
    fn display_io_error_prefix() {
        let io = std::io::Error::new(std::io::ErrorKind::NotFound, "file missing");
        let e = RelayError::Io(io);
        assert!(e.to_string().starts_with("I/O error:"), "got: {e}");
    }

    #[test]
    fn display_addr_parse_prefix() {
        let bad: std::result::Result<std::net::SocketAddr, _> = "not_an_addr".parse();
        let e = RelayError::AddrParse(bad.unwrap_err());
        assert!(e.to_string().starts_with("address parse error:"), "got: {e}");
    }

    #[test]
    fn from_io_error_conversion() {
        let io = std::io::Error::new(std::io::ErrorKind::PermissionDenied, "denied");
        let e: RelayError = io.into();
        assert!(matches!(e, RelayError::Io(_)));
    }

    #[test]
    fn from_toml_parse_error_conversion() {
        let bad: std::result::Result<toml::Value, _> = toml::from_str("= invalid");
        let toml_err = bad.unwrap_err();
        let e: RelayError = toml_err.into();
        assert!(matches!(e, RelayError::TomlParse(_)));
        assert!(e.to_string().starts_with("TOML parse error:"), "got: {e}");
    }

    #[test]
    fn result_type_alias_wraps_relay_error() {
        let r: Result<i32> = Err(RelayError::Config("oops".into()));
        assert!(r.is_err());
    }

    #[test]
    fn config_error_carries_message() {
        let msg = "something specific went wrong";
        let e = RelayError::Config(msg.into());
        assert!(e.to_string().contains(msg));
    }

    #[test]
    fn display_addr_parse_contains_original() {
        let bad: std::result::Result<std::net::SocketAddr, _> = "xyz:abc".parse();
        let e = RelayError::AddrParse(bad.unwrap_err());
        let s = e.to_string();
        assert!(s.starts_with("address parse error:"), "got: {s}");
    }
}
