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

export interface AgentSession {
  session_id: string; task_id: string; task_item_id: string | null; step_id: string;
  agent_id: string; state: string; pid: number; writer_client_id: string | null;
  writer_actor: string | null; writer_lease_expires_at: string | null; state_version: number;
}

export interface SessionOutputChunk {
  offset: number; next_offset: number; text: string; eof: boolean; redacted: boolean;
}

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

export interface AttentionAction {
  id: string;
  label: string;
  required_role: string;
  confirmation: string;
  input_schema_json: string;
}

export interface AttentionItem {
  id: string;
  project_id: string;
  task_id: string;
  task_item_id: string | null;
  step_id: string | null;
  session_id: string | null;
  kind: string;
  severity: "intervention" | "attention";
  state: "open" | "claimed" | "snoozed" | "resolved";
  title: string;
  summary: string;
  requested_decision_json: string | null;
  actions: AttentionAction[];
  assignee: string | null;
  occurrence_count: number;
  reopen_count: number;
  version: number;
  created_at: string;
  updated_at: string;
  last_occurred_at: string;
  snoozed_until: string | null;
  resolved_at: string | null;
}

export interface AttentionListResult {
  items: AttentionItem[];
  latest_change_id: number;
}

export interface AttentionDelta {
  kind: "upsert" | "remove";
  change_id: number;
  item: AttentionItem | null;
  notification?: AttentionNotification | null;
}

export interface AttentionNotification {
  dedupe_key: string;
  attention_item_id: string;
  item_version: number;
  title: string;
  severity: string;
  process_id: string;
  deep_link: string;
}

export interface SourceEvent {
  id: string;
  project_id: string;
  provider: string;
  installation_id: string;
  external_event_id: string;
  event_type: string;
  reaction_name: string | null;
  reaction_target_kind: string | null;
  reaction_target_id: string | null;
  conversation_id: string | null;
  thread_id: string | null;
  occurred_at: string;
  received_at: string;
  normalized_json: string;
  routing_state: string;
  routing_attempts: number;
  routed_task_id: string | null;
  last_error_code: string | null;
}

export interface SourceBinding {
  id: string;
  task_id: string;
  provider: string;
  installation_id: string;
  conversation_id: string | null;
  thread_id: string | null;
  binding_type: string;
  created_at: string;
}

export interface HandoffBriefing {
  goal: string;
  current_state: Record<string, unknown>;
  last_success: Record<string, unknown> | null;
  failure: Record<string, unknown> | null;
  test_evidence: Record<string, unknown>[];
  changed_files: string[];
  constraints: string[];
  decisions: string[];
  open_questions: string[];
  recommendations: string[];
}

export interface HandoffSnapshot {
  id: string;
  task_id: string;
  source_event_cursor: number;
  projection_version: number;
  briefing: HandoffBriefing;
  content_hash: string;
  state_version: string;
  created_at: string;
}

export interface ResumeBoundary {
  id: string;
  task_id: string;
  cycle: number;
  step_id: string | null;
  task_item_id: string | null;
  provider_session_available: boolean;
  side_effect_class: string;
  replay_safe: boolean;
  reason: string;
  state_version: string;
}

export interface ResumePlan {
  id: string;
  task_id: string;
  boundary: ResumeBoundary | null;
  mode: string;
  expected_state_version: string;
  consequence: Record<string, unknown>;
  elevated_confirmation_required: boolean;
  expires_at: string;
  status: string;
}

export interface ResumeExecution {
  execution_id: string;
  plan_id: string;
  accepted: boolean;
  status: string;
  child_task_id: string | null;
}

/** Wish status derived from task status + workflow context. */
export type WishStatus = "drafting" | "pending_confirm" | "confirmed" | "failed" | "cancelled";
