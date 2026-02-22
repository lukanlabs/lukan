/**
 * Local type definitions for the web client.
 * These mirror the types from the Rust backend's JSON serialization.
 */

// ── Checkpoint types ──────────────────────────────────────────────

export interface FileSnapshot {
  path: string;
  operation: "created" | "modified" | "deleted";
  beforeContent: string;
  afterContent: string;
  linesAdded: number;
  linesDeleted: number;
  diff?: string;
}

export interface Checkpoint {
  id: string;
  timestamp: number;
  messageIndex: number;
  description: string;
  userMessage: string;
  files: FileSnapshot[];
  totalLinesAdded: number;
  totalLinesDeleted: number;
}

// ── Session types ─────────────────────────────────────────────────

export interface SessionSummary {
  id: string;
  name: string;
  createdAt: string;
  updatedAt: string;
  messageCount: number;
  firstUserMessage: string;
  lastUserMessage: string;
  model: string;
}

// ── Permission mode ───────────────────────────────────────────────

export type PermissionMode = "manual" | "auto" | "skip" | "planner";

// ── Message types ─────────────────────────────────────────────────

export interface TextBlock {
  type: "text";
  text: string;
}

export interface ThinkingBlock {
  type: "thinking";
  text: string;
}

export interface ToolUseBlock {
  type: "tool_use";
  id: string;
  name: string;
  input: Record<string, unknown>;
}

export interface ToolResultBlock {
  type: "tool_result";
  toolUseId: string;
  content: string;
  isError?: boolean;
  diff?: string;
  image?: string;
}

export interface ImageBlock {
  type: "image";
  source: "base64" | "url";
  data: string;
  mediaType?: string;
}

export type ContentBlock =
  | TextBlock
  | ThinkingBlock
  | ToolUseBlock
  | ToolResultBlock
  | ImageBlock;

export interface Message {
  role: "user" | "assistant" | "tool";
  content: string | ContentBlock[];
  toolCallId?: string;
  name?: string;
}

// ── Stream events ─────────────────────────────────────────────────

export interface ToolApprovalRequest {
  id: string;
  name: string;
  input: Record<string, unknown>;
}

export type StopReason = "end_turn" | "tool_use" | "max_tokens" | "error";

export type StreamEvent =
  | { type: "text_delta"; text: string }
  | { type: "thinking_delta"; text: string }
  | { type: "tool_use_start"; id: string; name: string }
  | { type: "tool_use_delta"; input: string }
  | {
      type: "tool_use_end";
      id: string;
      name: string;
      input: Record<string, unknown>;
    }
  | { type: "tool_progress"; id: string; name: string; content: string }
  | {
      type: "tool_result";
      id: string;
      name: string;
      content: string;
      isError?: boolean;
      diff?: string;
      image?: string;
    }
  | { type: "approval_required"; tools: ToolApprovalRequest[] }
  | {
      type: "planner_question";
      id: string;
      questions: Array<{
        header: string;
        question: string;
        options: Array<{ label: string; description?: string }>;
        multiSelect: boolean;
      }>;
    }
  | {
      type: "usage";
      inputTokens: number;
      outputTokens: number;
      cacheCreationTokens?: number;
      cacheReadTokens?: number;
    }
  | { type: "message_start" }
  | { type: "message_end"; stopReason: StopReason }
  | { type: "queued_message_injected"; content: string | ContentBlock[] }
  | {
      type: "plan_review";
      id: string;
      title: string;
      plan: string;
      tasks: Array<{ title: string; detail: string }>;
    }
  | { type: "mode_changed"; mode: string }
  | { type: "error"; error: string };

// ── Worker types ──────────────────────────────────────────────────

export interface WorkerDefinition {
  id: string;
  name: string;
  schedule: string;
  prompt: string;
  tools?: string[];
  provider?: string;
  model?: string;
  enabled: boolean;
  notify?: Array<"web" | "whatsapp">;
  createdAt: string;
  lastRunAt?: string;
  lastRunStatus?: "success" | "error";
}

export interface WorkerRun {
  id: string;
  workerId: string;
  startedAt: string;
  completedAt?: string;
  status: "running" | "success" | "error";
  output: string;
  error?: string;
  tokenUsage: {
    input: number;
    output: number;
    cacheCreation: number;
    cacheRead: number;
  };
  turns: number;
}

export interface WorkerSummary extends WorkerDefinition {
  recentRunStatus?: "running" | "success" | "error";
}

export interface WorkerDetail extends WorkerSummary {
  recentRuns: WorkerRun[];
}

export interface WorkerCreateInput {
  name: string;
  schedule: string;
  prompt: string;
  tools?: string[];
  provider?: string;
  model?: string;
  enabled?: boolean;
  notify?: Array<"web" | "whatsapp">;
}

export interface WorkerUpdateInput {
  name?: string;
  schedule?: string;
  prompt?: string;
  tools?: string[];
  provider?: string;
  model?: string;
  enabled?: boolean;
  notify?: Array<"web" | "whatsapp">;
}

// ── Sub-agent types ───────────────────────────────────────────────

export interface SubAgentSummary {
  id: string;
  task: string;
  status: "running" | "completed" | "error" | "aborted";
  startedAt: string;
  completedAt?: string;
  turns: number;
  maxTurns: number;
  tokenUsage: {
    input: number;
    output: number;
    cacheCreation: number;
    cacheRead: number;
  };
  toolCount: number;
  runningToolCount: number;
  error?: string;
}

export interface SubAgentDetail extends SubAgentSummary {
  blocks: Array<
    | { type: "text"; id: string; text: string }
    | { type: "thinking"; id: string; text: string }
    | {
        type: "tool";
        id: string;
        tool: {
          id: string;
          name: string;
          isRunning: boolean;
          rawInput?: Record<string, unknown>;
          content?: string;
          isError?: boolean;
          diff?: string;
        };
      }
  >;
  textOutput: string;
}

// ── Server message (union of stream events + server-specific) ─────

export type ServerMessage =
  | StreamEvent
  | {
      type: "init";
      sessionId: string;
      messages: Message[];
      checkpoints: Checkpoint[];
      tokenUsage: {
        input: number;
        output: number;
        cacheCreation: number;
        cacheRead: number;
      };
      contextSize: number;
      permissionMode: PermissionMode;
      providerName: string;
      modelName: string;
      browserScreenshots: boolean;
    }
  | { type: "session_list"; sessions: SessionSummary[] }
  | {
      type: "session_loaded";
      sessionId: string;
      messages: Message[];
      checkpoints: Checkpoint[];
      tokenUsage: {
        input: number;
        output: number;
        cacheCreation: number;
        cacheRead: number;
      };
      contextSize: number;
    }
  | {
      type: "processing_complete";
      messages: Message[];
      checkpoints: Checkpoint[];
      contextSize?: number;
    }
  | { type: "model_list"; models: string[]; current: string }
  | { type: "model_changed"; providerName: string; modelName: string }
  | { type: "screenshots_changed"; enabled: boolean }
  | { type: "config_values"; config: Record<string, unknown> }
  | { type: "config_saved"; config: Record<string, unknown> }
  | { type: "sub_agents_update"; agents: SubAgentSummary[] }
  | { type: "sub_agent_detail"; agent: SubAgentDetail }
  | { type: "auth_required" }
  | { type: "auth_ok"; token: string }
  | { type: "auth_error"; error: string }
  | { type: "workers_update"; workers: WorkerSummary[] }
  | { type: "worker_detail"; worker: WorkerDetail }
  | { type: "worker_run_detail"; run: WorkerRun }
  | {
      type: "worker_notification";
      workerId: string;
      workerName: string;
      status: "success" | "error";
      summary: string;
    };

// ── Client message (sent to server) ──────────────────────────────

export type ClientMessage =
  | { type: "send_message"; content: string }
  | { type: "approve"; approvedIds: string[] }
  | { type: "deny_all" }
  | { type: "answer_question"; answer: string }
  | { type: "plan_accept"; tasks?: Array<{ title: string; detail: string }> }
  | { type: "plan_reject"; feedback: string }
  | { type: "plan_task_feedback"; taskIndex: number; feedback: string }
  | { type: "abort" }
  | { type: "load_session"; sessionId: string }
  | { type: "new_session"; name?: string }
  | { type: "list_sessions" }
  | { type: "delete_session"; sessionId: string }
  | { type: "set_permission_mode"; mode: PermissionMode }
  | { type: "list_models" }
  | { type: "set_model"; model: string }
  | { type: "set_screenshots"; enabled: boolean }
  | { type: "get_config" }
  | { type: "set_config"; config: Record<string, unknown> }
  | { type: "get_sub_agent_detail"; id: string }
  | { type: "abort_sub_agent"; id: string }
  | { type: "auth"; token: string }
  | { type: "auth_login"; password: string }
  | { type: "list_workers" }
  | { type: "create_worker"; worker: WorkerCreateInput }
  | { type: "update_worker"; id: string; patch: WorkerUpdateInput }
  | { type: "delete_worker"; id: string }
  | { type: "toggle_worker"; id: string; enabled: boolean }
  | { type: "get_worker_detail"; id: string }
  | { type: "get_worker_run_detail"; workerId: string; runId: string };
