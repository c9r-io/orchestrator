import { useEffect, useState, useCallback } from "react";
import { invoke } from "@tauri-apps/api/core";
import { useRole } from "../hooks/useRole";
import StatusIcon from "../components/StatusIcon";
import i18n from "../lib/i18n";
import type { TaskSummary, TaskCreateResult } from "../lib/types";

interface Props {
  onSelectWish: (taskId: string) => void;
}

const MAX_CHARS = 2000;

const STATUS_FILTERS = [
  i18n.wishPool.filterAll,
  i18n.wishPool.filterDrafting,
  i18n.wishPool.filterPendingConfirm,
  i18n.wishPool.filterConfirmed,
  i18n.wishPool.filterCancelled,
] as const;

function wishStatusLabel(status: string): string {
  switch (status.toLowerCase()) {
    case "running":
    case "in_progress":
      return i18n.wishStatus.drafting;
    case "completed":
    case "succeeded":
      return i18n.wishStatus.pendingConfirm;
    case "paused":
      return i18n.wishStatus.paused;
    case "failed":
    case "error":
      return i18n.wishStatus.failed;
    case "deleted":
      return i18n.wishStatus.cancelled;
    default:
      return status;
  }
}

function matchesFilter(task: TaskSummary, filter: string): boolean {
  if (filter === i18n.wishPool.filterAll) return true;
  return wishStatusLabel(task.status) === filter;
}

export default function WishPool({ onSelectWish }: Props) {
  const [input, setInput] = useState("");
  const [submitting, setSubmitting] = useState(false);
  const [wishes, setWishes] = useState<TaskSummary[]>([]);
  const [filter, setFilter] = useState<string>(i18n.wishPool.filterAll);
  const [error, setError] = useState<string | null>(null);
  const { canAccess } = useRole();

  const loadWishes = useCallback(async () => {
    try {
      const tasks = await invoke<TaskSummary[]>("task_list", { project_filter: "wish-pool" });
      setWishes(tasks.sort((a, b) => b.updated_at.localeCompare(a.updated_at)));
    } catch {
      // silently fail on list refresh
    }
  }, []);

  useEffect(() => {
    loadWishes();
  }, [loadWishes]);

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
      // Navigate to the newly created wish detail
      onSelectWish(result.task_id);
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

  const filtered = wishes.filter((w) => matchesFilter(w, filter));

  return (
    <div>
      <h1 className="page-title">{i18n.wishPool.title}</h1>

      {/* Input area */}
      {canAccess("operator") && (
        <div className="liquid-glass" style={{ marginBottom: 20 }}>
          <textarea
            value={input}
            onChange={(e) => setInput(e.target.value.slice(0, MAX_CHARS))}
            onKeyDown={handleKeyDown}
            placeholder={i18n.wishPool.placeholder}
            aria-label={i18n.wishPool.inputLabel}
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
              aria-label={i18n.wishPool.submitLabel}
            >
              {submitting ? i18n.wishPool.submitting : i18n.wishPool.submit}
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
          onClick={loadWishes}
          style={{ marginLeft: "auto", fontSize: 13 }}
        >
          {i18n.common.refresh}
        </button>
      </div>

      {/* Wish list */}
      {filtered.length === 0 && (
        <div className="liquid-glass" style={{ textAlign: "center" }}>
          <p style={{ color: "var(--text-secondary)" }}>
            {wishes.length === 0 ? i18n.wishPool.emptyFirst : i18n.wishPool.emptyFiltered}
          </p>
        </div>
      )}

      <div style={{ display: "flex", flexDirection: "column", gap: 8 }}>
        {filtered.map((wish) => (
          <div
            key={wish.id}
            className="liquid-glass"
            style={{ cursor: "pointer", padding: 16 }}
            onClick={() => onSelectWish(wish.id)}
            role="button"
            tabIndex={0}
            onKeyDown={(e) => e.key === "Enter" && onSelectWish(wish.id)}
            aria-label={i18n.wishPool.wishLabel(wish.name || wish.goal)}
          >
            <div style={{ display: "flex", alignItems: "center", gap: 8 }}>
              <StatusIcon status={wish.status} />
              <span style={{ flex: 1, fontWeight: 500 }}>
                {wish.goal?.slice(0, 50) || wish.name || wish.id.slice(0, 8)}
              </span>
              <span style={{ fontSize: 12, color: "var(--text-tertiary)" }}>
                {wish.updated_at}
              </span>
            </div>
          </div>
        ))}
      </div>
    </div>
  );
}
