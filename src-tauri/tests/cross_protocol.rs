// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026-present Patrick S Connallon

//! Scenario 1 of #24: TCP source → UDP destination, bytes flow through.
//!
//! The cross-protocol path is the headline feature of nexthop — TCP
//! and UDP can be on either side of the source / destination split.
//! Unit tests cover the fan-out and encoding layers; this exercises
//! the real bind + accept + connect + write + recv path end-to-end.

mod common;

use common::*;
use std::io::Write as _;
use std::net::{TcpStream, UdpSocket};
use std::time::Duration;

#[test]
fn tcp_source_to_udp_destination_delivers_bytes() {
    let source_port = ephemeral_tcp_port();
    let dest_port = ephemeral_udp_port();

    let dest_socket = UdpSocket::bind(format!("127.0.0.1:{dest_port}")).expect("bind dest");
    dest_socket
        .set_read_timeout(Some(Duration::from_secs(3)))
        .unwrap();

    let config_path = temp_config_path("cross-proto");
    write_config(
        &config_path,
        &ConfigSpec::new(
            EndpointSpec::tcp_server(source_port),
            vec![DestSpec::from_endpoint(EndpointSpec::udp_client(dest_port))],
        ),
    );

    let guard = ChildGuard {
        child: Some(spawn_relay(&config_path)),
        config: config_path.clone(),
    };

    // Let the relay bind its TCP listener.
    std::thread::sleep(STARTUP_DELAY);

    let mut tcp = TcpStream::connect(format!("127.0.0.1:{source_port}")).expect("connect source");
    let payload = b"cross-protocol-payload";
    tcp.write_all(payload).expect("write payload");
    tcp.flush().expect("flush");

    udp_expect(&dest_socket, payload, "dest");

    drop(guard);
}
