import { invoke } from "@tauri-apps/api/core";
import type { StatusMsg } from "../App";

interface PrefsTabProps {
  darkMode: boolean;
  onDarkModeChange: (v: boolean) => void;
  onStatus: (msg: StatusMsg | null) => void;
}

export default function PrefsTab({
  darkMode,
  onDarkModeChange,
  onStatus,
}: PrefsTabProps) {
  const handleSave = async () => {
    try {
      await invoke("save_prefs", { prefs: { dark_mode: darkMode } });
      onStatus({ text: "Preferences saved.", err: false });
    } catch (e) {
      onStatus({ text: String(e), err: true });
    }
  };

  return (
    <div className="h-full overflow-y-auto px-6 py-4 space-y-6">
      <h1 className="text-lg font-semibold">Preferences</h1>

      <section className="space-y-3">
        <h2 className="text-base font-medium text-gray-700 dark:text-gray-200">
          Appearance
        </h2>
        <div className="flex items-center gap-4">
          <span className="text-sm text-gray-600 dark:text-gray-400">Theme</span>
          <div className="flex rounded overflow-hidden border border-gray-300 dark:border-gray-600">
            <button
              onClick={() => onDarkModeChange(false)}
              className={`px-4 py-1.5 text-sm ${
                !darkMode
                  ? "bg-blue-600 text-white"
                  : "bg-white dark:bg-gray-800 text-gray-700 dark:text-gray-300 hover:bg-gray-50 dark:hover:bg-gray-700"
              }`}
            >
              ☀ Light
            </button>
            <button
              onClick={() => onDarkModeChange(true)}
              className={`px-4 py-1.5 text-sm border-l border-gray-300 dark:border-gray-600 ${
                darkMode
                  ? "bg-blue-600 text-white"
                  : "bg-white dark:bg-gray-800 text-gray-700 dark:text-gray-300 hover:bg-gray-50 dark:hover:bg-gray-700"
              }`}
            >
              ☾ Dark
            </button>
          </div>
        </div>
      </section>

      <button
        onClick={handleSave}
        className="px-4 py-2 text-sm rounded bg-blue-600 hover:bg-blue-700 text-white font-medium"
      >
        Save Preferences
      </button>
    </div>
  );
}
