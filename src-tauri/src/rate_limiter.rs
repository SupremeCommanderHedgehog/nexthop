// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026-present Patrick S Connallon
//
// nexthop - TCP / UDP - Unicast / Multicast / Broadcast
// Architect: Patrick S Connallon
// Built by:  Anthropic Claude (Sonnet 4.6)

use std::sync::atomic::{AtomicU64, Ordering};
use tokio::time::{Duration, Instant};

/// Async token-bucket rate limiter.
///
/// State is stored in two AtomicU64s (token count as f64 bits, last-refill
/// timestamp as nanoseconds from epoch) updated via CAS loops, eliminating
/// mutex contention across concurrent source connections.
pub struct RateLimiter {
    tokens: AtomicU64,  // f64::to_bits() of current token count (bytes)
    last_ns: AtomicU64, // nanoseconds since `epoch`
    max_tokens: f64,
    rate: f64, // bytes per second
    epoch: Instant,
}

impl RateLimiter {
    /// Create a limiter that allows `bytes_per_second` sustained throughput
    /// with a burst allowance of `burst` bytes.
    pub fn new(bytes_per_second: u64, burst: u64) -> Self {
        let max = burst as f64;
        Self {
            tokens: AtomicU64::new(max.to_bits()),
            last_ns: AtomicU64::new(0),
            max_tokens: max,
            rate: bytes_per_second as f64,
            epoch: Instant::now(),
        }
    }

    /// Waits until `n` bytes worth of tokens are available, then consumes them.
    pub async fn acquire(&self, n: u64) {
        let needed = n as f64;
        loop {
            let now = Instant::now();
            let now_ns = now.duration_since(self.epoch).as_nanos() as u64;
            let last_ns = self.last_ns.load(Ordering::Acquire);
            let elapsed = now_ns.saturating_sub(last_ns) as f64 / 1_000_000_000.0;

            let old_bits = self.tokens.load(Ordering::Acquire);
            let current = f64::from_bits(old_bits);
            let refilled = (current + elapsed * self.rate).min(self.max_tokens);

            if refilled >= needed {
                let new_bits = (refilled - needed).to_bits();
                if self
                    .tokens
                    .compare_exchange(old_bits, new_bits, Ordering::Release, Ordering::Relaxed)
                    .is_ok()
                {
                    // Advance last_ns monotonically; a racing thread may have set
                    // a later timestamp between our token CAS and this update, so
                    // fetch_max ensures we never move the clock backward.
                    self.last_ns.fetch_max(now_ns, Ordering::Release);
                    return;
                }
                // A concurrent acquire changed the token count; retry.
                continue;
            }

            let wait = Duration::from_secs_f64((needed - refilled) / self.rate);
            tokio::time::sleep(wait).await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn immediate_acquire_within_burst() {
        let rl = RateLimiter::new(1000, 5000);
        // Should return immediately: 500 < burst of 5000
        rl.acquire(500).await;
    }

    #[tokio::test]
    async fn acquire_waits_when_exhausted() {
        let rl = RateLimiter::new(10_000, 100);
        // Drain the bucket
        rl.acquire(100).await;
        let start = Instant::now();
        // Next 100 bytes must wait ~10 ms at 10 000 B/s
        rl.acquire(100).await;
        assert!(start.elapsed() >= Duration::from_millis(5));
    }

    #[tokio::test]
    async fn acquire_zero_bytes_is_immediate() {
        let rl = RateLimiter::new(100, 100);
        // Drain all tokens
        rl.acquire(100).await;
        // acquire(0) must return immediately even with empty bucket
        let start = Instant::now();
        rl.acquire(0).await;
        assert!(
            start.elapsed() < Duration::from_millis(50),
            "elapsed: {:?}",
            start.elapsed()
        );
    }

    #[tokio::test]
    async fn tokens_do_not_exceed_max() {
        // burst = 500; rate = 100_000 B/s
        let rl = RateLimiter::new(100_000, 500);
        rl.acquire(100).await; // tokens → 400
                               // Sleep 100 ms: would refill 10 000 tokens, but capped at 500
        tokio::time::sleep(Duration::from_millis(100)).await;
        // Should succeed immediately (tokens refilled to max 500)
        let start = Instant::now();
        rl.acquire(500).await;
        assert!(
            start.elapsed() < Duration::from_millis(50),
            "elapsed: {:?}",
            start.elapsed()
        );
    }

    #[tokio::test]
    async fn large_burst_initial_acquire() {
        // burst >> sustained rate: large acquire should be immediate
        let rl = RateLimiter::new(100, 10_000);
        let start = Instant::now();
        rl.acquire(9_000).await; // within burst
        assert!(
            start.elapsed() < Duration::from_millis(100),
            "elapsed: {:?}",
            start.elapsed()
        );
    }

    // ── Additional coverage ────────────────────────────────────────────

    #[tokio::test]
    async fn sequential_acquires_respect_rate() {
        // rate = 1000 B/s, burst = 1000 → each acquire(1000) should be ~1s apart
        let rl = RateLimiter::new(10_000, 1_000);
        rl.acquire(1_000).await; // drain burst instantly
        let start = Instant::now();
        rl.acquire(500).await; // must wait ~50 ms
        let elapsed = start.elapsed();
        assert!(
            elapsed >= Duration::from_millis(30),
            "too fast: {elapsed:?}"
        );
        assert!(
            elapsed < Duration::from_millis(200),
            "too slow: {elapsed:?}"
        );
    }

    #[tokio::test]
    async fn multiple_small_acquires_stay_within_budget() {
        let rl = RateLimiter::new(1_000, 200);
        rl.acquire(200).await; // drain burst
        let start = Instant::now();
        rl.acquire(100).await;
        rl.acquire(100).await;
        let elapsed = start.elapsed();
        // 200 bytes @ 1000 B/s = 200 ms
        assert!(
            elapsed >= Duration::from_millis(150),
            "too fast: {elapsed:?}"
        );
    }

    #[tokio::test]
    async fn acquire_exactly_burst_is_immediate() {
        let rl = RateLimiter::new(1, 512); // 1 B/s but 512 burst
        let start = Instant::now();
        rl.acquire(512).await;
        assert!(
            start.elapsed() < Duration::from_millis(50),
            "elapsed: {:?}",
            start.elapsed()
        );
    }

    #[tokio::test]
    async fn refill_does_not_exceed_burst() {
        // Very high rate; sleeping lets refill attempt to exceed burst.
        let rl = RateLimiter::new(1_000_000, 100);
        rl.acquire(100).await; // drain completely
        tokio::time::sleep(Duration::from_millis(500)).await; // would refill 500k tokens
                                                              // Bucket is capped at 100; this must not wait at all.
        let start = Instant::now();
        rl.acquire(100).await;
        assert!(
            start.elapsed() < Duration::from_millis(50),
            "elapsed: {:?}",
            start.elapsed()
        );
    }
}
