import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import type { StatsPayload, StatsSnapshot } from "../types";

interface MonitorTabProps {
  isRunning: boolean;
}

function fmtBytes(n: number): string {
  if (n >= 1_000_000_000) return `${(n / 1_000_000_000).toFixed(2)} GB`;
  if (n >= 1_000_000) return `${(n / 1_000_000).toFixed(2)} MB`;
  if (n >= 1_000) return `${(n / 1_000).toFixed(2)} KB`;
  return `${n} B`;
}

function fmtUptime(s: number): string {
  const h = Math.floor(s / 3600);
  const m = Math.floor((s % 3600) / 60);
  const sec = s % 60;
  return `${h}h ${String(m).padStart(2, "0")}m ${String(sec).padStart(2, "0")}s`;
}

function StatRow({
  label,
  value,
  colorClass,
}: {
  label: string;
  value: string;
  colorClass?: string;
}) {
  return (
    <div className="flex justify-between text-sm py-0.5">
      <span className="text-gray-500 dark:text-gray-400">{label}</span>
      <span className={`font-mono font-medium ${colorClass ?? "text-gray-800 dark:text-gray-100"}`}>
        {value}
      </span>
    </div>
  );
}

function StatsCard({
  snap,
  isSource,
}: {
  snap: StatsSnapshot;
  isSource: boolean;
}) {
  const headerColor = isSource
    ? "bg-blue-700 dark:bg-blue-800"
    : "bg-green-700 dark:bg-green-800";

  return (
    <div className="rounded border border-gray-200 dark:border-gray-700 overflow-hidden">
      <div className={`${headerColor} text-white px-3 py-1.5 text-sm font-semibold`}>
        {snap.label}
      </div>
      <div className="px-3 py-2 space-y-0.5 bg-gray-50 dark:bg-gray-800">
        <div className="flex items-center gap-1 text-xs font-mono text-gray-400 dark:text-gray-500 pb-1 border-b border-gray-200 dark:border-gray-700">
          <span>{snap.local_addr}</span>
          <span>↔</span>
          <span>{snap.peer_addr}</span>
        </div>
        {isSource ? (
          <StatRow label="RX" value={fmtBytes(snap.rx_bytes)} />
        ) : (
          <StatRow label="TX" value={fmtBytes(snap.tx_bytes)} />
        )}
        <StatRow label="Messages" value={String(snap.messages)} />
        <StatRow
          label="Active Conns"
          value={String(snap.active_connections)}
          colorClass={
            snap.active_connections > 0
              ? "text-green-600 dark:text-green-400"
              : "text-gray-500"
          }
        />
        <StatRow label="Total Conns" value={String(snap.total_connections)} />
        <StatRow
          label="Errors"
          value={String(snap.errors)}
          colorClass={
            snap.errors > 0
              ? "text-red-600 dark:text-red-400"
              : "text-gray-500"
          }
        />
        {!isSource && (
          <>
            <StatRow
              label="Dropped"
              value={String(snap.dropped)}
              colorClass={
                snap.dropped > 0
                  ? "text-yellow-600 dark:text-yellow-400"
                  : "text-gray-500"
              }
            />
            {snap.dropped > 0 && (
              <div className="pl-4 text-xs space-y-0.5">
                {snap.dropped_overflow > 0 && (
                  <StatRow label="↳ overflow" value={String(snap.dropped_overflow)} />
                )}
                {snap.dropped_oversize > 0 && (
                  <StatRow label="↳ oversize" value={String(snap.dropped_oversize)} />
                )}
                {snap.dropped_validation > 0 && (
                  <StatRow label="↳ validation" value={String(snap.dropped_validation)} />
                )}
                {snap.dropped_write_error > 0 && (
                  <StatRow label="↳ write error" value={String(snap.dropped_write_error)} />
                )}
              </div>
            )}
          </>
        )}
        <StatRow label="Uptime" value={fmtUptime(snap.uptime_s)} />
      </div>
    </div>
  );
}

export default function MonitorTab({ isRunning }: MonitorTabProps) {
  const [stats, setStats] = useState<StatsPayload | null>(null);
  const [refreshInterval, setRefreshInterval] = useState(1);

  useEffect(() => {
    if (!isRunning) {
      setStats(null);
      return;
    }
    const poll = () => {
      invoke<StatsPayload>("get_stats")
        .then(setStats)
        .catch(console.error);
    };
    poll();
    const id = setInterval(poll, refreshInterval * 1000);
    return () => clearInterval(id);
  }, [isRunning, refreshInterval]);

  return (
    <div className="h-full overflow-y-auto px-6 py-4 space-y-4">
      <div className="flex items-center gap-4">
        <h1 className="text-lg font-semibold">Monitoring</h1>
        <div className="flex items-center gap-2 ml-auto text-sm text-gray-600 dark:text-gray-400">
          <label htmlFor="refresh-interval">Refresh (s):</label>
          <input
            id="refresh-interval"
            type="range"
            min={0.1}
            max={60}
            step={0.1}
            value={refreshInterval}
            onChange={(e) => setRefreshInterval(Number(e.target.value))}
            className="w-32"
          />
          <span className="w-10 text-right font-mono">{refreshInterval.toFixed(1)}</span>
        </div>
      </div>

      {!isRunning && (
        <div className="flex items-center justify-center h-48 text-gray-400 dark:text-gray-500 text-sm">
          Relay is not running.
        </div>
      )}

      {isRunning && stats && (
        <div className="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-3 gap-4">
          <StatsCard snap={stats.source} isSource={true} />
          {stats.destinations.map((d, i) => (
            <StatsCard key={i} snap={d} isSource={false} />
          ))}
        </div>
      )}

      {isRunning && !stats && (
        <div className="flex items-center justify-center h-48 text-gray-400 text-sm">
          Loading stats...
        </div>
      )}
    </div>
  );
}
