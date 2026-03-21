import { useState, useCallback, useEffect } from "react";
import { invoke } from "@tauri-apps/api/core";
import { useRole } from "../hooks/useRole";
import i18n from "../lib/i18n";

interface StoreEntry {
  key: string;
  value_json: string;
  updated_at: string;
}

export default function ExpertStore() {
  const [store, setStore] = useState("env");
  const [entries, setEntries] = useState<StoreEntry[]>([]);
  const [selectedKey, setSelectedKey] = useState<string | null>(null);
  const [selectedValue, setSelectedValue] = useState("");
  const [editMode, setEditMode] = useState(false);
  const [newKey, setNewKey] = useState("");
  const [newValue, setNewValue] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [msg, setMsg] = useState<string | null>(null);
  const { canAccess } = useRole();

  const load = useCallback(async () => {
    setError(null);
    try {
      const data = await invoke<StoreEntry[]>("store_list", { store });
      setEntries(data);
    } catch (e) {
      setError(typeof e === "string" ? e : String(e));
    }
  }, [store]);

  useEffect(() => { load(); }, [load]);

  const handleSelect = async (key: string) => {
    setSelectedKey(key);
    setEditMode(false);
    try {
      const val = await invoke<string | null>("store_get", { store, key });
      setSelectedValue(val ?? "");
    } catch (e) {
      setSelectedValue(`Error: ${e}`);
    }
  };

  const handlePut = async () => {
    setError(null); setMsg(null);
    try {
      const m = await invoke<string>("store_put", {
        store, key: editMode ? selectedKey : newKey,
        valueJson: editMode ? selectedValue : newValue,
      });
      setMsg(m);
      setNewKey(""); setNewValue("");
      load();
    } catch (e) { setError(typeof e === "string" ? e : String(e)); }
  };

  const handleDelete = async (key: string) => {
    try {
      await invoke<string>("store_delete", { store, key });
      setSelectedKey(null);
      load();
    } catch (e) { setError(typeof e === "string" ? e : String(e)); }
  };

  return (
    <div>
      <div style={{ display: "flex", gap: 4, marginBottom: 12 }}>
        {["env", "secret"].map((s) => (
          <button key={s} className={`btn ${store === s ? "btn-primary" : "btn-ghost"}`}
            onClick={() => { setStore(s); setSelectedKey(null); }} style={{ fontSize: 12 }}>
            {s}
          </button>
        ))}
      </div>
      {error && <p style={{ color: "var(--danger)", fontSize: 13 }}>{error}</p>}
      {msg && <p style={{ color: "var(--success)", fontSize: 13 }}>{msg}</p>}

      <div style={{ display: "grid", gridTemplateColumns: "1fr 2fr", gap: 12 }}>
        <div style={{ maxHeight: 300, overflowY: "auto" }}>
          {entries.map((e) => (
            <div key={e.key} onClick={() => handleSelect(e.key)}
              style={{ padding: "6px 8px", cursor: "pointer", fontSize: 13,
                borderBottom: "1px solid var(--glass-border-subtle)",
                background: selectedKey === e.key ? "var(--accent-tint)" : "transparent" }}>
              {e.key}
            </div>
          ))}
          {entries.length === 0 && <p style={{ color: "var(--text-tertiary)", fontSize: 13 }}>{i18n.common.empty}</p>}
        </div>
        <div>
          {selectedKey && !editMode && (
            <div>
              <p style={{ fontSize: 12, color: "var(--text-secondary)", marginBottom: 4 }}>{selectedKey}</p>
              <pre style={{ background: "var(--bg-secondary)", borderRadius: 8, padding: 8,
                fontSize: 12, whiteSpace: "pre-wrap", maxHeight: 200, overflowY: "auto" }}>
                {selectedValue}
              </pre>
              {canAccess("operator") && (
                <div style={{ display: "flex", gap: 4, marginTop: 8 }}>
                  <button className="btn btn-ghost" style={{ fontSize: 12 }} onClick={() => setEditMode(true)}>{i18n.common.edit}</button>
                  <button className="btn btn-ghost" style={{ fontSize: 12, color: "var(--danger)" }}
                    onClick={() => handleDelete(selectedKey)}>{i18n.common.delete}</button>
                </div>
              )}
            </div>
          )}
          {editMode && selectedKey && (
            <div>
              <textarea value={selectedValue} onChange={(e) => setSelectedValue(e.target.value)}
                style={{ width: "100%", minHeight: 100, background: "var(--bg-secondary)",
                  border: "1px solid var(--glass-border-subtle)", borderRadius: 8, padding: 8,
                  fontFamily: "monospace", fontSize: 12, color: "var(--text-primary)" }} />
              <div style={{ display: "flex", gap: 4, marginTop: 4 }}>
                <button className="btn btn-primary" style={{ fontSize: 12 }} onClick={handlePut}>{i18n.common.save}</button>
                <button className="btn btn-ghost" style={{ fontSize: 12 }} onClick={() => setEditMode(false)}>{i18n.common.cancel}</button>
              </div>
            </div>
          )}
        </div>
      </div>

      {canAccess("operator") && (
        <div style={{ marginTop: 12, display: "flex", gap: 4, alignItems: "center" }}>
          <input placeholder="key" value={newKey} onChange={(e) => setNewKey(e.target.value)}
            style={{ padding: "4px 8px", borderRadius: 8, border: "1px solid var(--glass-border-subtle)",
              background: "var(--bg-secondary)", color: "var(--text-primary)", fontSize: 12 }} />
          <input placeholder="value (JSON)" value={newValue} onChange={(e) => setNewValue(e.target.value)}
            style={{ flex: 1, padding: "4px 8px", borderRadius: 8, border: "1px solid var(--glass-border-subtle)",
              background: "var(--bg-secondary)", color: "var(--text-primary)", fontSize: 12 }} />
          <button className="btn btn-primary" style={{ fontSize: 12 }} onClick={handlePut}
            disabled={!newKey}>{i18n.common.add}</button>
        </div>
      )}
    </div>
  );
}
