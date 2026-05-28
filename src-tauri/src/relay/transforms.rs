//! Per-destination transform pipeline. See ADR 0002.
//!
//! Each `DestEntry` owns a `Vec<Arc<dyn Transform>>` built once at spawn
//! from the destination's `[[destinations.transforms]]` config. The
//! destination write loop runs each payload through the pipeline
//! immediately after `rx.recv()` and before rate-limit acquisition. The
//! pipeline short-circuits on the first `Drop`, incrementing
//! `dropped_validation` on the destination's stats and discarding the
//! payload.

use std::sync::Arc;

use bytes::Bytes;

use crate::config::TransformConfig;
use crate::stats::Stats;

/// Outcome of one `Transform::apply` call.
pub enum Decision {
    /// Forward this payload. May be the same `Bytes` we received
    /// (zero-copy refcount bump) or a freshly allocated rewrite.
    Pass(Bytes),
    /// Discard the payload. The destination counts it against
    /// `dropped_validation`.
    Drop,
}

/// One stage in a destination's pipeline. Implementations must be
/// lock-free or use their own internal sync; the dispatch loop calls
/// `apply` without holding any relay locks.
pub trait Transform: Send + Sync + 'static {
    fn apply(&self, payload: Bytes) -> Decision;
}

/// Build a pipeline from a destination's TOML config.
pub fn build_pipeline(configs: &[TransformConfig]) -> Vec<Arc<dyn Transform>> {
    configs
        .iter()
        .map(|cfg| match cfg {
            TransformConfig::DropSmallerThan { n_bytes } => {
                Arc::new(DropSmallerThan { n_bytes: *n_bytes }) as Arc<dyn Transform>
            }
        })
        .collect()
}

/// Apply every transform in order. Returns `Some(payload)` to forward
/// (the input passed every stage) or `None` if any stage dropped.
/// `dropped_validation` is incremented exactly once per dropped payload.
pub fn apply_pipeline(
    pipeline: &[Arc<dyn Transform>],
    stats: &Stats,
    payload: Bytes,
) -> Option<Bytes> {
    let mut current = payload;
    for t in pipeline {
        match t.apply(current) {
            Decision::Pass(next) => current = next,
            Decision::Drop => {
                stats.add_dropped_validation(1);
                return None;
            }
        }
    }
    Some(current)
}

// ── concrete transforms ───────────────────────────────────────────────

/// Drop payloads smaller than `n_bytes`.
pub struct DropSmallerThan {
    pub n_bytes: usize,
}

impl Transform for DropSmallerThan {
    fn apply(&self, payload: Bytes) -> Decision {
        if payload.len() < self.n_bytes {
            Decision::Drop
        } else {
            Decision::Pass(payload)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drop_smaller_than_passes_at_threshold() {
        let t = DropSmallerThan { n_bytes: 4 };
        let payload = Bytes::from_static(&[1, 2, 3, 4]);
        match t.apply(payload.clone()) {
            Decision::Pass(out) => assert_eq!(out, payload),
            Decision::Drop => panic!("expected Pass at threshold"),
        }
    }

    #[test]
    fn drop_smaller_than_passes_above_threshold() {
        let t = DropSmallerThan { n_bytes: 4 };
        let payload = Bytes::from_static(&[1, 2, 3, 4, 5]);
        match t.apply(payload.clone()) {
            Decision::Pass(out) => assert_eq!(out, payload),
            Decision::Drop => panic!("expected Pass above threshold"),
        }
    }

    #[test]
    fn drop_smaller_than_drops_below_threshold() {
        let t = DropSmallerThan { n_bytes: 4 };
        let payload = Bytes::from_static(&[1, 2, 3]);
        assert!(matches!(t.apply(payload), Decision::Drop));
    }

    #[test]
    fn apply_pipeline_pass_through_when_empty() {
        let stats = Stats::new("test", "", "");
        let payload = Bytes::from_static(&[1, 2, 3]);
        let out = apply_pipeline(&[], &stats, payload.clone()).unwrap();
        assert_eq!(out, payload);
        assert_eq!(stats.snapshot().dropped_validation, 0);
    }

    #[test]
    fn apply_pipeline_short_circuits_on_drop() {
        let stats = Stats::new("test", "", "");
        let pipeline: Vec<Arc<dyn Transform>> = vec![Arc::new(DropSmallerThan { n_bytes: 10 })];
        let payload = Bytes::from_static(&[1, 2, 3]);
        assert!(apply_pipeline(&pipeline, &stats, payload).is_none());
        assert_eq!(stats.snapshot().dropped_validation, 1);
    }

    #[test]
    fn build_pipeline_constructs_drop_smaller_than() {
        let cfgs = vec![TransformConfig::DropSmallerThan { n_bytes: 8 }];
        let pipeline = build_pipeline(&cfgs);
        assert_eq!(pipeline.len(), 1);
        // Verify it actually drops below 8 bytes.
        let stats = Stats::new("test", "", "");
        let payload = Bytes::from_static(&[1; 7]);
        assert!(apply_pipeline(&pipeline, &stats, payload).is_none());
    }
}
