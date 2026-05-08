import { useCallback, useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { getVersion } from "@tauri-apps/api/app";
import { getCurrentWindow } from "@tauri-apps/api/window";
import ConfigTab from "./components/ConfigTab";
import MonitorTab from "./components/MonitorTab";
import PrefsTab from "./components/PrefsTab";
import type { Prefs, RelayConfig } from "./types";

type Tab = "config" | "monitor" | "prefs";

export interface StatusMsg {
  text: string;
  err: boolean;
}

export default function App() {
  const [tab, setTab] = useState<Tab>("config");
  const [isRunning, setIsRunning] = useState(false);
  const [statusMsg, setStatusMsg] = useState<StatusMsg | null>(null);
  const [darkMode, setDarkMode] = useState(true);
  const [localIps, setLocalIps] = useState<string[]>([]);
  const [broadcastIps, setBroadcastIps] = useState<string[]>([]);
  const [version, setVersion] = useState("");

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

  // Poll relay status every 2 seconds to detect unexpected stops
  useEffect(() => {
    const id = setInterval(() => {
      invoke<boolean>("get_relay_status")
        .then((running) => {
          setIsRunning((prev) => {
            if (prev && !running) {
              setStatusMsg({ text: "Relay stopped.", err: false });
            }
            return running;
          });
        })
        .catch(console.error);
    }, 2000);
    return () => clearInterval(id);
  }, []);

  const handleStart = useCallback(async (config: RelayConfig) => {
    try {
      await invoke("start_relay", { config });
      setIsRunning(true);
      setStatusMsg({ text: "Relay started.", err: false });
    } catch (e) {
      setStatusMsg({ text: String(e), err: true });
    }
  }, []);

  const handleStop = useCallback(async () => {
    try {
      await invoke("stop_relay");
      setIsRunning(false);
      setStatusMsg({ text: "Relay stopped.", err: false });
    } catch (e) {
      setStatusMsg({ text: String(e), err: true });
    }
  }, []);

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
