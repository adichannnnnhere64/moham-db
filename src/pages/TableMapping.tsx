import { useState, useEffect } from "react";
import { invoke } from "@tauri-apps/api/core";
import type { SyncConfig, TableMappingItem } from "../App";

type Props = {
  config: SyncConfig;
  setConfig: React.Dispatch<React.SetStateAction<SyncConfig>>;
};

export default function TableMapping({ config, setConfig }: Props) {
  const [remoteTables, setRemoteTables] = useState<string[]>([]);
  const [localTables, setLocalTables] = useState<string[]>([]);

  useEffect(() => {
    if (!config.remoteDb.host || !config.remoteDb.database) return;
    invoke<string[]>("list_tables", { dbConfig: config.remoteDb })
      .then(setRemoteTables)
      .catch(() => setRemoteTables([]));
  }, [config.remoteDb]);

  useEffect(() => {
    if (!config.localDb.host || !config.localDb.database) return;
    invoke<string[]>("list_tables", { dbConfig: config.localDb })
      .then(setLocalTables)
      .catch(() => setLocalTables([]));
  }, [config.localDb]);

  function addRow() {
    setConfig((c) => ({
      ...c,
      tableMappings: [...c.tableMappings, { remoteTable: "", localTable: "" }],
    }));
  }

  function updateRow(index: number, patch: Partial<TableMappingItem>) {
    setConfig((c) => ({
      ...c,
      tableMappings: c.tableMappings.map((m, i) =>
        i === index ? { ...m, ...patch } : m
      ),
    }));
  }

  function removeRow(index: number) {
    setConfig((c) => ({
      ...c,
      tableMappings: c.tableMappings.filter((_, i) => i !== index),
    }));
  }

  return (
    <>
      <p className="page-title">Table Mapping</p>
      <div className="card">
        <div className="toggle-row">
          <label className="toggle">
            <input
              type="checkbox"
              checked={config.syncAllTables}
              onChange={(e) =>
                setConfig((c) => ({ ...c, syncAllTables: e.currentTarget.checked }))
              }
            />
            <span>Sync ALL remote tables automatically</span>
          </label>
          <label className="toggle">
            <input
              type="checkbox"
              checked={config.createMissingTables}
              onChange={(e) =>
                setConfig((c) => ({ ...c, createMissingTables: e.currentTarget.checked }))
              }
            />
            <span>Create missing local tables (for a fresh local DB)</span>
          </label>
        </div>

        {config.syncAllTables && (
          <p className="empty" style={{ marginTop: 12 }}>
            Syncing every remote table — manual mappings below are ignored.
          </p>
        )}

        <div className="section-heading" style={{ marginTop: 18 }}>
          <h2>Remote → Local Table Pairs</h2>
          <button className="btn btn-secondary" onClick={addRow} disabled={config.syncAllTables}>
            + Add Mapping
          </button>
        </div>

        {config.tableMappings.length === 0 ? (
          <p className="empty">No mappings yet. Add one above.</p>
        ) : (
          <>
            <datalist id="remote-tables-list">
              {remoteTables.map((t) => <option key={t} value={t} />)}
            </datalist>
            <datalist id="local-tables-list">
              {localTables.map((t) => <option key={t} value={t} />)}
            </datalist>

            <div className="mapping-header">
              <span>Remote Table</span>
              <span>Local Table</span>
              <span />
            </div>
            {config.tableMappings.map((m, i) => (
              <div className="mapping-row" key={i}>
                <input
                  value={m.remoteTable}
                  onChange={(e) => updateRow(i, { remoteTable: e.currentTarget.value })}
                  placeholder="remote_orders"
                  list="remote-tables-list"
                />
                <input
                  value={m.localTable}
                  onChange={(e) => updateRow(i, { localTable: e.currentTarget.value })}
                  placeholder="local_orders"
                  list="local-tables-list"
                />
                <button
                  className="btn btn-danger"
                  onClick={() => removeRow(i)}
                  style={{ padding: "7px 12px", fontSize: 12 }}
                >
                  Remove
                </button>
              </div>
            ))}
          </>
        )}

        <div className="btn-row">
          <button className="btn btn-primary" onClick={() => invoke("save_settings", { config }).catch(() => {})}>
            Save Mappings
          </button>
        </div>
      </div>
    </>
  );
}
