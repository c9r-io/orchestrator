import { useEffect, useState, useCallback } from "react";
import { invoke } from "@tauri-apps/api/core";
import { useRole } from "../hooks/useRole";
import ConfirmDialog from "./ConfirmDialog";
import i18n from "../lib/i18n";

interface SecretKeyInfo {
  key_id: string;
  status: string;
  created_at: string;
}

interface SecretKeyStatusResult {
  active_key: SecretKeyInfo | null;
  all_keys: SecretKeyInfo[];
}

export default function ExpertSecret() {
  const [keys, setKeys] = useState<SecretKeyInfo[]>([]);
  const [activeKeyId, setActiveKeyId] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [msg, setMsg] = useState<string | null>(null);
  const [revokeTarget, setRevokeTarget] = useState<string | null>(null);
  const { canAccess } = useRole();

  const load = useCallback(async () => {
    setError(null);
    try {
      const status = await invoke<SecretKeyStatusResult>("secret_key_status", {});
      setKeys(status.all_keys);
      setActiveKeyId(status.active_key?.key_id ?? null);
    } catch (e) {
      setError(typeof e === "string" ? e : String(e));
    }
  }, []);

  useEffect(() => { load(); }, [load]);

  const handleRotate = async () => {
    setError(null); setMsg(null);
    try {
      const r = await invoke<{ message: string; resources_updated: number }>(
        "secret_key_rotate", {}
      );
      setMsg(`${r.message} (${r.resources_updated} resources updated)`);
      load();
    } catch (e) { setError(typeof e === "string" ? e : String(e)); }
  };

  const handleRevoke = async () => {
    if (!revokeTarget) return;
    const id = revokeTarget;
    setRevokeTarget(null);
    try {
      const m = await invoke<string>("secret_key_revoke", { keyId: id, force: true });
      setMsg(m);
      load();
    } catch (e) { setError(typeof e === "string" ? e : String(e)); }
  };

  return (
    <div>
      {error && <p style={{ color: "var(--danger)", fontSize: 13 }}>{error}</p>}
      {msg && <p style={{ color: "var(--success)", fontSize: 13 }}>{msg}</p>}

      {keys.length === 0 && !error && (
        <p style={{ color: "var(--text-tertiary)", fontSize: 13 }}>{i18n.expertSecret.noKeys}</p>
      )}

      {keys.length > 0 && (
        <table style={{ width: "100%", borderCollapse: "collapse", marginBottom: 12 }}>
          <thead>
            <tr>
              <th style={thStyle}>{i18n.expertSecret.colKeyId}</th>
              <th style={thStyle}>{i18n.expertSecret.colStatus}</th>
              <th style={thStyle}>{i18n.expertSecret.colCreatedAt}</th>
              {canAccess("admin") && <th style={thStyle}>{i18n.expertSecret.colActions}</th>}
            </tr>
          </thead>
          <tbody>
            {keys.map((k) => (
              <tr key={k.key_id} style={{
                borderBottom: "1px solid var(--glass-border-subtle)",
                background: k.key_id === activeKeyId ? "var(--accent-tint)" : "transparent",
              }}>
                <td style={tdStyle}>
                  <code>{k.key_id.slice(0, 12)}</code>
                  {k.key_id === activeKeyId && <span style={{ color: "var(--accent)", marginLeft: 4 }}>{i18n.expertSecret.activeLabel}</span>}
                </td>
                <td style={tdStyle}>{k.status}</td>
                <td style={tdStyle}>{k.created_at}</td>
                {canAccess("admin") && (
                  <td style={tdStyle}>
                    {k.key_id !== activeKeyId && k.status !== "revoked" && (
                      <button className="btn btn-ghost" style={{ fontSize: 11, color: "var(--danger)" }}
                        onClick={() => setRevokeTarget(k.key_id)}>{i18n.expertSecret.revoke}</button>
                    )}
                  </td>
                )}
              </tr>
            ))}
          </tbody>
        </table>
      )}

      {canAccess("admin") && (
        <div style={{ display: "flex", gap: 8 }}>
          <button className="btn btn-primary" style={{ fontSize: 12 }} onClick={handleRotate}>{i18n.expertSecret.rotateKey}</button>
          <button className="btn btn-ghost" style={{ fontSize: 12 }} onClick={load}>{i18n.common.refresh}</button>
        </div>
      )}

      <ConfirmDialog open={!!revokeTarget} title={i18n.expertSecret.revokeTitle}
        message={i18n.expertSecret.revokeMessage(revokeTarget?.slice(0, 12) ?? "")}
        confirmLabel={i18n.expertSecret.revokeConfirm} destructive onConfirm={handleRevoke}
        onCancel={() => setRevokeTarget(null)} />
    </div>
  );
}

const thStyle: React.CSSProperties = { padding: "8px 12px", textAlign: "left", color: "var(--text-secondary)", fontSize: 12, fontWeight: 600 };
const tdStyle: React.CSSProperties = { padding: "6px 12px", fontSize: 13 };
