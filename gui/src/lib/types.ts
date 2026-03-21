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

export interface TaskLogChunk {
  run_id: string;
  phase: string;
  content: string;
  started_at: string | null;
}

/** Wish status derived from task status + workflow context. */
export type WishStatus = "drafting" | "pending_confirm" | "confirmed" | "failed" | "cancelled";
