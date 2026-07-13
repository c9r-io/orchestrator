import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import type { SourceBinding } from "../lib/types";
import i18n from "../lib/i18n";

export default function SourcePanel({ taskId }: { taskId: string }) {
  const [bindings, setBindings] = useState<SourceBinding[]>([]);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let active = true;
    invoke<SourceBinding[]>("source_binding_list", { task_id: taskId })
      .then((items) => { if (active) setBindings(items); })
      .catch((reason) => { if (active) setError(String(reason)); });
    return () => { active = false; };
  }, [taskId]);

  return (
    <section className="liquid-glass" aria-labelledby="source-bindings-title" style={{ marginBottom: 16 }}>
      <h3 id="source-bindings-title" style={{ marginBottom: 10 }}>{i18n.sources.taskBindings}</h3>
      {error && <p role="alert" style={{ color: "var(--danger)" }}>{error}</p>}
      {!error && bindings.length === 0 && <p style={{ color: "var(--text-tertiary)" }}>{i18n.sources.noBindings}</p>}
      {bindings.map((binding) => (
        <div key={binding.id} style={{ padding: "8px 0", borderBottom: "1px solid var(--glass-border-subtle)" }}>
          <div style={{ display: "flex", gap: 8, alignItems: "center", flexWrap: "wrap" }}>
            <span className="badge badge-info">{binding.provider}</span>
            <strong>{binding.installation_id}</strong>
            <span>{binding.binding_type}</span>
          </div>
          <p style={{ marginTop: 4, color: "var(--text-secondary)", overflowWrap: "anywhere" }}>
            {binding.conversation_id ?? "—"} / {binding.thread_id ?? "—"}
          </p>
        </div>
      ))}
    </section>
  );
}
