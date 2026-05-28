import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import type {
  CastMode,
  DestConfig,
  EndpointMode,
  OverflowPolicy,
  Protocol,
  RelayConfig,
} from "../types";
import { defaultConfig, defaultDestination } from "../types";
import type { StatusMsg } from "../App";

interface ConfigTabProps {
  localIps: string[];
  broadcastIps: string[];
  isRunning: boolean;
  onStart: (config: RelayConfig) => Promise<void>;
  onStop: () => Promise<void>;
  onStatus: (msg: StatusMsg | null) => void;
  version?: string;
}

const LOG_LEVELS = ["trace", "debug", "info", "warn", "error"];
const PROTOCOLS: Protocol[] = ["udp", "tcp"];
const MODES: EndpointMode[] = ["server", "client"];
const CAST_MODES: CastMode[] = ["unicast", "broadcast", "multicast"];
const SOURCE_CAST_MODES: CastMode[] = ["unicast", "multicast"];
const OVERFLOW_POLICIES: OverflowPolicy[] = ["drop_newest", "block"];

function Select<T extends string>({
  value,
  options,
  onChange,
  disabled,
  labels,
}: {
  value: T;
  options: T[];
  onChange: (v: T) => void;
  disabled?: boolean;
  labels?: Partial<Record<T, string>>;
}) {
  return (
    <select
      value={value}
      disabled={disabled}
      onChange={(e) => onChange(e.target.value as T)}
      className="border border-gray-300 dark:border-gray-600 rounded px-2 py-1 text-sm bg-white dark:bg-gray-800 disabled:opacity-50"
    >
      {options.map((o) => (
        <option key={o} value={o}>
          {labels?.[o] ?? o}
        </option>
      ))}
    </select>
  );
}

const MULTICAST_MODE_LABELS: Partial<Record<EndpointMode, string>> = {
  client: "publisher",
  server: "subscriber",
};

function TextInput({
  value,
  onChange,
  placeholder,
  disabled,
  className,
}: {
  value: string;
  onChange: (v: string) => void;
  placeholder?: string;
  disabled?: boolean;
  className?: string;
}) {
  return (
    <input
      type="text"
      value={value}
      placeholder={placeholder}
      disabled={disabled}
      onChange={(e) => onChange(e.target.value)}
      className={`border border-gray-300 dark:border-gray-600 rounded px-2 py-1 text-sm bg-white dark:bg-gray-800 disabled:opacity-50 ${className ?? ""}`}
    />
  );
}

function NumberInput({
  value,
  onChange,
  min,
  disabled,
  className,
}: {
  value: number | null;
  onChange: (v: number | null) => void;
  min?: number;
  disabled?: boolean;
  className?: string;
}) {
  return (
    <input
      type="number"
      value={value ?? ""}
      min={min}
      disabled={disabled}
      onChange={(e) =>
        onChange(e.target.value === "" ? null : Number(e.target.value))
      }
      className={`border border-gray-300 dark:border-gray-600 rounded px-2 py-1 text-sm bg-white dark:bg-gray-800 disabled:opacity-50 ${className ?? ""}`}
    />
  );
}

function SectionHeader({ children }: { children: React.ReactNode }) {
  return (
    <h2 className="text-base font-semibold text-gray-700 dark:text-gray-200 mb-3">
      {children}
    </h2>
  );
}

function Label({ children }: { children: React.ReactNode }) {
  return (
    <label className="text-sm text-gray-600 dark:text-gray-400 whitespace-nowrap">
      {children}
    </label>
  );
}

function Row({ children }: { children: React.ReactNode }) {
  return <div className="flex items-center gap-3 flex-wrap">{children}</div>;
}

export default function ConfigTab({
  localIps,
  broadcastIps,
  isRunning,
  onStart,
  onStop,
  onStatus,
  version,
}: ConfigTabProps) {
  const [config, setConfig] = useState<RelayConfig>(defaultConfig());
  const [loading, setLoading] = useState(true);

  useEffect(() => {
    invoke<RelayConfig>("get_config")
      .then((cfg) => {
        // Rust omits None fields; normalize to null so checks are consistent.
        cfg.rate_limit = cfg.rate_limit ?? null;
        cfg.general.health_port = cfg.general.health_port ?? null;
        cfg.source.name = cfg.source.name ?? null;
        cfg.source.multicast_interface = cfg.source.multicast_interface ?? null;
        cfg.source.multicast_interface_index = cfg.source.multicast_interface_index ?? null;
        cfg.source.reconnect_delay_ms = cfg.source.reconnect_delay_ms ?? null;
        cfg.destinations = cfg.destinations.map((d) => ({
          ...d,
          name: d.name ?? null,
          multicast_interface: d.multicast_interface ?? null,
          multicast_interface_index: d.multicast_interface_index ?? null,
          reconnect_delay_ms: d.reconnect_delay_ms ?? null,
        }));
        setConfig(cfg);
        setLoading(false);
      })
      .catch((e) => {
        onStatus({ text: String(e), err: true });
        setLoading(false);
      });
  }, [onStatus]);

  const updateSource = (patch: Partial<typeof config.source>) =>
    setConfig((c) => ({ ...c, source: { ...c.source, ...patch } }));

  const updateGeneral = (patch: Partial<typeof config.general>) =>
    setConfig((c) => ({ ...c, general: { ...c.general, ...patch } }));

  const updateDest = (idx: number, patch: Partial<DestConfig>) =>
    setConfig((c) => ({
      ...c,
      destinations: c.destinations.map((d, i) =>
        i === idx ? { ...d, ...patch } : d
      ),
    }));

  const addDest = () =>
    setConfig((c) => ({
      ...c,
      destinations: [...c.destinations, defaultDestination()],
    }));

  const removeDest = (idx: number) =>
    setConfig((c) => ({
      ...c,
      destinations: c.destinations.filter((_, i) => i !== idx),
    }));

  const handleSave = async () => {
    try {
      await invoke("save_config", { config });
      onStatus({ text: "Configuration saved.", err: false });
    } catch (e) {
      onStatus({ text: String(e), err: true });
    }
  };

  const handleStartStop = async () => {
    if (isRunning) {
      await onStop();
    } else {
      await onStart(config);
    }
  };

  // IPv4 vs IPv6 detection for server IP dropdown filtering
  const isIpv6Address = (addr: string) =>
    addr.startsWith("[") || addr.includes(":");

  const sourceIsIpv6 = isIpv6Address(config.source.address.split(":")[0]);
  const filteredIps = localIps.filter((ip) => {
    const ipIsV6 = ip.includes(":");
    return ipIsV6 === sourceIsIpv6 || ip === "0.0.0.0" || ip === "::";
  });

  if (loading) {
    return (
      <div className="flex items-center justify-center h-full text-gray-500">
        Loading...
      </div>
    );
  }

  return (
    <div className="h-full overflow-y-auto px-6 py-4 space-y-6">
      {/* Logo / header */}
      <div className="flex gap-4 items-center">
        <img
          src="/nexthop-logo-small.jpg"
          alt="nexthop"
          className="h-12 w-auto rounded object-contain"
        />
        <div className="flex flex-col justify-between py-0.5">
          <div className="text-xl font-bold flex items-baseline gap-2">
            nexthop
            {version && <span className="text-sm font-normal text-gray-400 dark:text-gray-500">v{version}</span>}
          </div>
          <div className="text-xs text-gray-500 dark:text-gray-400">
            TCP/UDP relay — unicast · multicast · broadcast
          </div>
          <div className="text-xs text-gray-500 dark:text-gray-400">
            Copyright (C) 2026-present Patrick S Connallon
          </div>
          <div className="text-xs text-gray-500 dark:text-gray-400">
            GPL-3.0-or-later
          </div>
        </div>
      </div>

      {/* ── Data Source ── */}
      <section>
        <SectionHeader>Data Source</SectionHeader>
        <div className="space-y-3">
          <Row>
            <Label>Cast Mode</Label>
            <Select
              value={config.source.cast_mode}
              options={SOURCE_CAST_MODES}
              onChange={(v) => {
                const patch: Partial<typeof config.source> = { cast_mode: v };
                if (v !== "unicast") patch.protocol = "udp";
                if (v === "multicast") patch.mode = "server";
                updateSource(patch);
              }}
            />
            <Label>Protocol</Label>
            <Select
              value={config.source.protocol}
              options={PROTOCOLS}
              onChange={(v) => updateSource({ protocol: v })}
              disabled={config.source.cast_mode !== "unicast"}
            />
            <Label>Mode</Label>
            <Select
              value={config.source.mode}
              options={MODES}
              onChange={(v) => updateSource({ mode: v })}
              disabled={config.source.cast_mode === "multicast"}
              labels={
                config.source.cast_mode === "multicast"
                  ? MULTICAST_MODE_LABELS
                  : undefined
              }
            />
          </Row>
          <Row>
            <Label>Address</Label>
            {config.source.cast_mode === "multicast" ? (
              <input
                type="text"
                value={(() => { const p = config.source.address.split(":"); return p.slice(0, -1).join(":") || p[0]; })()}
                onChange={(e) => {
                  const port = config.source.address.split(":").pop() ?? "5000";
                  updateSource({ address: `${e.target.value}:${port}` });
                }}
                placeholder="224.0.0.x"
                className={`border rounded px-2 py-1 text-sm bg-white dark:bg-gray-800 w-48 ${
                  (() => {
                    const p = config.source.address.split(":");
                    const h = p.slice(0, -1).join(":") || p[0];
                    return h && !/^(22[4-9]|23[0-9])(\.\d{1,3}){3}$/.test(h)
                      ? "border-red-400 dark:border-red-500"
                      : "border-gray-300 dark:border-gray-600";
                  })()
                }`}
              />
            ) : config.source.mode === "server" ? (
              <select
                value={(() => {
                  const p = config.source.address.split(":");
                  return p.slice(0, -1).join(":") || p[0];
                })()}
                onChange={(e) => {
                  const port =
                    config.source.address.split(":").pop() ?? "5000";
                  updateSource({ address: `${e.target.value}:${port}` });
                }}
                className="border border-gray-300 dark:border-gray-600 rounded px-2 py-1 text-sm bg-white dark:bg-gray-800"
              >
                {filteredIps.map((ip) => (
                  <option key={ip} value={ip}>
                    {ip}
                  </option>
                ))}
              </select>
            ) : (
              <TextInput
                value={(() => {
                  const p = config.source.address.split(":");
                  return p.slice(0, -1).join(":") || p[0];
                })()}
                onChange={(v) => {
                  const port = config.source.address.split(":").pop() ?? "5000";
                  updateSource({ address: `${v}:${port}` });
                }}
                placeholder="host"
                className="w-48"
              />
            )}
            <Label>Port</Label>
            <input
              type="number"
              min={1}
              max={65535}
              value={config.source.address.split(":").pop() ?? "5000"}
              onChange={(e) => {
                const p = config.source.address.split(":");
                const host = p.slice(0, -1).join(":") || p[0];
                updateSource({ address: `${host}:${e.target.value}` });
              }}
              className="border border-gray-300 dark:border-gray-600 rounded px-2 py-1 text-sm bg-white dark:bg-gray-800 w-24"
            />
            <Label>Name</Label>
            <TextInput
              value={config.source.name ?? ""}
              onChange={(v) => updateSource({ name: v || null })}
              placeholder="optional"
              className="w-32"
            />
          </Row>
          {config.source.cast_mode === "multicast" && (
            <Row>
              <Label>Multicast Interface</Label>
              <select
                value={config.source.multicast_interface ?? ""}
                onChange={(e) =>
                  updateSource({ multicast_interface: e.target.value || null })
                }
                className="border border-gray-300 dark:border-gray-600 rounded px-2 py-1 text-sm bg-white dark:bg-gray-800"
              >
                <option value="">— default —</option>
                {localIps
                  .filter((ip) => ip !== "0.0.0.0" && ip !== "::")
                  .map((ip) => (
                    <option key={ip} value={ip}>{ip}</option>
                  ))}
              </select>
              <Label>TTL</Label>
              <input
                type="number"
                min={1}
                max={255}
                value={config.source.multicast_ttl}
                onChange={(e) =>
                  updateSource({ multicast_ttl: Number(e.target.value) })
                }
                className="border border-gray-300 dark:border-gray-600 rounded px-2 py-1 text-sm bg-white dark:bg-gray-800 w-16"
              />
            </Row>
          )}
          {config.source.mode === "client" && (
            <Row>
              <Label>Reconnect Delay (ms)</Label>
              <NumberInput
                value={config.source.reconnect_delay_ms}
                onChange={(v) => updateSource({ reconnect_delay_ms: v })}
                min={100}
                className="w-24"
              />
            </Row>
          )}
        </div>
      </section>

      {/* ── Data Destinations ── */}
      <section>
        <SectionHeader>Data Destinations</SectionHeader>
        <div className="overflow-x-auto">
          <table className="text-sm border-collapse w-full min-w-max">
            <thead>
              <tr className="text-gray-500 dark:text-gray-400 text-xs uppercase tracking-wide">
                <th className="text-left px-2 py-1">Cast</th>
                <th className="text-left px-2 py-1">Mode</th>
                <th className="text-left px-2 py-1">Protocol</th>
                <th className="text-left px-2 py-1">Address</th>
                <th className="text-left px-2 py-1">Port</th>
                <th className="text-left px-2 py-1">Overflow</th>
                <th className="text-left px-2 py-1">Name</th>
                <th className="px-2 py-1"></th>
              </tr>
            </thead>
            <tbody>
              {config.destinations.map((dest, idx) => {
                const addrParts = dest.address.split(":");
                const host = addrParts.slice(0, -1).join(":") || addrParts[0];
                const port = addrParts[addrParts.length - 1] ?? "5001";
                return (
                  <tr
                    key={idx}
                    className="border-t border-gray-100 dark:border-gray-800"
                  >
                    <td className="px-2 py-1">
                      <Select
                        value={dest.cast_mode}
                        options={CAST_MODES}
                        onChange={(v) => {
                          const patch: Partial<DestConfig> = { cast_mode: v };
                          if (v !== "unicast") patch.protocol = "udp";
                          updateDest(idx, patch);
                        }}
                      />
                    </td>
                    <td className="px-2 py-1">
                      <Select
                        value={dest.mode}
                        options={MODES}
                        onChange={(v) => updateDest(idx, { mode: v })}
                        labels={
                          dest.cast_mode === "multicast"
                            ? MULTICAST_MODE_LABELS
                            : undefined
                        }
                      />
                    </td>
                    <td className="px-2 py-1">
                      <Select
                        value={dest.protocol}
                        options={PROTOCOLS}
                        onChange={(v) => updateDest(idx, { protocol: v })}
                        disabled={dest.cast_mode !== "unicast"}
                      />
                    </td>
                    <td className="px-2 py-1">
                      {dest.cast_mode === "multicast" ? (
                        <input
                          type="text"
                          value={host}
                          onChange={(e) =>
                            updateDest(idx, { address: `${e.target.value}:${port}` })
                          }
                          placeholder="224.0.0.x"
                          className={`border rounded px-2 py-1 text-sm bg-white dark:bg-gray-800 w-36 ${
                            host && !/^(22[4-9]|23[0-9])(\.\d{1,3}){3}$/.test(host)
                              ? "border-red-400 dark:border-red-500"
                              : "border-gray-300 dark:border-gray-600"
                          }`}
                        />
                      ) : dest.mode === "server" && dest.cast_mode === "broadcast" ? (
                        <select
                          value={host}
                          onChange={(e) =>
                            updateDest(idx, { address: `${e.target.value}:${port}` })
                          }
                          className="border border-gray-300 dark:border-gray-600 rounded px-2 py-1 text-sm bg-white dark:bg-gray-800 w-36"
                        >
                          {broadcastIps.map((ip) => (
                            <option key={ip} value={ip}>{ip}</option>
                          ))}
                        </select>
                      ) : dest.mode === "server" || dest.cast_mode === "broadcast" ? (
                        <select
                          value={host}
                          onChange={(e) =>
                            updateDest(idx, { address: `${e.target.value}:${port}` })
                          }
                          className="border border-gray-300 dark:border-gray-600 rounded px-2 py-1 text-sm bg-white dark:bg-gray-800 w-36"
                        >
                          {localIps.map((ip) => (
                            <option key={ip} value={ip}>{ip}</option>
                          ))}
                        </select>
                      ) : (
                        <TextInput
                          value={host}
                          onChange={(v) =>
                            updateDest(idx, { address: `${v}:${port}` })
                          }
                          placeholder="host"
                          className="w-36"
                        />
                      )}
                    </td>
                    <td className="px-2 py-1">
                      <input
                        type="number"
                        min={1}
                        max={65535}
                        value={port}
                        onChange={(e) =>
                          updateDest(idx, {
                            address: `${host}:${e.target.value}`,
                          })
                        }
                        className="border border-gray-300 dark:border-gray-600 rounded px-2 py-1 text-sm bg-white dark:bg-gray-800 w-20"
                      />
                    </td>
                    <td className="px-2 py-1">
                      <Select
                        value={dest.overflow_policy}
                        options={OVERFLOW_POLICIES}
                        onChange={(v) =>
                          updateDest(idx, { overflow_policy: v })
                        }
                      />
                    </td>
                    <td className="px-2 py-1">
                      <TextInput
                        value={dest.name ?? ""}
                        onChange={(v) =>
                          updateDest(idx, { name: v || null })
                        }
                        placeholder="optional"
                        className="w-28"
                      />
                    </td>
                    <td className="px-2 py-1">
                      <button
                        onClick={() => removeDest(idx)}
                        className="text-red-500 hover:text-red-700 font-bold px-1"
                        title="Remove destination"
                      >
                        ✕
                      </button>
                    </td>
                  </tr>
                );
              })}
            </tbody>
          </table>
        </div>
        <button
          onClick={addDest}
          className="mt-2 text-sm text-blue-600 dark:text-blue-400 hover:underline"
        >
          + Add Destination
        </button>

        {/* Advanced per-destination settings (multicast / reconnect) */}
        {config.destinations.some(
          (d) =>
            d.cast_mode === "multicast" ||
            (d.mode === "client" && d.reconnect_delay_ms !== null)
        ) && (
          <details className="mt-3">
            <summary className="text-sm text-gray-500 dark:text-gray-400 cursor-pointer hover:text-gray-700 dark:hover:text-gray-200">
              Advanced Destination Settings
            </summary>
            <div className="mt-2 space-y-3 pl-2">
              {config.destinations.map((dest, idx) =>
                dest.cast_mode === "multicast" || dest.mode === "client" ? (
                  <div
                    key={idx}
                    className="space-y-2 text-sm border-l-2 border-gray-200 dark:border-gray-700 pl-3"
                  >
                    <div className="font-medium text-gray-600 dark:text-gray-400">
                      Dest {idx + 1}: {dest.address}
                    </div>
                    {dest.cast_mode === "multicast" && (
                      <Row>
                        <Label>Multicast Interface</Label>
                        <select
                          value={dest.multicast_interface ?? ""}
                          onChange={(e) =>
                            updateDest(idx, {
                              multicast_interface: e.target.value || null,
                            })
                          }
                          className="border border-gray-300 dark:border-gray-600 rounded px-2 py-1 text-sm bg-white dark:bg-gray-800"
                        >
                          <option value="">— default —</option>
                          {localIps
                            .filter((ip) => ip !== "0.0.0.0" && ip !== "::")
                            .map((ip) => (
                              <option key={ip} value={ip}>{ip}</option>
                            ))}
                        </select>
                        <Label>TTL</Label>
                        <input
                          type="number"
                          min={1}
                          max={255}
                          value={dest.multicast_ttl}
                          onChange={(e) =>
                            updateDest(idx, {
                              multicast_ttl: Number(e.target.value),
                            })
                          }
                          className="border border-gray-300 dark:border-gray-600 rounded px-2 py-1 text-sm bg-white dark:bg-gray-800 w-16"
                        />
                      </Row>
                    )}
                    {dest.mode === "client" && (
                      <Row>
                        <Label>Reconnect Delay (ms)</Label>
                        <NumberInput
                          value={dest.reconnect_delay_ms}
                          onChange={(v) =>
                            updateDest(idx, { reconnect_delay_ms: v })
                          }
                          min={100}
                          className="w-24"
                        />
                      </Row>
                    )}
                  </div>
                ) : null
              )}
            </div>
          </details>
        )}
      </section>

      {/* ── General Settings ── */}
      <section>
        <SectionHeader>General Settings</SectionHeader>
        <div className="space-y-3">
          <Row>
            <Label>Log Level</Label>
            <Select
              value={config.general.log_level as (typeof LOG_LEVELS)[number]}
              options={LOG_LEVELS as unknown as string[]}
              onChange={(v) => updateGeneral({ log_level: v })}
            />
            <Label>Stats Interval (s)</Label>
            <input
              type="number"
              min={1}
              value={config.general.stats_interval_secs}
              onChange={(e) =>
                updateGeneral({ stats_interval_secs: Number(e.target.value) })
              }
              className="border border-gray-300 dark:border-gray-600 rounded px-2 py-1 text-sm bg-white dark:bg-gray-800 w-20"
            />
          </Row>
          <Row>
            <Label>Channel Capacity</Label>
            <input
              type="number"
              min={1}
              value={config.general.channel_capacity}
              onChange={(e) =>
                updateGeneral({ channel_capacity: Number(e.target.value) })
              }
              className="border border-gray-300 dark:border-gray-600 rounded px-2 py-1 text-sm bg-white dark:bg-gray-800 w-24"
            />
            <Label>Max Payload (bytes)</Label>
            <input
              type="number"
              min={1}
              value={config.general.max_payload_size}
              onChange={(e) =>
                updateGeneral({ max_payload_size: Number(e.target.value) })
              }
              className="border border-gray-300 dark:border-gray-600 rounded px-2 py-1 text-sm bg-white dark:bg-gray-800 w-24"
            />
            <Label>Health Port</Label>
            <NumberInput
              value={config.general.health_port}
              onChange={(v) => updateGeneral({ health_port: v })}
              min={1}
              className="w-20"
            />
          </Row>
        </div>
      </section>

      {/* ── Rate Limit ── */}
      <section>
        <SectionHeader>Rate Limit</SectionHeader>
        <div className="space-y-3">
          <Row>
            <label className="flex items-center gap-2 text-sm">
              <input
                type="checkbox"
                checked={config.rate_limit != null}
                onChange={(e) =>
                  setConfig((c) => ({
                    ...c,
                    rate_limit: e.target.checked
                      ? { bytes_per_second: 10_000_000, burst_size: 131_072 }
                      : null,
                  }))
                }
              />
              Enable rate limiting
            </label>
          </Row>
          {config.rate_limit != null && (
            <Row>
              <Label>Bytes/sec</Label>
              <input
                type="number"
                min={1}
                value={config.rate_limit.bytes_per_second}
                onChange={(e) =>
                  setConfig((c) => ({
                    ...c,
                    rate_limit: c.rate_limit
                      ? {
                          ...c.rate_limit,
                          bytes_per_second: Number(e.target.value),
                        }
                      : null,
                  }))
                }
                className="border border-gray-300 dark:border-gray-600 rounded px-2 py-1 text-sm bg-white dark:bg-gray-800 w-32"
              />
              <Label>Burst Size (bytes)</Label>
              <input
                type="number"
                min={1}
                value={config.rate_limit.burst_size}
                onChange={(e) =>
                  setConfig((c) => ({
                    ...c,
                    rate_limit: c.rate_limit
                      ? {
                          ...c.rate_limit,
                          burst_size: Number(e.target.value),
                        }
                      : null,
                  }))
                }
                className="border border-gray-300 dark:border-gray-600 rounded px-2 py-1 text-sm bg-white dark:bg-gray-800 w-32"
              />
            </Row>
          )}
        </div>
      </section>

      {/* ── Action bar ── */}
      <div className="flex items-center gap-3 pb-4">
        <button
          onClick={handleSave}
          disabled={isRunning}
          title={isRunning ? "Stop relay before saving configuration" : ""}
          className="px-4 py-2 text-sm rounded bg-gray-100 dark:bg-gray-700 hover:bg-gray-200 dark:hover:bg-gray-600 disabled:opacity-40 disabled:cursor-not-allowed"
        >
          Save Configuration
        </button>
        <button
          onClick={handleStartStop}
          className={`px-5 py-2 text-sm rounded font-medium text-white ${
            isRunning
              ? "bg-red-600 hover:bg-red-700"
              : "bg-green-600 hover:bg-green-700"
          }`}
        >
          {isRunning ? "■ Stop" : "▶ Start"}
        </button>
      </div>
    </div>
  );
}
