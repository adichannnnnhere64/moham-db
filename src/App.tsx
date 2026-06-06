import { useState, useEffect, useRef } from "react";
import { invoke } from "@tauri-apps/api/core";
import Dashboard from "./pages/Dashboard";
import DBConnections from "./pages/DBConnections";
import TableMapping from "./pages/TableMapping";
import SyncSettings from "./pages/SyncSettings";
import ActivityLogs from "./pages/ActivityLogs";

type Page = "dashboard" | "db-connections" | "table-mapping" | "sync-settings" | "activity-logs";

export type DbConfig = {
  host: string;
  port: number;
  database: string;
  username: string;
  password: string;
};

export type TableMappingItem = {
  remoteTable: string;
  localTable: string;
};

export type SyncConfig = {
  localDb: DbConfig;
  remoteDb: DbConfig;
  tableMappings: TableMappingItem[];
  intervalMinutes: number;
  syncAllTables: boolean;
  createMissingTables: boolean;
};

export const defaultDbConfig = (): DbConfig => ({
  host: "",
  port: 3306,
  database: "",
  username: "",
  password: "",
});

export const defaultConfig = (): SyncConfig => ({
  localDb: { ...defaultDbConfig(), host: "127.0.0.1" },
  remoteDb: defaultDbConfig(),
  tableMappings: [],
  intervalMinutes: 10,
  syncAllTables: false,
  createMissingTables: false,
});

const NAV: { id: Page; label: string }[] = [
  { id: "dashboard", label: "Dashboard" },
  { id: "db-connections", label: "DBConnections" },
  { id: "table-mapping", label: "Table Mapping" },
  { id: "sync-settings", label: "Sync Settings" },
  { id: "activity-logs", label: "Activity Logs" },
];

export default function App() {
  const [page, setPage] = useState<Page>("dashboard");
  const [config, setConfig] = useState<SyncConfig>(defaultConfig());
  const [loaded, setLoaded] = useState(false);
  const hasLoaded = useRef(false);

  useEffect(() => {
    invoke<SyncConfig>("load_settings")
      .then((saved) => {
        // Merge over defaults so older settings.json files missing newer
        // fields still produce a complete config.
        setConfig({ ...defaultConfig(), ...saved });
      })
      .catch(() => {})
      .finally(() => {
        hasLoaded.current = true;
        setLoaded(true);
      });
  }, []);

  useEffect(() => {
    if (!hasLoaded.current) return;
    invoke("save_settings", { config }).catch(() => {});
  }, [config]);

  return (
    <div className="shell">
      <aside className="sidebar">
        <div className="sidebar-logo">Database Sync Engine v1.0</div>
        <nav>
          {NAV.map((n) => (
            <button
              key={n.id}
              className={`sidebar-link${page === n.id ? " active" : ""}`}
              onClick={() => setPage(n.id)}
            >
              {n.label}
            </button>
          ))}
        </nav>
      </aside>

      <main className="content">
        {!loaded ? (
          <p className="empty">Loading settings…</p>
        ) : (
          <>
            {page === "dashboard" && <Dashboard config={config} />}
            {page === "db-connections" && (
              <DBConnections config={config} setConfig={setConfig} />
            )}
            {page === "table-mapping" && (
              <TableMapping config={config} setConfig={setConfig} />
            )}
            {page === "sync-settings" && (
              <SyncSettings config={config} setConfig={setConfig} />
            )}
            {page === "activity-logs" && <ActivityLogs />}
          </>
        )}
      </main>
    </div>
  );
}
