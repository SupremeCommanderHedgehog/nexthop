// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026-present Patrick S Connallon

//! End-to-end check for the per-destination transform pipeline
//! introduced in ADR 0002 / #79.
//!
//! Topology: UDP source → two UDP destinations. The first carries a
//! `drop_smaller_than = 8` transform; the second has no transforms.
//! We send a 4-byte payload and a 16-byte payload through the source
//! and assert that the filtered destination only sees the 16-byte one
//! while the unfiltered destination sees both.

mod common;

use common::*;
use std::net::UdpSocket;
use std::time::Duration;

#[test]
fn drop_smaller_than_filters_only_the_destination_that_configured_it() {
    let source_port = ephemeral_udp_port();
    let filtered_port = ephemeral_udp_port();
    let unfiltered_port = ephemeral_udp_port();

    let filtered_sock =
        UdpSocket::bind(format!("127.0.0.1:{filtered_port}")).expect("bind filtered dest");
    filtered_sock
        .set_read_timeout(Some(Duration::from_secs(1)))
        .unwrap();

    let unfiltered_sock =
        UdpSocket::bind(format!("127.0.0.1:{unfiltered_port}")).expect("bind unfiltered dest");
    unfiltered_sock
        .set_read_timeout(Some(Duration::from_secs(2)))
        .unwrap();

    let config_path = temp_config_path("transforms-pipeline");
    write_config(
        &config_path,
        &ConfigSpec::new(
            EndpointSpec::udp_server(source_port),
            vec![
                DestSpec::from_endpoint(EndpointSpec::udp_client(filtered_port))
                    .with_drop_smaller_than(8),
                DestSpec::from_endpoint(EndpointSpec::udp_client(unfiltered_port)),
            ],
        ),
    );

    let guard = ChildGuard {
        child: Some(spawn_relay(&config_path)),
        config: config_path.clone(),
    };

    std::thread::sleep(STARTUP_DELAY);

    let sender = UdpSocket::bind("127.0.0.1:0").expect("bind sender");

    let small = [0xAAu8; 4];
    sender
        .send_to(&small, format!("127.0.0.1:{source_port}"))
        .expect("send small");

    udp_expect(&unfiltered_sock, &small, "unfiltered (small)");
    assert!(
        udp_try_recv(&filtered_sock, "filtered (small)").is_none(),
        "filtered destination should drop the 4-byte payload"
    );

    let big = [0xBBu8; 16];
    sender
        .send_to(&big, format!("127.0.0.1:{source_port}"))
        .expect("send big");

    udp_expect(&unfiltered_sock, &big, "unfiltered (big)");
    udp_expect(&filtered_sock, &big, "filtered (big)");

    drop(guard);
}
