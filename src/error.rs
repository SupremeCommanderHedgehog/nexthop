// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026-present nexthop@krypte.me
//
// nexthop - TCP / UDP - Unicast / Multicast / Broadcast
// Architect: nexthop@krypte.me
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
}
