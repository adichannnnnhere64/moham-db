import { useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import type { SyncConfig } from "../App";

type SyncStatus = {
  running: boolean;
  nextSyncSecs: number | null;
  lastSyncMs: number | null;
  lastError: string | null;
};

type Props = {
  config: SyncConfig;
  setConfig: React.Dispatch<React.SetStateAction<SyncConfig>>;
};

export default function SyncSettings({ config, setConfig }: Props) {
  const [status, setStatus] = useState<SyncStatus | null>(null);
  const [busy, setBusy] = useState(false);
  const [message, setMessage] = useState<string | null>(null);

  async function handleStart() {
    setBusy(true);
    setMessage(null);
    try {
      const s = await invoke<SyncStatus>("start_sync", { config });
      setStatus(s);
      setMessage("Sync engine started.");
    } catch (e) {
      setMessage(String(e));
    } finally {
      setBusy(false);
    }
  }

  async function handleStop() {
    setBusy(true);
    try {
      const s = await invoke<SyncStatus>("stop_sync");
      setStatus(s);
      setMessage("Sync engine stopped.");
    } catch (e) {
      setMessage(String(e));
    } finally {
      setBusy(false);
    }
  }

  async function handleSave() {
    setBusy(true);
    try {
      await invoke("save_settings", { config });
      setMessage("Settings saved.");
    } catch (e) {
      setMessage(String(e));
    } finally {
      setBusy(false);
    }
  }

  const running = status?.running ?? false;

  return (
    <>
      <p className="page-title">Sync Settings</p>
      <div className="card">
        <div className="card-title">Interval Configuration</div>
        <div className="interval-row">
          <label>Sync every</label>
          <input
            type="number"
            min="0"
            max="1440"
            value={config.intervalMinutes}
            onChange={(e) =>
              setConfig((c) => ({ ...c, intervalMinutes: Number(e.currentTarget.value) }))
            }
          />
          <label>minutes (0 = manual only)</label>
        </div>

        <div className="btn-row">
          <button className="btn btn-primary" onClick={handleStart} disabled={busy}>
            {running ? "Restart Sync" : "Start Sync"}
          </button>
          <button className="btn btn-danger" onClick={handleStop} disabled={busy || !running}>
            Stop Sync
          </button>
          <button className="btn btn-secondary" onClick={handleSave} disabled={busy}>
            Save Settings
          </button>
        </div>

        {message && (
          <p style={{ marginTop: 14, fontSize: 13, color: "var(--text-muted)" }}>{message}</p>
        )}

        {status && (
          <div style={{ marginTop: 20, fontSize: 13, color: "var(--text-muted)" }}>
            Status: <strong style={{ color: running ? "var(--green)" : "var(--red)" }}>
              {running ? "Running" : "Stopped"}
            </strong>
            {status.nextSyncSecs !== null && running && (
              <span> — next sync in {Math.round(status.nextSyncSecs / 60)} min</span>
            )}
          </div>
        )}
      </div>
    </>
  );
}
