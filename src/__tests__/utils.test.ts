import { describe, it, expect } from "vitest";

// ── fmtBytes (copied from MonitorTab for testability) ─────────────────────
// These are pure functions; test them in isolation.

function fmtBytes(n: number): string {
  if (n >= 1_000_000_000) return `${(n / 1_000_000_000).toFixed(2)} GB`;
  if (n >= 1_000_000) return `${(n / 1_000_000).toFixed(2)} MB`;
  if (n >= 1_000) return `${(n / 1_000).toFixed(2)} KB`;
  return `${n} B`;
}

// ── fmtUptime (copied from MonitorTab for testability) ────────────────────

function fmtUptime(s: number): string {
  const h = Math.floor(s / 3600);
  const m = Math.floor((s % 3600) / 60);
  const sec = s % 60;
  return `${h}h ${String(m).padStart(2, "0")}m ${String(sec).padStart(2, "0")}s`;
}

// ── Address split helper (mirrors ConfigTab logic) ─────────────────────────

function splitAddr(address: string): { host: string; port: string } {
  const parts = address.split(":");
  const port = parts[parts.length - 1] ?? "";
  const host = parts.slice(0, -1).join(":") || parts[0];
  return { host, port };
}

// ── Multicast address validator (mirrors ConfigTab regex) ──────────────────

const MULTICAST_RE = /^(22[4-9]|23[0-9])(\.\d{1,3}){3}$/;

function isValidMulticast(ip: string): boolean {
  return MULTICAST_RE.test(ip);
}

// ── fmtBytes tests ─────────────────────────────────────────────────────────

describe("fmtBytes", () => {
  it("formats bytes under 1 KB", () => {
    expect(fmtBytes(0)).toBe("0 B");
    expect(fmtBytes(1)).toBe("1 B");
    expect(fmtBytes(999)).toBe("999 B");
  });

  it("formats kilobytes", () => {
    expect(fmtBytes(1000)).toBe("1.00 KB");
    expect(fmtBytes(1500)).toBe("1.50 KB");
    expect(fmtBytes(999_999)).toBe("1000.00 KB");
  });

  it("formats megabytes", () => {
    expect(fmtBytes(1_000_000)).toBe("1.00 MB");
    expect(fmtBytes(2_500_000)).toBe("2.50 MB");
  });

  it("formats gigabytes", () => {
    expect(fmtBytes(1_000_000_000)).toBe("1.00 GB");
    expect(fmtBytes(3_750_000_000)).toBe("3.75 GB");
  });

  it("uses two decimal places always", () => {
    expect(fmtBytes(1_100_000)).toMatch(/^\d+\.\d{2} MB$/);
    expect(fmtBytes(1_100)).toMatch(/^\d+\.\d{2} KB$/);
  });
});

// ── fmtUptime tests ────────────────────────────────────────────────────────

describe("fmtUptime", () => {
  it("formats zero uptime", () => {
    expect(fmtUptime(0)).toBe("0h 00m 00s");
  });

  it("formats seconds only", () => {
    expect(fmtUptime(45)).toBe("0h 00m 45s");
  });

  it("formats minutes and seconds", () => {
    expect(fmtUptime(90)).toBe("0h 01m 30s");
  });

  it("pads single-digit minutes and seconds", () => {
    expect(fmtUptime(61)).toBe("0h 01m 01s");
  });

  it("formats hours, minutes, seconds", () => {
    expect(fmtUptime(3661)).toBe("1h 01m 01s");
  });

  it("formats large uptime correctly", () => {
    // 2h 30m 15s = 2*3600 + 30*60 + 15 = 7200 + 1800 + 15 = 9015
    expect(fmtUptime(9015)).toBe("2h 30m 15s");
  });

  it("handles exactly one hour", () => {
    expect(fmtUptime(3600)).toBe("1h 00m 00s");
  });

  it("handles 23h 59m 59s", () => {
    expect(fmtUptime(86399)).toBe("23h 59m 59s");
  });
});

// ── splitAddr tests ────────────────────────────────────────────────────────

describe("splitAddr (IPv4)", () => {
  it("splits host and port for simple IPv4", () => {
    const { host, port } = splitAddr("192.168.1.1:5000");
    expect(host).toBe("192.168.1.1");
    expect(port).toBe("5000");
  });

  it("splits 0.0.0.0:5000", () => {
    const { host, port } = splitAddr("0.0.0.0:5000");
    expect(host).toBe("0.0.0.0");
    expect(port).toBe("5000");
  });

  it("handles port 80", () => {
    const { host, port } = splitAddr("127.0.0.1:80");
    expect(host).toBe("127.0.0.1");
    expect(port).toBe("80");
  });
});

describe("splitAddr (IPv6)", () => {
  it("splits [::1]:8080 correctly", () => {
    const { host, port } = splitAddr("[::1]:8080");
    expect(host).toBe("[::1]");
    expect(port).toBe("8080");
  });

  it("splits [2001:db8::1]:443 correctly", () => {
    const { host, port } = splitAddr("[2001:db8::1]:443");
    expect(host).toBe("[2001:db8::1]");
    expect(port).toBe("443");
  });
});

// ── isValidMulticast tests ─────────────────────────────────────────────────

describe("multicast address validation", () => {
  it("accepts valid multicast addresses (224-239 range)", () => {
    expect(isValidMulticast("224.0.0.1")).toBe(true);
    expect(isValidMulticast("239.255.255.255")).toBe(true);
    expect(isValidMulticast("225.1.2.3")).toBe(true);
    expect(isValidMulticast("230.0.0.0")).toBe(true);
  });

  it("rejects addresses below 224", () => {
    expect(isValidMulticast("223.255.255.255")).toBe(false);
    expect(isValidMulticast("192.168.1.1")).toBe(false);
    expect(isValidMulticast("0.0.0.0")).toBe(false);
  });

  it("rejects addresses above 239", () => {
    expect(isValidMulticast("240.0.0.0")).toBe(false);
    expect(isValidMulticast("255.255.255.255")).toBe(false);
  });

  it("rejects non-IP strings", () => {
    expect(isValidMulticast("not-an-ip")).toBe(false);
    expect(isValidMulticast("")).toBe(false);
    expect(isValidMulticast("224")).toBe(false);
  });

  it("rejects partial IPs", () => {
    expect(isValidMulticast("224.1.2")).toBe(false);
    expect(isValidMulticast("224.1")).toBe(false);
  });

  it("accepts boundary values 224 and 239", () => {
    expect(isValidMulticast("224.0.0.0")).toBe(true);
    expect(isValidMulticast("239.255.255.255")).toBe(true);
  });
});
