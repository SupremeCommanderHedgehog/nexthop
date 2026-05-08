import { describe, it, expect } from "vitest";
import {
  defaultEndpoint,
  defaultDestination,
  defaultConfig,
  type RelayConfig,
  type EndpointConfig,
  type DestConfig,
} from "../types";

// ── defaultEndpoint ────────────────────────────────────────────────────────

describe("defaultEndpoint", () => {
  it("returns udp server on 0.0.0.0:5000", () => {
    const ep = defaultEndpoint();
    expect(ep.protocol).toBe("udp");
    expect(ep.mode).toBe("server");
    expect(ep.address).toBe("0.0.0.0:5000");
  });

  it("defaults cast_mode to unicast", () => {
    expect(defaultEndpoint().cast_mode).toBe("unicast");
  });

  it("defaults multicast_ttl to 16", () => {
    expect(defaultEndpoint().multicast_ttl).toBe(16);
  });

  it("defaults optional fields to null", () => {
    const ep = defaultEndpoint();
    expect(ep.name).toBeNull();
    expect(ep.multicast_interface).toBeNull();
    expect(ep.multicast_interface_index).toBeNull();
    expect(ep.reconnect_delay_ms).toBeNull();
  });
});

// ── defaultDestination ─────────────────────────────────────────────────────

describe("defaultDestination", () => {
  it("returns udp client to 127.0.0.1:5001", () => {
    const d = defaultDestination();
    expect(d.protocol).toBe("udp");
    expect(d.mode).toBe("client");
    expect(d.address).toBe("127.0.0.1:5001");
  });

  it("defaults overflow_policy to drop_newest", () => {
    expect(defaultDestination().overflow_policy).toBe("drop_newest");
  });

  it("defaults cast_mode to unicast", () => {
    expect(defaultDestination().cast_mode).toBe("unicast");
  });

  it("defaults multicast_ttl to 16", () => {
    expect(defaultDestination().multicast_ttl).toBe(16);
  });

  it("defaults optional fields to null", () => {
    const d = defaultDestination();
    expect(d.name).toBeNull();
    expect(d.multicast_interface).toBeNull();
    expect(d.multicast_interface_index).toBeNull();
    expect(d.reconnect_delay_ms).toBeNull();
  });
});

// ── defaultConfig ──────────────────────────────────────────────────────────

describe("defaultConfig", () => {
  it("has exactly one destination", () => {
    expect(defaultConfig().destinations).toHaveLength(1);
  });

  it("general defaults are correct", () => {
    const g = defaultConfig().general;
    expect(g.log_level).toBe("info");
    expect(g.stats_interval_secs).toBe(30);
    expect(g.channel_capacity).toBe(1024);
    expect(g.max_payload_size).toBe(65535);
    expect(g.health_port).toBeNull();
  });

  it("rate_limit defaults to null", () => {
    expect(defaultConfig().rate_limit).toBeNull();
  });

  it("source matches defaultEndpoint()", () => {
    const cfg = defaultConfig();
    const ep = defaultEndpoint();
    expect(cfg.source).toEqual(ep);
  });

  it("destination matches defaultDestination()", () => {
    const cfg = defaultConfig();
    const d = defaultDestination();
    expect(cfg.destinations[0]).toEqual(d);
  });

  it("produces a structurally valid config object", () => {
    const cfg: RelayConfig = defaultConfig();
    expect(cfg).toHaveProperty("general");
    expect(cfg).toHaveProperty("source");
    expect(cfg).toHaveProperty("destinations");
    expect(Array.isArray(cfg.destinations)).toBe(true);
  });
});

// ── Type shape invariants ──────────────────────────────────────────────────

describe("EndpointConfig shape", () => {
  it("has all required fields", () => {
    const ep: EndpointConfig = defaultEndpoint();
    const keys = Object.keys(ep);
    for (const k of [
      "name", "protocol", "mode", "address", "cast_mode",
      "multicast_interface", "multicast_interface_index",
      "multicast_ttl", "reconnect_delay_ms",
    ]) {
      expect(keys).toContain(k);
    }
  });
});

describe("DestConfig shape", () => {
  it("has overflow_policy in addition to endpoint fields", () => {
    const d: DestConfig = defaultDestination();
    expect(d).toHaveProperty("overflow_policy");
    expect(d).toHaveProperty("protocol");
    expect(d).toHaveProperty("address");
  });
});
