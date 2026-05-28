# ADR 0002 — Per-destination transform pipeline

- **Status:** accepted
- **Date:** 2026-05-28
- **Closes:** [#28](https://github.com/SupremeCommanderHedgehog/nexthop/issues/28)

## Context

The relay today forwards payloads byte-for-byte from the source to
every destination's fan-out. Several requested operator workflows
cannot be expressed with that model:

- Strip a 4-byte length prefix that the upstream producer adds but
  the downstream consumer rejects.
- Drop datagrams smaller than a threshold (heartbeats), or larger
  than a threshold (PCAP-style oversized frames).
- Byte-swap 16- or 32-bit words for endian-incompatible consumers.
- Prepend a timestamp so downstream loggers do not need their own
  clocks.
- Filter on payload content (regex match / no-match).

These all share the same shape: "given the bytes, decide whether to
forward them and possibly modify them first." That is a classic
transform pipeline.

This ADR picks the design; it does **not** ship transforms. Each
concrete transform plus its config schema lands in a separate PR with
its own tests.

## Decision

Introduce a **per-destination, ordered, type-erased pipeline of
`Transform`s** invoked between the destination's mpsc receive and
its socket write. Each transform either passes (possibly modified)
or drops the payload; the pipeline short-circuits on the first
drop.

### Trait shape

```rust
pub enum Decision {
    /// Forward this payload. May be the same `Bytes` we received
    /// (zero-copy pass-through) or a freshly allocated rewrite.
    Pass(Bytes),
    /// Discard. The destination counts this against
    /// `dropped_validation` (the existing per-reason sub-counter).
    Drop,
}

pub trait Transform: Send + Sync + 'static {
    /// Apply the transform. `payload` is the running payload after
    /// every earlier transform in the same pipeline. Implementations
    /// must be lock-free or use their own internal sync — the
    /// pipeline calls this without holding any of the relay's locks.
    fn apply(&self, payload: Bytes) -> Decision;
}
```

Each `DestEntry` (already defined for the supervisor — see
ADR-less #25 / PR #53) gains a `transforms: Vec<Arc<dyn Transform>>`.
The destination's write loop, immediately after `rx.recv()`, runs:

```rust
let mut payload = received;
for t in &self.transforms {
    match t.apply(payload) {
        Decision::Pass(next) => payload = next,
        Decision::Drop => {
            stats.add_dropped_validation(1);
            continue 'recv_loop;
        }
    }
}
// existing write path: acquire rate-limit tokens, write to socket.
```

The pipeline is read-only after `DestEntry` is spawned. To change a
destination's transforms, the supervisor follows the existing
identity-change path from PR #53: remove the entry, spawn a new one
with the new pipeline. Per-transform live mutation is deliberately
out of scope; the simpler invariant (pipeline is immutable for the
lifetime of a `DestEntry`) keeps the hot path lock-free.

### Config shape

`[[destinations.transforms]]` is a nested array-of-tables. Each
entry has a `type` tag that selects the transform and any
transform-specific fields:

```toml
[[destinations]]
protocol = "udp"
mode     = "client"
address  = "127.0.0.1:5001"

[[destinations.transforms]]
type    = "drop_smaller_than"
n_bytes = 16

[[destinations.transforms]]
type = "byte_swap_16"

[[destinations.transforms]]
type    = "prepend_timestamp"
clock   = "epoch_ns"
```

The order in the file is the order in the pipeline. Per-destination
means each `[[destinations]]` can carry its own list — one consumer
can have timestamps prepended while another sees raw bytes,
matching the existing per-dest `rate_limit` and `overflow_policy`
pattern.

### Initial transform set

The five transforms listed in #28 ship in follow-up PRs, in this
order, so the trait design is validated against real
implementations before more land:

1. `drop_smaller_than { n_bytes: usize }`
2. `drop_larger_than { n_bytes: usize }`
3. `byte_swap_16` / `byte_swap_32` (no params)
4. `prepend_timestamp { clock: "epoch_ns" | "monotonic_ns" }`
5. `regex_filter { pattern: String, action: "drop_match" | "drop_non_match" }`

Each transform PR adds a `relay::transforms::<name>` module, a
deserialization branch in the `[[destinations.transforms]]` parser,
and at least one unit test per code path (pass + drop).

### WASM is deferred

Approach B from #28 — `.wasm` modules loaded at runtime via
`wasmtime` — is **not adopted now**. It stays on the roadmap as a
future `features = ["wasm-transforms"]` cargo feature. The
`Transform` trait above is shaped so a `WasmTransform` wrapper can
be one of the implementations later; the dispatch path does not
care whether the impl is native Rust or a sandboxed guest.

## Known trade-offs

### Adding a transform requires a rebuild

This is the trade-off Approach B (WASM) was supposed to dodge. We
accept it because:

- The relay's typical deployment model is "build once, ship the
  binary." Operators who want a new transform open a PR with the
  new module; that PR is reviewed and merged like any other code
  change. Their next deploy gets the transform.
- The five initial transforms cover the bulk of what's actually been
  requested. Once that set is in place, the marginal value of a new
  one is low enough that we don't need a runtime-loadable plugin
  mechanism yet.
- WASM brings its own significant cost: a ~5–10 MB additional dep
  tree (wasmtime + cranelift), a sandbox-escape attack surface, and
  per-packet µs-scale overhead vs. ns-scale for Rust traits.

### Per-destination, not per-source

The pipeline runs **after** the source's fan-out, on each
destination's queue. Means: identical payloads land in every
destination's mpsc, and each destination's pipeline rewrites them
independently. If two destinations both want the same transform,
they each pay the (cheap) cost of running it.

The alternative would be a single source-side pipeline applied
before fan-out. That is simpler and avoids the duplicate cost, but
loses the flexibility users have already asked for ("prepend a
timestamp on the logging destination, not on the live mirror"). The
overflow-policy and rate-limit settings already work per-destination
for the same reason; the transform pipeline mirrors that pattern.

A future ADR can add a **source-side** pre-fan-out pipeline as a
separate `[source.transforms]` table if and when a use case emerges
that benefits from it. The two are orthogonal.

### Pipeline is immutable for the entry's lifetime

To change a destination's transforms, the supervisor treats it as
an identity change (remove + add) — same code path as changing the
address. This means:

- A live transform swap drops in-flight queued packets in that
  destination's mpsc.
- The remove+drain window is bounded by the existing 5-second
  shutdown timeout.
- Other destinations are unaffected.

This is consistent with how reconfiguring an address or protocol
already works (PR #53). Operators who need to A/B-test a new
transform pipeline against an existing destination should add a
**new** destination with the new pipeline rather than mutating
the existing one.

### Errors short-circuit; panics abort

`Transform::apply` returns `Decision`, not `Result<Decision, _>`.
A transform that cannot decide (regex compile failed, buffer ran
out, etc.) must internally choose to either `Pass` or `Drop`.
Panics inside `apply` propagate the same way panics inside the
existing destination write loop do — they crash that destination's
task, which the supervisor logs and surfaces in stats. The hot path
does not pay for catch_unwind on every packet.

## Alternatives considered

- **Approach B (WASM) immediately.** Rejected per the trade-offs
  above. Stays on the roadmap as a feature flag.
- **Single global pipeline applied source-side.** Rejected — loses
  per-destination flexibility that the rate-limit and
  overflow-policy mechanisms already establish as a pattern.
- **Both source-side and per-destination from the start.** Rejected
  on YAGNI grounds. Per-dest covers every requested use case; a
  source-side variant can be layered in later as its own ADR if a
  use case surfaces (e.g. an oversize-validate step that should
  apply once, not per destination).
- **Stateful pipeline shared across packets.** All five initial
  transforms are stateless or use only intrinsic state (regex
  compile result, length threshold). A future stateful transform
  — token-bucket aggregation, sliding-window dedupe — can use
  internal `Arc<Mutex<…>>` or atomics within its own struct without
  the trait changing.
- **Decision shape `Drop | Pass | Rewrite(Vec<u8>)`.** Rejected as
  redundant: `Pass(Bytes)` already covers both "forward unchanged"
  (return the input `Bytes` clone, which is a refcount bump) and
  "forward modified" (construct a new `Bytes`). One enum variant
  fewer.

## Consequences

- Five follow-up issues land transforms #1–#5 (one PR per transform
  with config schema, parser branch, and tests). They can land in
  any order once the trait + config plumbing is in place.
- The first follow-up PR also lands the pipeline mechanism: the
  `Transform` trait, the `Decision` enum, the `transforms:
  Vec<Arc<dyn Transform>>` field on `DestEntry`, the destination
  task's call into the pipeline, and the
  `[[destinations.transforms]]` parser. Subsequent transform PRs
  only add their own module and config branch.
- `add_dropped_validation` (already a per-reason counter; see PR
  #41 / drop-reason breakdown) is the home for pipeline drops. No
  new stats counters needed.
- MANUAL.md gains a new "Transforms" section after each transform
  lands, documenting the config shape and behavior. The Hot reload
  matrix gets a `[[destinations.transforms]]` row marking it
  restart-required (per the immutability decision above).
- This ADR establishes that a future WASM-based extension would be
  a feature flag, not a separate codebase.
