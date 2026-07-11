import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { useCallback, useEffect, useState } from "react";
import type { TaskTimelinePage, TimelineDelta, TimelineEntry } from "../lib/types";

const PAGE_SIZE = 50;

function mergeEntries(current: TimelineEntry[], incoming: TimelineEntry[]) {
  const merged = [...current];
  const positions = new Map(current.map((entry, index) => [entry.id, index]));
  for (const entry of incoming) {
    const position = positions.get(entry.id);
    if (position === undefined) {
      positions.set(entry.id, merged.length);
      merged.push(entry);
    } else {
      merged[position] = entry;
    }
  }
  return merged;
}

export function useTimeline(taskId: string, categories: string[] = []) {
  const [entries, setEntries] = useState<TimelineEntry[]>([]);
  const [nextCursor, setNextCursor] = useState<string | null>(null);
  const [hasMore, setHasMore] = useState(false);
  const [loading, setLoading] = useState(true);
  const [loadingMore, setLoadingMore] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const categoryKey = categories.join(",");

  const fetchPage = useCallback(
    async (cursor: string | null) => {
      return invoke<TaskTimelinePage>("task_timeline", {
        task_id: taskId,
        cursor,
        limit: PAGE_SIZE,
        categories,
      });
    },
    // categoryKey is the stable representation used to avoid array identity churn.
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [taskId, categoryKey]
  );

  const loadMore = useCallback(async () => {
    if (!hasMore || !nextCursor || loadingMore) return;
    setLoadingMore(true);
    try {
      const page = await fetchPage(nextCursor);
      setEntries((current) => mergeEntries(current, page.entries));
      setNextCursor(page.next_cursor);
      setHasMore(page.has_more);
    } catch (reason) {
      setError(typeof reason === "string" ? reason : String(reason));
    } finally {
      setLoadingMore(false);
    }
  }, [fetchPage, hasMore, loadingMore, nextCursor]);

  useEffect(() => {
    let disposed = false;
    let unlisten: UnlistenFn | null = null;
    let unlistenError: UnlistenFn | null = null;

    const refreshAndFollow = async () => {
      setLoading(true);
      setError(null);
      try {
        const page = await fetchPage(null);
        if (disposed) return;
        setEntries(page.entries);
        setNextCursor(page.next_cursor);
        setHasMore(page.has_more);

        unlisten = await listen<TimelineDelta>(`task-timeline-${taskId}`, (event) => {
          if (event.payload.kind === "reset_required") {
            fetchPage(null)
              .then((snapshot) => {
                if (disposed) return;
                setEntries(snapshot.entries);
                setNextCursor(snapshot.next_cursor);
                setHasMore(snapshot.has_more);
              })
              .catch((reason) => setError(typeof reason === "string" ? reason : String(reason)));
          } else if (event.payload.entry) {
            setEntries((current) => mergeEntries(current, [event.payload.entry as TimelineEntry]));
          }
        });
        unlistenError = await listen<string>(`stream-error-timeline-${taskId}`, (event) => {
          setError(event.payload);
        });
        await invoke("start_task_timeline_follow", {
          task_id: taskId,
          after_event_id: page.snapshot_max_event_id,
          categories,
        });
      } catch (reason) {
        if (!disposed) setError(typeof reason === "string" ? reason : String(reason));
      } finally {
        if (!disposed) setLoading(false);
      }
    };

    refreshAndFollow();
    return () => {
      disposed = true;
      unlisten?.();
      unlistenError?.();
      invoke("stop_task_timeline_follow", { task_id: taskId }).catch(() => {});
    };
    // categoryKey is the stable representation used to avoid array identity churn.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [fetchPage, taskId, categoryKey]);

  return { entries, hasMore, loading, loadingMore, error, loadMore };
}
