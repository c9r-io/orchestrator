/** Mirrors the Rust serializable types from Tauri commands. */

export interface PingInfo {
  version: string;
  git_hash: string;
  uptime_secs: string;
}

export interface TaskSummary {
  id: string;
  name: string;
  status: string;
  total_items: number;
  finished_items: number;
  failed_items: number;
  created_at: string;
  updated_at: string;
  project_id: string;
  workflow_id: string;
  goal: string;
}

export interface TaskDetail {
  id: string;
  name: string;
  status: string;
  goal: string;
  total_items: number;
  finished_items: number;
  failed_items: number;
  created_at: string;
  updated_at: string;
  project_id: string;
  workflow_id: string;
  items: TaskItemSummary[];
}

export interface TaskItemSummary {
  id: string;
  qa_file_path: string;
  status: string;
  order_no: number;
}

export interface TaskCreateResult {
  task_id: string;
  status: string;
  message: string;
}

export interface TaskActionResult {
  message: string;
}

export interface LogLine {
  line: string;
  timestamp: string;
}

export interface WatchSnapshot {
  task: TaskSummary;
  items: TaskItemSummary[];
}

export interface ResourceResult {
  content: string;
  format: string;
}

export interface AgentInfo {
  name: string;
  enabled: boolean;
  lifecycle_state: string;
  in_flight_items: number;
  capabilities: string[];
  is_healthy: boolean;
}

export interface StoreEntry {
  key: string;
  value_json: string;
  updated_at: string;
}

export type Role = "read_only" | "operator" | "admin";

/** Connection lifecycle states emitted from the Rust backend. */
export type ConnectionState =
  | { kind: "Disconnected" }
  | { kind: "Connecting" }
  | { kind: "Connected" }
  | { kind: "Reconnecting"; attempt: number; max_attempts: number }
  | { kind: "Failed"; message: string };

export interface TaskLogChunk {
  run_id: string;
  phase: string;
  content: string;
  started_at: string | null;
}

export interface TimelineActor {
  actor_type: string;
  actor_id: string;
}

export interface TimelineEvidence {
  kind: string;
  label: string;
  uri: string | null;
  content_type: string | null;
  digest: string | null;
  redacted: boolean;
}

export interface TimelineEntry {
  id: string;
  task_id: string;
  occurred_at: string;
  category: string;
  title: string;
  summary: string;
  status: string | null;
  actor: TimelineActor | null;
  step_id: string | null;
  task_item_id: string | null;
  command_run_id: string | null;
  session_id: string | null;
  checkpoint_id: string | null;
  source_event_id: string | null;
  evidence: TimelineEvidence[];
  raw_event_ids: number[];
  projection_version: number;
}

export interface TaskTimelinePage {
  entries: TimelineEntry[];
  next_cursor: string | null;
  has_more: boolean;
  snapshot_max_event_id: number;
  projection_version: number;
}

export interface TimelineDelta {
  kind: "upsert" | "reset_required";
  entry: TimelineEntry | null;
  snapshot_max_event_id: number;
}

/** Wish status derived from task status + workflow context. */
export type WishStatus = "drafting" | "pending_confirm" | "confirmed" | "failed" | "cancelled";
