// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026-present Patrick S Connallon

//! Fuzzes the config parser end-to-end: `toml::from_str` → `validate()`.
//!
//! Both arms must always return cleanly — `Ok` or `Err`, never a panic.
//! The serde-derived deserializer, the manual TOML emitter, and the
//! handwritten `validate()` between them are the highest panic-risk
//! surface in the crate, so a 5-minute run on every push catches
//! regressions that targeted tests would never reach.

#![no_main]

use libfuzzer_sys::fuzz_target;
use nexthop_lib::config::RelayConfig;

fuzz_target!(|data: &[u8]| {
    // Reject non-UTF-8 quickly so the rest of the iteration budget
    // goes to bytes that actually exercise the parser.
    let Ok(s) = std::str::from_utf8(data) else {
        return;
    };
    match toml::from_str::<RelayConfig>(s) {
        Ok(cfg) => {
            // validate() does its own work beyond what serde catches
            // (address parsing, mode/cast compatibility, rate-limit
            // sanity, multicast interface IPs, etc.).
            let _ = cfg.validate();
        }
        Err(_) => {
            // Parse failures are expected; we only assert no panic.
        }
    }
});
