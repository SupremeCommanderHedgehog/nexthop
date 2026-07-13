import { useCallback, useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getVersion } from "@tauri-apps/api/app";
import { getCurrentWindow } from "@tauri-apps/api/window";
import ConfigTab from "./components/ConfigTab";
import MonitorTab from "./components/MonitorTab";
import PrefsTab from "./components/PrefsTab";
import type { Prefs, RelayConfig, RelayStoppedEvent } from "./types";

type Tab = "config" | "monitor" | "prefs";

export interface StatusMsg {
  text: string;
  err: boolean;
}

const RELAY_STOPPED_TEXT = "Relay stopped.";

export default function App() {
  const [tab, setTab] = useState<Tab>("config");
  const [isRunning, setIsRunning] = useState(false);
  const [statusMsg, setStatusMsg] = useState<StatusMsg | null>(null);
  const [darkMode, setDarkMode] = useState(true);
  const [localIps, setLocalIps] = useState<string[]>([]);
  const [broadcastIps, setBroadcastIps] = useState<string[]>([]);
  const [version, setVersion] = useState("");

  // Id of the relay run the UI currently reflects. Minted client-side and set
  // synchronously (before the start_relay round-trip) so a fast crash event
  // can't arrive with no id to match. Used to ignore a late `relay-stopped`
  // event from a prior run (e.g. after a quick stop→start). null when idle.
  const currentRunIdRef = useRef<string | null>(null);
  // Set once the user has issued a start/stop, so the mount-time status seed
  // never overwrites a run the user began during its await.
  const userActedRef = useRef(false);
  // Serializes relay start/stop so the backend receives them in order. Two
  // concurrent commands could otherwise apply out of order (e.g. a rapid
  // start→stop where stop lands first) and strand a running relay the UI
  // believes is stopped.
  const relayOpRef = useRef<Promise<void>>(Promise.resolve());

  useEffect(() => {
    document.documentElement.classList.toggle("dark", darkMode);
  }, [darkMode]);

  useEffect(() => {
    invoke<string[]>("get_local_ips").then(setLocalIps).catch(console.error);
    invoke<string[]>("get_broadcast_ips").then(setBroadcastIps).catch(console.error);
    invoke<Prefs>("get_prefs")
      .then((p) => setDarkMode(p.dark_mode))
      .catch(console.error);
    getVersion().then((v) => {
      setVersion(v);
      getCurrentWindow().setTitle(`nexthop v${v}`).catch(console.error);
    }).catch(console.error);
  }, []);

  // The backend pushes a `relay-stopped` event when the relay task exits (crash
  // or graceful stop), so no status polling is needed. Register the listener
  // before the initial status read to avoid missing a stop in the gap.
  useEffect(() => {
    let unlisten: (() => void) | undefined;
    let cancelled = false;

    (async () => {
      unlisten = await listen<RelayStoppedEvent>("relay-stopped", (e) => {
        // Ignore a late event from a prior run (e.g. a quick stop→start left an
        // old task still winding down); only the current run may change the UI.
        if (e.payload.run_id !== currentRunIdRef.current) return;
        currentRunIdRef.current = null;
        setIsRunning(false);
        // Graceful stops are already reported by handleStop; only surface a
        // message for an unexpected (error) exit.
        if (e.payload.error) {
          setStatusMsg({
            text: `Relay stopped unexpectedly: ${e.payload.error}`,
            err: true,
          });
        }
      });
      if (cancelled) {
        unlisten();
        return;
      }
      // Sync the initial state in case the relay was already running on load.
      try {
        const runId = await invoke<string | null>("get_relay_status");
        // Defer to any start/stop the user issued during this await — their
        // handler owns currentRunIdRef in that case.
        if (!userActedRef.current) {
          currentRunIdRef.current = runId;
          setIsRunning(runId !== null);
        }
      } catch (err) {
        console.error(err);
      }
    })();

    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, []);

  // Chain a relay op after any in-flight one so start/stop never overlap.
  const runRelayOp = useCallback((op: () => Promise<void>) => {
    relayOpRef.current = relayOpRef.current.then(op, op);
    return relayOpRef.current;
  }, []);

  const handleStart = useCallback(
    (config: RelayConfig) => {
      // Mark that the user acted synchronously (before the op is scheduled) so
      // the mount-time status seed can never race ahead and clobber this run.
      userActedRef.current = true;
      return runRelayOp(async () => {
        // Ignore a start while a run is already active or starting, so we never
        // clobber its id.
        if (currentRunIdRef.current !== null) return;
        // Mint the run id and claim it BEFORE the round-trip so a crash event
        // for this run is never dropped for want of a matching id.
        const runId = crypto.randomUUID();
        currentRunIdRef.current = runId;
        try {
          await invoke("start_relay", { config, runId });
          // Reflect running only if this run is still current — a crash event
          // may have already fired during the round-trip and cleared the id.
          if (currentRunIdRef.current === runId) {
            setIsRunning(true);
            setStatusMsg({ text: "Relay started.", err: false });
          }
        } catch (e) {
          // Start was rejected (e.g. validation); undo our optimistic claim.
          if (currentRunIdRef.current === runId) {
            currentRunIdRef.current = null;
            setIsRunning(false);
          }
          setStatusMsg({ text: String(e), err: true });
        }
      });
    },
    [runRelayOp]
  );

  const handleStop = useCallback(() => {
    userActedRef.current = true;
    return runRelayOp(async () => {
      currentRunIdRef.current = null;
      try {
        await invoke("stop_relay");
        setIsRunning(false);
        setStatusMsg({ text: RELAY_STOPPED_TEXT, err: false });
      } catch (e) {
        setStatusMsg({ text: String(e), err: true });
      }
    });
  }, [runRelayOp]);

  const tabs: { id: Tab; label: string }[] = [
    { id: "config", label: "⚙ Configuration" },
    { id: "monitor", label: "📊 Monitoring" },
    { id: "prefs", label: "Preferences" },
  ];

  return (
    <div className="flex flex-col h-screen bg-white dark:bg-gray-900 text-gray-900 dark:text-gray-100 select-none">
      {/* Top bar */}
      <div className="flex items-center gap-2 border-b border-gray-200 dark:border-gray-700 px-3 py-2 flex-shrink-0">
        {tabs.map((t) => (
          <button
            key={t.id}
            onClick={() => setTab(t.id)}
            className={`px-3 py-1 rounded text-sm font-medium transition-colors ${
              tab === t.id
                ? "bg-blue-600 text-white"
                : "text-gray-600 dark:text-gray-300 hover:bg-gray-100 dark:hover:bg-gray-800"
            }`}
          >
            {t.label}
          </button>
        ))}
        <div className="ml-auto flex items-center gap-3">
          {isRunning ? (
            <span className="text-green-500 text-sm font-medium">● Running</span>
          ) : (
            <span className="text-gray-400 text-sm font-medium">● Stopped</span>
          )}
        </div>
      </div>

      {/* Tab content */}
      <div className="flex-1 overflow-hidden">
        {tab === "config" && (
          <ConfigTab
            localIps={localIps}
            broadcastIps={broadcastIps}
            isRunning={isRunning}
            onStart={handleStart}
            onStop={handleStop}
            onStatus={setStatusMsg}
            version={version}
          />
        )}
        {tab === "monitor" && <MonitorTab isRunning={isRunning} />}
        {tab === "prefs" && (
          <PrefsTab
            darkMode={darkMode}
            onDarkModeChange={setDarkMode}
            onStatus={setStatusMsg}
          />
        )}
      </div>

      {/* Status bar */}
      {statusMsg && (
        <div
          className={`flex items-center px-4 py-2 border-t text-sm flex-shrink-0 ${
            statusMsg.err
              ? "bg-red-50 dark:bg-red-950 border-red-300 dark:border-red-800 text-red-700 dark:text-red-300"
              : "bg-green-50 dark:bg-green-950 border-green-300 dark:border-green-800 text-green-700 dark:text-green-300"
          }`}
        >
          <span className="flex-1">{statusMsg.text}</span>
          <button
            onClick={() => setStatusMsg(null)}
            className="ml-4 text-gray-400 hover:text-gray-600 dark:hover:text-gray-200 font-bold"
          >
            ✕
          </button>
        </div>
      )}
    </div>
  );
}
