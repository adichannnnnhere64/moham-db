import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import type { SyncConfig } from "../App";

type SyncStatus = {
  running: boolean;
  nextSyncSecs: number | null;
  lastSyncMs: number | null;
  lastError: string | null;
};

type LogEntry = {
  id: number;
  timestampMs: number;
  level: "INFO" | "SUCCESS" | "ERROR";
  message: string;
};

type TableSyncState = {
  remoteTable: string;
  localTable: string;
  status: "Success" | "Error" | "Pending";
  ago: string;
};

type Props = { config: SyncConfig };

export default function Dashboard({ config }: Props) {
  const [status, setStatus] = useState<SyncStatus>({
    running: false,
    nextSyncSecs: null,
    lastSyncMs: null,
    lastError: null,
  });
  const [logs, setLogs] = useState<LogEntry[]>([]);
  const [tableSyncStates, setTableSyncStates] = useState<TableSyncState[]>([]);
  const [localOk, setLocalOk] = useState<boolean | null>(null);
  const [remoteOk, setRemoteOk] = useState<boolean | null>(null);
  const [refreshing, setRefreshing] = useState(false);

  useEffect(() => {
    let mounted = true;

    async function poll() {
      try {
        const [s, l] = await Promise.all([
          invoke<SyncStatus>("sync_status"),
          invoke<LogEntry[]>("activity_logs"),
        ]);
        if (!mounted) return;
        setStatus(s);
        setLogs(l.slice(0, 8));
      } catch {
        // ignore polling errors
      }
    }

    poll();
    const id = window.setInterval(poll, 1000);
    return () => {
      mounted = false;
      window.clearInterval(id);
    };
  }, []);

  useEffect(() => {
    async function checkConnections() {
      if (!config.localDb.host) return;
      try {
        const [lr, rr] = await Promise.all([
          invoke<{ ok: boolean }>("test_connection", { dbConfig: config.localDb }),
          invoke<{ ok: boolean }>("test_connection", { dbConfig: config.remoteDb }),
        ]);
        setLocalOk(lr.ok);
        setRemoteOk(rr.ok);
      } catch {
        // ignore
      }
    }
    checkConnections();
  }, [config.localDb, config.remoteDb]);

  useEffect(() => {
    const successLogs = logs.filter((l) => l.level === "SUCCESS");
    const states = config.tableMappings.map((m) => {
      const match = successLogs.find((l) =>
        l.message.includes(`'${m.remoteTable}'`)
      );
      const errMatch = logs.find(
        (l) => l.level === "ERROR" && l.message.includes(m.remoteTable)
      );
      if (match) {
        const secs = Math.round((Date.now() - match.timestampMs) / 1000);
        const ago = secs < 60 ? `${secs} secs ago` : `${Math.round(secs / 60)} mins ago`;
        return { ...m, status: "Success" as const, ago: `Success (${ago})` };
      }
      if (errMatch) return { ...m, status: "Error" as const, ago: "Error" };
      return { ...m, status: "Pending" as const, ago: "Pending" };
    });
    setTableSyncStates(states);
  }, [logs, config.tableMappings]);

  async function handleRefresh() {
    setRefreshing(true);
    try {
      await invoke("sync_now", { config });
    } catch {
      // error will appear in logs
    } finally {
      setRefreshing(false);
    }
  }

  function formatCountdown(secs: number | null): string {
    if (secs === null) return "—";
    const m = Math.floor(secs / 60);
    const s = secs % 60;
    return `${m.toString().padStart(2, "0")}:${s.toString().padStart(2, "0")} Minutes`;
  }

  function formatLog(entry: LogEntry): string {
    const d = new Date(entry.timestampMs);
    const ts = d.toISOString().replace("T", " ").slice(0, 19);
    return `[${ts}] ${entry.level}: ${entry.message}`;
  }

  return (
    <>
      <p className="page-title">Database Sync Engine v1.0</p>

      <div className="dash-top">
        <div className="card">
          <div className="card-title">Connection Status</div>
          <div
            className={`status-line ${localOk === true ? "ok" : localOk === false ? "err" : "pending"}`}
          >
            {localOk === true
              ? "Local DB Connected"
              : localOk === false
                ? "Local DB: Connection Failed"
                : "Local DB: Not tested"}
          </div>
          <div
            className={`status-line ${remoteOk === true ? "ok" : remoteOk === false ? "err" : "pending"}`}
          >
            {remoteOk === true
              ? `Remote DB (${config.remoteDb.host}): Connected`
              : remoteOk === false
                ? `Remote DB (${config.remoteDb.host || "—"}): Failed`
                : "Remote DB: Not tested"}
          </div>
        </div>

        <div className="card">
          <div className="card-title">Next Automatic Sync</div>
          <div className="countdown">
            {status.running
              ? formatCountdown(status.nextSyncSecs)
              : "Sync not running"}
          </div>
          <button
            className="btn-refresh"
            onClick={handleRefresh}
            disabled={refreshing}
          >
            {refreshing ? "SYNCING..." : "REFRESH NOW"}
          </button>
        </div>
      </div>

      <div className="card" style={{ marginBottom: 20 }}>
        <div className="section-heading">
          <h2>Active Table Mappings</h2>
        </div>
        {config.tableMappings.length === 0 ? (
          <p className="empty">No table mappings configured.</p>
        ) : (
          <table className="data-table">
            <thead>
              <tr>
                <th>Remote Table</th>
                <th>Local Table</th>
                <th>Last Sync Status</th>
              </tr>
            </thead>
            <tbody>
              {tableSyncStates.map((m, i) => (
                <tr key={i}>
                  <td>{m.remoteTable}</td>
                  <td>{m.localTable}</td>
                  <td>
                    <span
                      className={
                        m.status === "Success"
                          ? "sync-ok"
                          : m.status === "Error"
                            ? "sync-err"
                            : "sync-pending"
                      }
                    >
                      {m.ago}
                    </span>
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        )}
      </div>

      <div className="card">
        <div className="card-title">Recent Logs</div>
        <div className="log-terminal">
          {logs.length === 0 ? (
            <span style={{ color: "#6b7a99" }}>No logs yet.</span>
          ) : (
            logs.map((e) => (
              <div key={e.id} className={`log-entry ${e.level}`}>
                {formatLog(e)}
              </div>
            ))
          )}
        </div>
      </div>
    </>
  );
}
