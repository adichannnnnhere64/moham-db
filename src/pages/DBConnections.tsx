import { useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import type { SyncConfig, DbConfig } from "../App";

type Props = {
  config: SyncConfig;
  setConfig: React.Dispatch<React.SetStateAction<SyncConfig>>;
};

type ConnResult = { ok: boolean; message: string } | null;

export default function DBConnections({ config, setConfig }: Props) {
  const [localResult, setLocalResult] = useState<ConnResult>(null);
  const [remoteResult, setRemoteResult] = useState<ConnResult>(null);
  const [testingLocal, setTestingLocal] = useState(false);
  const [testingRemote, setTestingRemote] = useState(false);
  const [saving, setSaving] = useState(false);

  function updateLocal(key: keyof DbConfig, value: string | number) {
    setConfig((c) => ({ ...c, localDb: { ...c.localDb, [key]: value } }));
  }

  function updateRemote(key: keyof DbConfig, value: string | number) {
    setConfig((c) => ({ ...c, remoteDb: { ...c.remoteDb, [key]: value } }));
  }

  async function testLocal() {
    setTestingLocal(true);
    setLocalResult(null);
    try {
      const r = await invoke<{ ok: boolean; message: string }>("test_connection", {
        dbConfig: config.localDb,
      });
      setLocalResult(r);
    } catch (e) {
      setLocalResult({ ok: false, message: String(e) });
    } finally {
      setTestingLocal(false);
    }
  }

  async function testRemote() {
    setTestingRemote(true);
    setRemoteResult(null);
    try {
      const r = await invoke<{ ok: boolean; message: string }>("test_connection", {
        dbConfig: config.remoteDb,
      });
      setRemoteResult(r);
    } catch (e) {
      setRemoteResult({ ok: false, message: String(e) });
    } finally {
      setTestingRemote(false);
    }
  }

  async function handleSave() {
    setSaving(true);
    try {
      await invoke("save_settings", { config });
    } catch {
      // ignore — user sees no feedback here; they can test connection
    } finally {
      setSaving(false);
    }
  }

  return (
    <>
      <p className="page-title">Database Connections</p>
      <div className="db-panels">
        <div className="card">
          <div className="db-panel-title">Local Database</div>
          <div className="form-grid">
            <div className="form-group">
              <label>Host</label>
              <input
                value={config.localDb.host}
                onChange={(e) => updateLocal("host", e.currentTarget.value)}
                placeholder="127.0.0.1"
              />
            </div>
            <div className="form-group">
              <label>Port</label>
              <input
                type="number"
                min="1"
                max="65535"
                value={config.localDb.port}
                onChange={(e) => updateLocal("port", Number(e.currentTarget.value))}
              />
            </div>
            <div className="form-group">
              <label>Database</label>
              <input
                value={config.localDb.database}
                onChange={(e) => updateLocal("database", e.currentTarget.value)}
              />
            </div>
            <div className="form-group">
              <label>Username</label>
              <input
                value={config.localDb.username}
                onChange={(e) => updateLocal("username", e.currentTarget.value)}
              />
            </div>
            <div className="form-group span-two">
              <label>Password</label>
              <input
                type="password"
                value={config.localDb.password}
                onChange={(e) => updateLocal("password", e.currentTarget.value)}
              />
            </div>
          </div>
          <div className="btn-row">
            <button className="btn btn-secondary" onClick={testLocal} disabled={testingLocal}>
              {testingLocal ? "Testing..." : "Test Connection"}
            </button>
          </div>
          {localResult && (
            <div className={`conn-result ${localResult.ok ? "ok" : "err"}`}>
              {localResult.message}
            </div>
          )}
        </div>

        <div className="card">
          <div className="db-panel-title">Remote Database</div>
          <div className="form-grid">
            <div className="form-group">
              <label>Host</label>
              <input
                value={config.remoteDb.host}
                onChange={(e) => updateRemote("host", e.currentTarget.value)}
                placeholder="192.168.1.50"
              />
            </div>
            <div className="form-group">
              <label>Port</label>
              <input
                type="number"
                min="1"
                max="65535"
                value={config.remoteDb.port}
                onChange={(e) => updateRemote("port", Number(e.currentTarget.value))}
              />
            </div>
            <div className="form-group">
              <label>Database</label>
              <input
                value={config.remoteDb.database}
                onChange={(e) => updateRemote("database", e.currentTarget.value)}
              />
            </div>
            <div className="form-group">
              <label>Username</label>
              <input
                value={config.remoteDb.username}
                onChange={(e) => updateRemote("username", e.currentTarget.value)}
              />
            </div>
            <div className="form-group span-two">
              <label>Password</label>
              <input
                type="password"
                value={config.remoteDb.password}
                onChange={(e) => updateRemote("password", e.currentTarget.value)}
              />
            </div>
          </div>
          <div className="btn-row">
            <button className="btn btn-secondary" onClick={testRemote} disabled={testingRemote}>
              {testingRemote ? "Testing..." : "Test Connection"}
            </button>
          </div>
          {remoteResult && (
            <div className={`conn-result ${remoteResult.ok ? "ok" : "err"}`}>
              {remoteResult.message}
            </div>
          )}
        </div>
      </div>

      <div className="btn-row">
        <button className="btn btn-primary" onClick={handleSave} disabled={saving}>
          {saving ? "Saving..." : "Save Connections"}
        </button>
      </div>
    </>
  );
}
