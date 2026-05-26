// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026-present Patrick S Connallon

// Prevents a console window from appearing on Windows in release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    nexthop_lib::run();
}
