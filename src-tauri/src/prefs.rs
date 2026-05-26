// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026-present Patrick S Connallon

use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Prefs {
    #[serde(default = "default_dark")]
    pub dark_mode: bool,
}

fn default_dark() -> bool {
    true
}

impl Default for Prefs {
    fn default() -> Self {
        Self { dark_mode: true }
    }
}

impl Prefs {
    pub fn load(path: &str) -> Result<Self, String> {
        match std::fs::read_to_string(path) {
            Err(_) => Ok(Self::default()),
            Ok(s) => toml::from_str(&s).map_err(|e| format!("preferences parse error: {e}")),
        }
    }

    pub fn save(&self, path: &str) -> Result<(), String> {
        let s = toml::to_string_pretty(self).map_err(|e| format!("serialize: {e}"))?;
        std::fs::write(path, s).map_err(|e| format!("write: {e}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(name: &str) -> String {
        std::env::temp_dir().join(name).to_string_lossy().into_owned()
    }

    #[test]
    fn load_missing_file_returns_default() {
        let p = tmp("nexthop_prefs_missing_xyzzy.toml");
        let _ = std::fs::remove_file(&p);
        let prefs = Prefs::load(&p).expect("missing file should give default");
        assert!(prefs.dark_mode);
    }

    #[test]
    fn load_invalid_toml_returns_error() {
        let p = tmp("nexthop_prefs_invalid_xyzzy.toml");
        std::fs::write(&p, "this = [[[not valid toml").unwrap();
        let r = Prefs::load(&p);
        let _ = std::fs::remove_file(&p);
        assert!(r.is_err());
        assert!(r.unwrap_err().contains("preferences parse error"));
    }

    #[test]
    fn save_and_load_round_trip() {
        let p = tmp("nexthop_prefs_roundtrip_xyzzy.toml");
        let prefs = Prefs { dark_mode: false };
        prefs.save(&p).expect("save");
        let loaded = Prefs::load(&p).expect("load");
        let _ = std::fs::remove_file(&p);
        assert!(!loaded.dark_mode);
    }

    #[test]
    fn default_is_dark() {
        assert!(Prefs::default().dark_mode);
    }

    // ── Additional coverage ────────────────────────────────────────────

    #[test]
    fn save_and_load_dark_mode_true() {
        let p = tmp("nexthop_prefs_dark_xyzzy.toml");
        let prefs = Prefs { dark_mode: true };
        prefs.save(&p).expect("save");
        let loaded = Prefs::load(&p).expect("load");
        let _ = std::fs::remove_file(&p);
        assert!(loaded.dark_mode);
    }

    #[test]
    fn load_empty_file_returns_default() {
        let p = tmp("nexthop_prefs_empty_xyzzy.toml");
        std::fs::write(&p, "").unwrap();
        let prefs = Prefs::load(&p).expect("empty file should give default");
        let _ = std::fs::remove_file(&p);
        assert!(prefs.dark_mode);
    }

    #[test]
    fn load_partial_file_uses_defaults_for_missing_fields() {
        let p = tmp("nexthop_prefs_partial_xyzzy.toml");
        // dark_mode absent → falls back to default_dark() = true
        std::fs::write(&p, "# no fields\n").unwrap();
        let prefs = Prefs::load(&p).expect("partial file should deserialize");
        let _ = std::fs::remove_file(&p);
        assert!(prefs.dark_mode);
    }

    #[test]
    fn save_produces_valid_toml() {
        let p = tmp("nexthop_prefs_valid_xyzzy.toml");
        let prefs = Prefs { dark_mode: false };
        prefs.save(&p).expect("save");
        let content = std::fs::read_to_string(&p).unwrap();
        let _ = std::fs::remove_file(&p);
        // toml::from_str should not error
        let _: Prefs = toml::from_str(&content).expect("re-parse saved file");
    }

    #[test]
    fn clone_produces_equal_value() {
        let p = Prefs { dark_mode: true };
        let q = p.clone();
        assert_eq!(p.dark_mode, q.dark_mode);
    }

    #[test]
    fn debug_format_contains_dark_mode() {
        let p = Prefs { dark_mode: false };
        let s = format!("{p:?}");
        assert!(s.contains("dark_mode"), "got: {s}");
    }
}
