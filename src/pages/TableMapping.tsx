import { invoke } from "@tauri-apps/api/core";
import type { SyncConfig, TableMappingItem } from "../App";

type Props = {
  config: SyncConfig;
  setConfig: React.Dispatch<React.SetStateAction<SyncConfig>>;
};

export default function TableMapping({ config, setConfig }: Props) {
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

  async function handleSave() {
    try {
      await invoke("save_settings", { config });
    } catch {
      // ignore
    }
  }

  return (
    <>
      <p className="page-title">Table Mapping</p>
      <div className="card">
        <div className="section-heading">
          <h2>Remote → Local Table Pairs</h2>
          <button className="btn btn-secondary" onClick={addRow}>
            + Add Mapping
          </button>
        </div>

        {config.tableMappings.length === 0 ? (
          <p className="empty">No mappings yet. Add one above.</p>
        ) : (
          <>
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
                />
                <input
                  value={m.localTable}
                  onChange={(e) => updateRow(i, { localTable: e.currentTarget.value })}
                  placeholder="local_orders"
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
          <button className="btn btn-primary" onClick={handleSave}>
            Save Mappings
          </button>
        </div>
      </div>
    </>
  );
}
