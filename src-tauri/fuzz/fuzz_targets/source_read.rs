// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026-present Patrick S Connallon

//! Fuzzes the framing + oversize-drop arithmetic of the UDP source's
//! read loop.
//!
//! The real loop is async-on-tokio and does a `recv_from` for each
//! datagram; here we replay an equivalent pure-byte-stream view of it.
//! `data` is interpreted as a sequence of `(u16-le length, bytes)`
//! frames, mirroring the kernel-handed-us-N-bytes-per-`recv_from`
//! contract. The harness must:
//!
//! 1. Drop oversize frames (length > `MAX_PAYLOAD`) and count them.
//! 2. Pass undersize frames through `Bytes::copy_from_slice`, which
//!    is the real loop's only allocation.
//! 3. Stop cleanly at EOF (no partial-frame panic).
//!
//! Invariants asserted at the end:
//! - Frame count is bounded by `data.len() / 2` (each frame has a
//!   2-byte header).
//! - Accepted-bytes-sum never exceeds `data.len()`.

#![no_main]

use libfuzzer_sys::fuzz_target;

const MAX_PAYLOAD: usize = 1024;

fuzz_target!(|data: &[u8]| {
    let mut received_bytes: u64 = 0;
    let mut received_frames: u64 = 0;
    let mut dropped_oversize: u64 = 0;
    let mut pos = 0;

    while pos + 2 <= data.len() {
        let claimed_len = u16::from_le_bytes([data[pos], data[pos + 1]]) as usize;
        pos += 2;

        // A short tail (claimed_len > remaining) is the real loop's
        // "kernel truncated this datagram" case — clamp and continue,
        // never index past the end.
        let avail = data.len() - pos;
        let actual_len = claimed_len.min(avail);

        if actual_len > MAX_PAYLOAD {
            dropped_oversize = dropped_oversize.saturating_add(1);
        } else {
            received_bytes = received_bytes.saturating_add(actual_len as u64);
            received_frames = received_frames.saturating_add(1);
            // The relay does exactly this on each accepted datagram.
            let _ = bytes::Bytes::copy_from_slice(&data[pos..pos + actual_len]);
        }
        pos += actual_len;
    }

    // Real invariants on the accounting: the total number of frames we
    // touched cannot exceed the number of header slots that fit in the
    // input, and accepted bytes cannot exceed input length.
    let max_frames = (data.len() / 2) as u64;
    assert!(received_frames.saturating_add(dropped_oversize) <= max_frames);
    assert!(received_bytes <= data.len() as u64);
});
