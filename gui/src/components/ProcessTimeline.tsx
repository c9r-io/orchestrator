import { useEffect } from "react";
import { useTimeline } from "../hooks/useTimeline";
import i18n from "../lib/i18n";
import type { TimelineEntry } from "../lib/types";

interface Props {
  taskId: string;
  selectedEntryId?: string | null;
  onSelectEntry?: (entry: TimelineEntry) => void;
}

const CATEGORY_LABELS: Record<string, string> = {
  goal: "目标",
  source: "来源",
  lifecycle: "状态",
  cycle: "循环",
  step: "步骤",
  tool: "工具",
  test: "测试",
  artifact: "产物",
  failure: "失败",
  recovery: "恢复",
  human_action: "人工",
  session: "会话",
  completion: "完成",
};

function TimelineRow({ entry, selected, onSelect }: { entry: TimelineEntry; selected: boolean; onSelect?: (entry: TimelineEntry) => void }) {
  const isFailure = entry.category === "failure" || entry.status === "failed";
  return (
    <li className={`timeline-row${isFailure ? " timeline-row-failure" : ""}${selected ? " timeline-row-selected" : ""}`}>
      <div className="timeline-rail" aria-hidden="true"><span /></div>
      <button type="button" className="timeline-content" aria-pressed={selected} onClick={() => onSelect?.(entry)}>
        <div className="timeline-meta">
          <time dateTime={entry.occurred_at}>{new Date(entry.occurred_at).toLocaleString()}</time>
          <span className="timeline-category">{CATEGORY_LABELS[entry.category] ?? entry.category}</span>
          {entry.status && <span className="timeline-status">{entry.status}</span>}
        </div>
        <strong>{entry.title}</strong>
        {entry.summary && <p>{entry.summary}</p>}
        <div className="timeline-context">
          {entry.actor && <span>{entry.actor.actor_type}: {entry.actor.actor_id}</span>}
          {entry.step_id && <span>step: {entry.step_id}</span>}
          {entry.session_id && <span>session: {entry.session_id}</span>}
          {entry.checkpoint_id && <span>checkpoint: {entry.checkpoint_id}</span>}
        </div>
        {entry.evidence.length > 0 && (
          <div className="timeline-evidence" aria-label={i18n.taskDetail.evidenceLabel}>
            {entry.evidence.map((evidence, index) => (
              <span key={`${evidence.kind}-${evidence.label}-${index}`}>
                {evidence.kind}: {evidence.label}{evidence.redacted ? " · redacted" : ""}
              </span>
            ))}
          </div>
        )}
      </button>
    </li>
  );
}

export default function ProcessTimeline({ taskId, selectedEntryId, onSelectEntry }: Props) {
  const { entries, hasMore, loading, loadingMore, error, loadMore } = useTimeline(taskId);
  useEffect(() => {
    if (!selectedEntryId && entries[0]) onSelectEntry?.(entries[0]);
  }, [entries, onSelectEntry, selectedEntryId, taskId]);
  return (
    <div className="liquid-glass timeline-panel">
      <div className="timeline-header">
        <div>
          <h3>{i18n.taskDetail.timeline}</h3>
          <p>{i18n.taskDetail.timelineHint}</p>
        </div>
        <span className="badge badge-info">{entries.length}</span>
      </div>
      {error && <p className="timeline-error">{error}</p>}
      {loading ? (
        <p className="timeline-empty">{i18n.common.loading}</p>
      ) : entries.length === 0 ? (
        <p className="timeline-empty">{i18n.taskDetail.timelineEmpty}</p>
      ) : (
        <ol className="timeline-list" aria-label={i18n.taskDetail.timelineLabel}>
          {entries.map((entry) => <TimelineRow key={entry.id} entry={entry} selected={entry.id === selectedEntryId} onSelect={onSelectEntry} />)}
        </ol>
      )}
      {hasMore && (
        <button className="btn btn-secondary timeline-more" onClick={loadMore} disabled={loadingMore}>
          {loadingMore ? i18n.common.loading : i18n.taskDetail.loadMore}
        </button>
      )}
    </div>
  );
}
