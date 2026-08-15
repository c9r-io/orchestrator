import { useEffect, useState, useCallback } from "react";
import { invoke } from "@tauri-apps/api/core";
import { useRole } from "../hooks/useRole";
import StatusIcon from "../components/StatusIcon";
import i18n from "../lib/i18n";
import type { TaskSummary, TaskCreateResult } from "../lib/types";

interface Props {
  onSelectDraft: (taskId: string) => void;
}

const MAX_CHARS = 2000;

const STATUS_FILTERS = [
  i18n.taskDraftList.filterAll,
  i18n.taskDraftList.filterDrafting,
  i18n.taskDraftList.filterPendingConfirm,
  i18n.taskDraftList.filterConfirmed,
  i18n.taskDraftList.filterCancelled,
] as const;

function draftStatusLabel(status: string): string {
  switch (status.toLowerCase()) {
    case "running":
    case "in_progress":
      return i18n.taskDraftStatus.drafting;
    case "completed":
    case "succeeded":
      return i18n.taskDraftStatus.pendingConfirm;
    case "paused":
      return i18n.taskDraftStatus.paused;
    case "failed":
    case "error":
      return i18n.taskDraftStatus.failed;
    case "deleted":
      return i18n.taskDraftStatus.cancelled;
    default:
      return status;
  }
}

function matchesFilter(task: TaskSummary, filter: string): boolean {
  if (filter === i18n.taskDraftList.filterAll) return true;
  return draftStatusLabel(task.status) === filter;
}

export default function TaskDraftList({ onSelectDraft }: Props) {
  const [input, setInput] = useState("");
  const [submitting, setSubmitting] = useState(false);
  const [drafts, setDrafts] = useState<TaskSummary[]>([]);
  const [filter, setFilter] = useState<string>(i18n.taskDraftList.filterAll);
  const [error, setError] = useState<string | null>(null);
  const { canAccess } = useRole();

  const loadDrafts = useCallback(async () => {
    try {
      const tasks = await invoke<TaskSummary[]>("task_list", { project_filter: "wish-pool" });
      setDrafts(tasks.sort((a, b) => b.updated_at.localeCompare(a.updated_at)));
    } catch {
      // silently fail on list refresh
    }
  }, []);

  useEffect(() => {
    loadDrafts();
  }, [loadDrafts]);

  const handleSubmit = async () => {
    if (!input.trim() || submitting) return;
    setSubmitting(true);
    setError(null);
    try {
      const result = await invoke<TaskCreateResult>("task_create", {
        goal: input.trim(),
        project_id: "wish-pool",
      });
      setInput("");
      // Navigate to the newly created draft detail
      onSelectDraft(result.task_id);
    } catch (e) {
      setError(typeof e === "string" ? e : String(e));
    } finally {
      setSubmitting(false);
    }
  };

  const handleKeyDown = (e: React.KeyboardEvent) => {
    if ((e.metaKey || e.ctrlKey) && e.key === "Enter") {
      e.preventDefault();
      handleSubmit();
    }
  };

  const filtered = drafts.filter((w) => matchesFilter(w, filter));

  return (
    <div>
      <h1 className="page-title">{i18n.taskDraftList.title}</h1>

      {/* Input area */}
      {canAccess("operator") && (
        <div className="liquid-glass" style={{ marginBottom: 20 }}>
          <textarea
            value={input}
            onChange={(e) => setInput(e.target.value.slice(0, MAX_CHARS))}
            onKeyDown={handleKeyDown}
            placeholder={i18n.taskDraftList.placeholder}
            aria-label={i18n.taskDraftList.inputLabel}
            style={{
              width: "100%",
              minHeight: 120,
              background: "transparent",
              border: "1px solid var(--glass-border-subtle)",
              borderRadius: 12,
              padding: 12,
              fontSize: 15,
              color: "var(--text-primary)",
              resize: "vertical",
              fontFamily: "inherit",
              outline: "none",
            }}
            disabled={submitting}
          />
          <div
            style={{
              display: "flex",
              alignItems: "center",
              justifyContent: "space-between",
              marginTop: 8,
            }}
          >
            <span style={{ fontSize: 12, color: "var(--text-tertiary)" }}>
              {input.length}/{MAX_CHARS}
            </span>
            <button
              className="btn btn-primary"
              onClick={handleSubmit}
              disabled={!input.trim() || submitting}
              aria-label={i18n.taskDraftList.submitLabel}
            >
              {submitting ? i18n.taskDraftList.submitting : i18n.taskDraftList.submit}
            </button>
          </div>
          {error && (
            <p style={{ color: "var(--danger)", fontSize: 13, marginTop: 8 }}>{error}</p>
          )}
        </div>
      )}

      {/* Filter tabs */}
      <div style={{ display: "flex", gap: 4, marginBottom: 16 }}>
        {STATUS_FILTERS.map((f) => (
          <button
            key={f}
            className={`btn ${filter === f ? "btn-primary" : "btn-ghost"}`}
            onClick={() => setFilter(f)}
            style={{ fontSize: 13, padding: "4px 12px" }}
          >
            {f}
          </button>
        ))}
        <button
          className="btn btn-ghost"
          onClick={loadDrafts}
          style={{ marginLeft: "auto", fontSize: 13 }}
        >
          {i18n.common.refresh}
        </button>
      </div>

      {/* Draft list */}
      {filtered.length === 0 && (
        <div className="liquid-glass" style={{ textAlign: "center" }}>
          <p style={{ color: "var(--text-secondary)" }}>
            {drafts.length === 0 ? i18n.taskDraftList.emptyFirst : i18n.taskDraftList.emptyFiltered}
          </p>
        </div>
      )}

      <div style={{ display: "flex", flexDirection: "column", gap: 8 }}>
        {filtered.map((draft) => (
          <div
            key={draft.id}
            className="liquid-glass"
            style={{ cursor: "pointer", padding: 16 }}
            onClick={() => onSelectDraft(draft.id)}
            role="button"
            tabIndex={0}
            onKeyDown={(e) => e.key === "Enter" && onSelectDraft(draft.id)}
            aria-label={i18n.taskDraftList.draftLabel(draft.name || draft.goal)}
          >
            <div style={{ display: "flex", alignItems: "center", gap: 8 }}>
              <StatusIcon status={draft.status} />
              <span style={{ flex: 1, fontWeight: 500 }}>
                {draft.goal?.slice(0, 50) || draft.name || draft.id.slice(0, 8)}
              </span>
              <span style={{ fontSize: 12, color: "var(--text-tertiary)" }}>
                {draft.updated_at}
              </span>
            </div>
          </div>
        ))}
      </div>
    </div>
  );
}
