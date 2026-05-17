import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";

type LogLevel = "INFO" | "SUCCESS" | "ERROR";

type LogEntry = {
  id: number;
  timestampMs: number;
  level: LogLevel;
  message: string;
};

const LEVELS: (LogLevel | "ALL")[] = ["ALL", "INFO", "SUCCESS", "ERROR"];

export default function ActivityLogs() {
  const [logs, setLogs] = useState<LogEntry[]>([]);
  const [filter, setFilter] = useState<LogLevel | "ALL">("ALL");

  useEffect(() => {
    let mounted = true;
    async function poll() {
      try {
        const l = await invoke<LogEntry[]>("activity_logs");
        if (mounted) setLogs(l);
      } catch {
        // ignore
      }
    }
    poll();
    const id = window.setInterval(poll, 1000);
    return () => {
      mounted = false;
      window.clearInterval(id);
    };
  }, []);

  async function handleClear() {
    try {
      await invoke("clear_logs");
      setLogs([]);
    } catch {
      // ignore
    }
  }

  const visible = filter === "ALL" ? logs : logs.filter((l) => l.level === filter);

  function formatLog(entry: LogEntry): string {
    const d = new Date(entry.timestampMs);
    const ts = d.toISOString().replace("T", " ").slice(0, 19);
    return `[${ts}] ${entry.level}: ${entry.message}`;
  }

  return (
    <>
      <p className="page-title">Activity Logs</p>
      <div className="card">
        <div className="section-heading">
          <div className="log-filters">
            {LEVELS.map((l) => (
              <button
                key={l}
                className={`filter-btn${filter === l ? " active" : ""}`}
                onClick={() => setFilter(l)}
              >
                {l}
              </button>
            ))}
          </div>
          <button className="btn btn-secondary" onClick={handleClear} style={{ fontSize: 12 }}>
            Clear Logs
          </button>
        </div>

        <div className="log-terminal" style={{ maxHeight: 480 }}>
          {visible.length === 0 ? (
            <span style={{ color: "#6b7a99" }}>No logs.</span>
          ) : (
            visible.map((e) => (
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
