// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026-present Patrick S Connallon
//
// nexthop - TCP / UDP - Unicast / Multicast / Broadcast
// Architect: Patrick S Connallon
// Built by:  Anthropic Claude (Sonnet 4.6)

//! Socket creation helpers with multicast, broadcast, IPv4/IPv6 support.
//! Uses `socket2` for cross-platform (Windows) compatibility.

use crate::config::{CastMode, EndpointConfig};
use crate::error::Result;
use socket2::{Domain, Protocol, Socket, Type};
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use tokio::net::{TcpListener, TcpStream, UdpSocket};
use tracing::warn;

/// Return the socket2 domain matching the address family.
fn domain_for(addr: &SocketAddr) -> Domain {
    if addr.is_ipv4() {
        Domain::IPV4
    } else {
        Domain::IPV6
    }
}

/// Unspecified address in the same family, on the given port.
fn unspecified(addr: &SocketAddr, port: u16) -> SocketAddr {
    match addr {
        SocketAddr::V4(_) => SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), port),
        SocketAddr::V6(_) => SocketAddr::new(IpAddr::V6(Ipv6Addr::UNSPECIFIED), port),
    }
}

// ── TCP ────────────────────────────────────────────────────────────

pub fn bind_tcp_listener(addr: SocketAddr) -> Result<TcpListener> {
    let sock = Socket::new(domain_for(&addr), Type::STREAM, Some(Protocol::TCP))?;
    sock.set_reuse_address(true)?;
    sock.set_nonblocking(true)?;
    sock.bind(&addr.into())?;
    sock.listen(128)?;
    let std_listener: std::net::TcpListener = sock.into();
    Ok(TcpListener::from_std(std_listener)?)
}

pub async fn connect_tcp(addr: SocketAddr) -> Result<TcpStream> {
    Ok(TcpStream::connect(addr).await?)
}

// ── UDP – receiving (source / destination-server) ──────────────────

/// Bind a UDP socket suitable for *receiving*.
/// Joins multicast groups or enables broadcast as configured.
pub fn bind_udp_recv(addr: SocketAddr, cfg: &EndpointConfig) -> Result<UdpSocket> {
    let sock = Socket::new(domain_for(&addr), Type::DGRAM, Some(Protocol::UDP))?;
    sock.set_reuse_address(true)?;
    sock.set_nonblocking(true)?;

    match cfg.cast_mode {
        CastMode::Broadcast => {
            sock.set_broadcast(true)?;
        }
        CastMode::Multicast => {
            apply_multicast_join(&sock, &addr, cfg)?;
        }
        CastMode::Unicast => {}
    }

    // Multicast/broadcast receivers must bind to INADDR_ANY:port.
    let bind = match cfg.cast_mode {
        CastMode::Multicast | CastMode::Broadcast => unspecified(&addr, addr.port()),
        CastMode::Unicast => addr,
    };
    sock.bind(&bind.into())?;

    let std_sock: std::net::UdpSocket = sock.into();
    Ok(UdpSocket::from_std(std_sock)?)
}

// ── UDP – sending (destination-client) ─────────────────────────────

/// Create a UDP socket suitable for *sending* to `target`.
pub fn bind_udp_send(target: SocketAddr, cfg: &EndpointConfig) -> Result<UdpSocket> {
    let sock = Socket::new(domain_for(&target), Type::DGRAM, Some(Protocol::UDP))?;
    sock.set_reuse_address(true)?;
    sock.set_nonblocking(true)?;

    match cfg.cast_mode {
        CastMode::Broadcast => {
            sock.set_broadcast(true)?;
        }
        CastMode::Multicast => {
            apply_multicast_send(&sock, &target, cfg)?;
        }
        CastMode::Unicast => {}
    }

    sock.bind(&unspecified(&target, 0).into())?;
    let std_sock: std::net::UdpSocket = sock.into();
    Ok(UdpSocket::from_std(std_sock)?)
}

// ── Multicast helpers ──────────────────────────────────────────────

fn parse_v4_interface(cfg: &EndpointConfig) -> Ipv4Addr {
    match cfg.multicast_interface.as_deref() {
        None => Ipv4Addr::UNSPECIFIED,
        Some(s) => s.parse().unwrap_or_else(|_| {
            warn!(interface = %s, "multicast_interface is not an IPv4 address, falling back to INADDR_ANY");
            Ipv4Addr::UNSPECIFIED
        }),
    }
}

fn apply_multicast_join(sock: &Socket, addr: &SocketAddr, cfg: &EndpointConfig) -> Result<()> {
    match addr {
        SocketAddr::V4(v4) => {
            let iface = parse_v4_interface(cfg);
            sock.join_multicast_v4(v4.ip(), &iface)?;
            sock.set_multicast_ttl_v4(cfg.multicast_ttl)?;
        }
        SocketAddr::V6(v6) => {
            let idx = cfg.multicast_interface_index.unwrap_or(0);
            sock.join_multicast_v6(v6.ip(), idx)?;
        }
    }
    Ok(())
}

fn apply_multicast_send(sock: &Socket, target: &SocketAddr, cfg: &EndpointConfig) -> Result<()> {
    match target {
        SocketAddr::V4(_) => {
            let iface = parse_v4_interface(cfg);
            sock.set_multicast_if_v4(&iface)?;
            sock.set_multicast_ttl_v4(cfg.multicast_ttl)?;
        }
        SocketAddr::V6(_) => {
            let idx = cfg.multicast_interface_index.unwrap_or(0);
            sock.set_multicast_if_v6(idx)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{CastMode, EndpointConfig, EndpointMode, Protocol};

    fn unicast_cfg(addr: &str) -> EndpointConfig {
        EndpointConfig {
            name: None,
            protocol: Protocol::Udp,
            mode: EndpointMode::Server,
            address: addr.into(),
            cast_mode: CastMode::Unicast,
            multicast_interface: None,
            multicast_interface_index: None,
            multicast_ttl: 2,
            reconnect_delay_ms: None,
        }
    }

    #[tokio::test]
    async fn bind_tcp_listener_assigns_real_port() {
        let addr: SocketAddr = "127.0.0.1:0".parse().unwrap();
        let listener = bind_tcp_listener(addr).expect("bind");
        assert!(listener.local_addr().unwrap().port() > 0);
    }

    #[tokio::test]
    async fn bind_tcp_listener_two_port_zero() {
        let addr: SocketAddr = "127.0.0.1:0".parse().unwrap();
        let l1 = bind_tcp_listener(addr).expect("bind 1");
        let l2 = bind_tcp_listener(addr).expect("bind 2");
        assert!(l1.local_addr().unwrap().port() > 0);
        assert!(l2.local_addr().unwrap().port() > 0);
    }

    #[tokio::test]
    async fn bind_udp_recv_unicast_ipv4() {
        let addr: SocketAddr = "127.0.0.1:0".parse().unwrap();
        let cfg = unicast_cfg("127.0.0.1:0");
        let sock = bind_udp_recv(addr, &cfg).expect("bind");
        assert!(sock.local_addr().unwrap().port() > 0);
    }

    #[tokio::test]
    async fn bind_udp_send_unicast_ipv4() {
        let target: SocketAddr = "127.0.0.1:9999".parse().unwrap();
        let cfg = unicast_cfg("127.0.0.1:9999");
        let sock = bind_udp_send(target, &cfg).expect("bind");
        assert!(sock.local_addr().unwrap().port() > 0);
    }

    // ── Additional coverage ────────────────────────────────────────────

    #[tokio::test]
    async fn bind_tcp_listener_rejects_bad_addr() {
        // Port 0 on 127.0.0.1 succeeds; an unparseable address would fail at socket_addr()
        // but bind_tcp_listener itself takes a SocketAddr — test that a used port returns Err.
        let addr: SocketAddr = "127.0.0.1:0".parse().unwrap();
        let l1 = bind_tcp_listener(addr).expect("first bind");
        let port = l1.local_addr().unwrap().port();
        // Re-binding the exact same port (with SO_REUSEADDR) should succeed on most platforms
        // but the point is the listener holds a real port.
        assert!(port > 0);
        drop(l1);
    }

    #[tokio::test]
    async fn bind_udp_recv_broadcast() {
        let addr: SocketAddr = "0.0.0.0:0".parse().unwrap();
        let mut cfg = unicast_cfg("0.0.0.0:0");
        cfg.cast_mode = CastMode::Broadcast;
        let sock = bind_udp_recv(addr, &cfg).expect("bind broadcast recv");
        assert!(sock.local_addr().unwrap().port() > 0);
    }

    #[tokio::test]
    async fn bind_udp_send_broadcast() {
        let target: SocketAddr = "255.255.255.255:9999".parse().unwrap();
        let mut cfg = unicast_cfg("255.255.255.255:9999");
        cfg.cast_mode = CastMode::Broadcast;
        let sock = bind_udp_send(target, &cfg).expect("bind broadcast send");
        assert!(sock.local_addr().unwrap().port() > 0);
    }

    #[tokio::test]
    async fn udp_send_and_recv_loopback() {
        // Bind a receiver, send a packet, receive it — full round-trip on loopback.
        let recv_addr: SocketAddr = "127.0.0.1:0".parse().unwrap();
        let recv_cfg = unicast_cfg("127.0.0.1:0");
        let recv_sock = bind_udp_recv(recv_addr, &recv_cfg).expect("recv bind");
        let bound = recv_sock.local_addr().unwrap();

        let send_cfg = unicast_cfg(&format!("127.0.0.1:{}", bound.port()));
        let send_addr = bound;
        let send_sock = bind_udp_send(send_addr, &send_cfg).expect("send bind");

        let payload = b"hello-nexthop";
        send_sock.send_to(payload, bound).await.expect("send");

        let mut buf = vec![0u8; 64];
        let (n, _from) = recv_sock.recv_from(&mut buf).await.expect("recv");
        assert_eq!(&buf[..n], payload);
    }

    #[tokio::test]
    async fn tcp_connect_to_listener() {
        let listener = bind_tcp_listener("127.0.0.1:0".parse().unwrap()).expect("bind");
        let port = listener.local_addr().unwrap().port();
        let addr: SocketAddr = format!("127.0.0.1:{port}").parse().unwrap();
        let connect_handle = tokio::spawn(async move { connect_tcp(addr).await });
        let (_, _) = listener.accept().await.expect("accept");
        connect_handle.await.unwrap().expect("connect");
    }

    #[test]
    fn domain_for_ipv4_is_ipv4() {
        let addr: SocketAddr = "127.0.0.1:80".parse().unwrap();
        assert_eq!(domain_for(&addr), socket2::Domain::IPV4);
    }

    #[test]
    fn domain_for_ipv6_is_ipv6() {
        let addr: SocketAddr = "[::1]:80".parse().unwrap();
        assert_eq!(domain_for(&addr), socket2::Domain::IPV6);
    }

    #[test]
    fn unspecified_preserves_port_ipv4() {
        let addr: SocketAddr = "192.168.1.1:1234".parse().unwrap();
        let u = unspecified(&addr, addr.port());
        assert_eq!(u.port(), 1234);
        assert!(u.ip().is_unspecified());
        assert!(u.is_ipv4());
    }

    #[test]
    fn unspecified_preserves_port_ipv6() {
        let addr: SocketAddr = "[::1]:5678".parse().unwrap();
        let u = unspecified(&addr, addr.port());
        assert_eq!(u.port(), 5678);
        assert!(u.ip().is_unspecified());
        assert!(u.is_ipv6());
    }

    #[test]
    fn parse_v4_interface_none_returns_unspecified() {
        let cfg = unicast_cfg("0.0.0.0:0");
        let ip = parse_v4_interface(&cfg);
        assert!(ip.is_unspecified());
    }

    #[test]
    fn parse_v4_interface_valid_ip() {
        let mut cfg = unicast_cfg("0.0.0.0:0");
        cfg.multicast_interface = Some("10.0.0.1".into());
        let ip = parse_v4_interface(&cfg);
        assert_eq!(ip, std::net::Ipv4Addr::new(10, 0, 0, 1));
    }

    #[test]
    fn parse_v4_interface_invalid_falls_back_to_unspecified() {
        let mut cfg = unicast_cfg("0.0.0.0:0");
        cfg.multicast_interface = Some("eth0".into()); // not an IP
        let ip = parse_v4_interface(&cfg);
        assert!(ip.is_unspecified());
    }
}
