// TypeScript types mirroring the Rust serde structs in src-tauri/src/config.rs and stats.rs.
// Serde rename_all annotations:
//   Protocol, EndpointMode, CastMode → "lowercase"  ("tcp", "udp", "server", "client", etc.)
//   OverflowPolicy                   → "snake_case"  ("drop_newest", "block")

export type Protocol = "tcp" | "udp";
export type EndpointMode = "server" | "client";
export type CastMode = "unicast" | "broadcast" | "multicast";
export type OverflowPolicy = "drop_newest" | "block";

export interface GeneralConfig {
  log_level: string;
  stats_interval_secs: number;
  channel_capacity: number;
  max_payload_size: number;
  health_port: number | null;
}

// Mirrors EndpointConfig. All fields are present in JSON (optional ones may be null).
export interface EndpointConfig {
  name: string | null;
  protocol: Protocol;
  mode: EndpointMode;
  address: string;
  cast_mode: CastMode;
  multicast_interface: string | null;
  multicast_interface_index: number | null;
  multicast_ttl: number;
  reconnect_delay_ms: number | null;
}

// DestConfig uses #[serde(flatten)] on base: EndpointConfig, so all EndpointConfig
// fields appear at the top level of this object in JSON.
export interface DestConfig {
  name: string | null;
  protocol: Protocol;
  mode: EndpointMode;
  address: string;
  cast_mode: CastMode;
  multicast_interface: string | null;
  multicast_interface_index: number | null;
  multicast_ttl: number;
  reconnect_delay_ms: number | null;
  overflow_policy: OverflowPolicy;
}

export interface RateLimitConfig {
  bytes_per_second: number;
  burst_size: number;
}

export interface RelayConfig {
  general: GeneralConfig;
  source: EndpointConfig;
  rate_limit: RateLimitConfig | null;
  destinations: DestConfig[];
}

export interface StatsSnapshot {
  label: string;
  local_addr: string;
  peer_addr: string;
  snapshot_at: number;
  uptime_s: number;
  rx_bytes: number;
  tx_bytes: number;
  messages: number;
  active_connections: number;
  total_connections: number;
  errors: number;
  /** Sum of dropped_overflow + dropped_oversize + dropped_validation + dropped_write_error. */
  dropped: number;
  dropped_overflow: number;
  dropped_oversize: number;
  dropped_validation: number;
  dropped_write_error: number;
}

export interface StatsPayload {
  source: StatsSnapshot;
  destinations: StatsSnapshot[];
}

export interface Prefs {
  dark_mode: boolean;
}

export function defaultEndpoint(): EndpointConfig {
  return {
    name: null,
    protocol: "udp",
    mode: "server",
    address: "0.0.0.0:5000",
    cast_mode: "unicast",
    multicast_interface: null,
    multicast_interface_index: null,
    multicast_ttl: 16,
    reconnect_delay_ms: null,
  };
}

export function defaultDestination(): DestConfig {
  return {
    name: null,
    protocol: "udp",
    mode: "client",
    address: "127.0.0.1:5001",
    cast_mode: "unicast",
    multicast_interface: null,
    multicast_interface_index: null,
    multicast_ttl: 16,
    reconnect_delay_ms: null,
    overflow_policy: "drop_newest",
  };
}

export function defaultConfig(): RelayConfig {
  return {
    general: {
      log_level: "info",
      stats_interval_secs: 30,
      channel_capacity: 1024,
      max_payload_size: 65535,
      health_port: null,
    },
    source: defaultEndpoint(),
    rate_limit: null,
    destinations: [defaultDestination()],
  };
}
