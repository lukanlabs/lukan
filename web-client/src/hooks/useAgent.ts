import { useState, useEffect, useCallback, useRef } from "react";
import type {
  Checkpoint,
  SessionSummary,
  PermissionMode,
  Message,
  WorkerSummary,
  WorkerDetail,
  WorkerRun,
  WorkerCreateInput,
  WorkerUpdateInput,
  ServerMessage,
  SubAgentSummary,
  SubAgentDetail,
} from "../lib/types.ts";
import {
  appendTextDelta,
  appendThinkingDelta,
  setToolInput,
  setToolProgress,
  setToolResult,
  startTool,
} from "../lib/streaming-blocks.ts";
import { WebSocketClient, storeToken, clearToken } from "../lib/ws-client.ts";

// ── Types ────────────────────────────────────────────────────────────

export interface ToolStatus {
  id: string;
  name: string;
  isRunning: boolean;
  isHistorical?: boolean; // True if loaded from history (not actively running)
  rawInput?: Record<string, unknown>;
  content?: string;
  isError?: boolean;
  diff?: string;
  image?: string;
}

export type StreamingBlock =
  | { type: "text"; id: string; text: string }
  | { type: "thinking"; id: string; text: string }
  | { type: "tool"; id: string; tool: ToolStatus };

export interface PendingApproval {
  tools: Array<{ id: string; name: string; input: Record<string, unknown> }>;
}

export interface PendingQuestion {
  id: string;
  questions: Array<{
    header: string;
    question: string;
    options: Array<{ label: string; description?: string }>;
    multiSelect: boolean;
  }>;
}

export interface PendingPlanReview {
  id: string;
  title: string;
  plan: string;
  tasks: Array<{ title: string; detail: string }>;
}

export interface AgentState {
  connected: boolean;
  /** "none" = no auth needed, "required" = show login, "authenticated" = logged in */
  authState: "none" | "required" | "authenticated";
  authError: string | null;
  sessionId: string;
  messages: Message[];
  streamingBlocks: StreamingBlock[];
  isProcessing: boolean;
  pendingApproval: PendingApproval | null;
  pendingQuestion: PendingQuestion | null;
  pendingPlanReview: PendingPlanReview | null;
  permissionMode: PermissionMode;
  tokenUsage: { input: number; output: number; cacheCreation: number; cacheRead: number };
  /** Last input tokens from most recent LLM call — represents current context window size */
  contextSize: number;
  checkpoints: Checkpoint[];
  providerName: string;
  modelName: string;
  error: string | null;
  sessionList: SessionSummary[] | null;
  availableModels: string[] | null;
  /** Cache of tool images (toolUseId → URL) that survives processing_complete */
  toolImages: Record<string, string>;
  /** Whether browser auto-screenshots are enabled */
  browserScreenshots: boolean;
  /** Safe config values from server */
  configValues: Record<string, unknown> | null;
  /** Sub-agent summary list */
  subAgents: SubAgentSummary[];
  /** Currently viewed sub-agent detail */
  subAgentDetail: SubAgentDetail | null;
  /** Worker summary list */
  workers: WorkerSummary[];
  /** Currently viewed worker detail */
  workerDetail: WorkerDetail | null;
  /** Currently viewed worker run detail */
  workerRunDetail: WorkerRun | null;
  /** Latest worker notification (auto-dismissed) */
  workerNotification: {
    workerId: string;
    workerName: string;
    status: "success" | "error";
    summary: string;
  } | null;
}

// ── Hook ─────────────────────────────────────────────────────────────

export function useAgent() {
  const [state, setState] = useState<AgentState>({
    connected: false,
    authState: "none",
    authError: null,
    sessionId: "",
    messages: [],
    streamingBlocks: [],
    isProcessing: false,
    pendingApproval: null,
    pendingQuestion: null,
    pendingPlanReview: null,
    permissionMode: "manual",
    tokenUsage: { input: 0, output: 0, cacheCreation: 0, cacheRead: 0 },
    contextSize: 0,
    checkpoints: [],
    providerName: "",
    modelName: "",
    error: null,
    sessionList: null,
    availableModels: null,
    toolImages: {},
    browserScreenshots: true,
    configValues: null,
    subAgents: [],
    subAgentDetail: null,
    workers: [],
    workerDetail: null,
    workerRunDetail: null,
    workerNotification: null,
  });

  const wsRef = useRef<WebSocketClient | null>(null);
  const blocksRef = useRef<StreamingBlock[]>([]);
  const blockIdCounter = useRef(0);
  const renderTimer = useRef<ReturnType<typeof setTimeout> | null>(null);
  /** Accumulates tool images across rounds; copied to state on processing_complete */
  const imageCacheRef = useRef<Record<string, string>>({});
  /** Timer for auto-dismissing worker notifications */
  const workerNotifyTimer = useRef<ReturnType<typeof setTimeout> | null>(null);

  // Batched render — accumulate fast deltas then flush at ~20fps
  const scheduleRender = useCallback(() => {
    if (renderTimer.current) return;
    renderTimer.current = setTimeout(() => {
      renderTimer.current = null;
      setState((s) => ({ ...s, streamingBlocks: [...blocksRef.current] }));
    }, 50);
  }, []);

  const flushRender = useCallback(() => {
    if (renderTimer.current) {
      clearTimeout(renderTimer.current);
      renderTimer.current = null;
    }
    setState((s) => ({ ...s, streamingBlocks: [...blocksRef.current] }));
  }, []);

  // Process server messages
  const handleMessage = useCallback(
    (event: ServerMessage) => {
      switch (event.type) {
        case "init":
          setState((s) => ({
            ...s,
            authState: s.authState === "required" ? "authenticated" : s.authState,
            authError: null,
            sessionId: event.sessionId,
            messages: event.messages,
            checkpoints: event.checkpoints,
            tokenUsage: event.tokenUsage,
            contextSize: event.contextSize,
            permissionMode: event.permissionMode,
            providerName: event.providerName,
            modelName: event.modelName,
            browserScreenshots: event.browserScreenshots,
          }));
          break;

        case "message_start":
          // Don't clear blocks here — tools from previous rounds should stay visible.
          // Only processing_complete clears everything once the full turn is done.
          setState((s) => ({ ...s, isProcessing: true, error: null }));
          break;

        case "text_delta": {
          appendTextDelta(blocksRef.current, event.text, () => `txt-${blockIdCounter.current++}`);
          scheduleRender();
          break;
        }

        case "thinking_delta": {
          appendThinkingDelta(
            blocksRef.current,
            event.text,
            () => `thk-${blockIdCounter.current++}`,
          );
          scheduleRender();
          break;
        }

        case "tool_use_start":
          flushRender();
          startTool(blocksRef.current, event.id, event.name, (tool) => ({
            type: "tool",
            id: event.id,
            tool,
          }));
          setState((s) => ({ ...s, streamingBlocks: [...blocksRef.current] }));
          break;

        case "tool_use_end": {
          setToolInput(blocksRef.current, event.id, event.input);
          scheduleRender();
          break;
        }

        case "tool_result": {
          setToolResult(blocksRef.current, event.id, {
            content: event.content,
            isError: event.isError,
            diff: event.diff,
            image: event.image,
          });
          // Persist image in cache so it survives processing_complete
          if (event.image) {
            imageCacheRef.current[event.id] = event.image;
          }
          flushRender();
          break;
        }

        case "tool_progress": {
          setToolProgress(blocksRef.current, event.id, event.content);
          scheduleRender();
          break;
        }

        case "approval_required":
          setState((s) => ({ ...s, pendingApproval: { tools: event.tools } }));
          break;

        case "planner_question":
          setState((s) => ({
            ...s,
            pendingQuestion: { id: event.id, questions: event.questions },
          }));
          break;

        case "plan_review":
          setState((s) => ({
            ...s,
            pendingPlanReview: {
              id: event.id,
              title: event.title,
              plan: event.plan,
              tasks: event.tasks,
            },
          }));
          break;

        case "usage":
          setState((s) => ({
            ...s,
            tokenUsage: {
              input: s.tokenUsage.input + event.inputTokens,
              output: s.tokenUsage.output + event.outputTokens,
              cacheCreation: s.tokenUsage.cacheCreation + (event.cacheCreationTokens ?? 0),
              cacheRead: s.tokenUsage.cacheRead + (event.cacheReadTokens ?? 0),
            },
            contextSize: event.inputTokens, // last call's input = current context window
          }));
          break;

        case "mode_changed":
          setState((s) => ({ ...s, permissionMode: event.mode as PermissionMode }));
          break;

        case "error":
          blocksRef.current = [];
          blockIdCounter.current = 0;
          setState((s) => ({ ...s, error: event.error, isProcessing: false, streamingBlocks: [] }));
          break;

        case "processing_complete":
          blocksRef.current = [];
          blockIdCounter.current = 0;
          setState((s) => ({
            ...s,
            isProcessing: false,
            messages: event.messages,
            checkpoints: event.checkpoints,
            streamingBlocks: [],
            toolImages: { ...s.toolImages, ...imageCacheRef.current },
            contextSize: event.contextSize ?? s.contextSize,
          }));
          break;

        case "session_loaded":
          imageCacheRef.current = {};
          setState((s) => ({
            ...s,
            sessionId: event.sessionId,
            messages: event.messages,
            checkpoints: event.checkpoints,
            tokenUsage: event.tokenUsage,
            contextSize: event.contextSize,
            streamingBlocks: [],
            toolImages: {},
            subAgents: [],
            subAgentDetail: null,
          }));
          break;

        case "session_list":
          setState((s) => ({ ...s, sessionList: event.sessions }));
          break;

        case "model_list":
          setState((s) => ({ ...s, availableModels: event.models }));
          break;

        case "model_changed":
          setState((s) => ({
            ...s,
            providerName: event.providerName,
            modelName: event.modelName,
          }));
          break;

        case "screenshots_changed":
          setState((s) => ({ ...s, browserScreenshots: event.enabled }));
          break;

        case "config_values":
          setState((s) => ({ ...s, configValues: event.config }));
          break;

        case "config_saved":
          setState((s) => ({
            ...s,
            configValues: event.config,
            browserScreenshots:
              typeof event.config.browserScreenshots === "boolean"
                ? event.config.browserScreenshots
                : s.browserScreenshots,
          }));
          break;

        case "sub_agents_update":
          setState((s) => ({ ...s, subAgents: event.agents }));
          break;

        case "sub_agent_detail":
          setState((s) => ({ ...s, subAgentDetail: event.agent }));
          break;

        case "workers_update":
          setState((s) => ({ ...s, workers: event.workers }));
          break;

        case "worker_detail":
          setState((s) => ({ ...s, workerDetail: event.worker, workerRunDetail: null }));
          break;

        case "worker_run_detail":
          setState((s) => ({ ...s, workerRunDetail: event.run }));
          break;

        case "worker_notification":
          // Cancel any pending dismiss timer before setting new notification
          if (workerNotifyTimer.current) {
            clearTimeout(workerNotifyTimer.current);
          }
          setState((s) => ({
            ...s,
            workerNotification: {
              workerId: event.workerId,
              workerName: event.workerName,
              status: event.status,
              summary: event.summary,
            },
          }));
          workerNotifyTimer.current = setTimeout(() => {
            workerNotifyTimer.current = null;
            setState((s) => ({ ...s, workerNotification: null }));
          }, 5000);
          break;

        case "queued_message_injected":
          // Agent consumed a queued message — clear streaming for next turn
          blocksRef.current = [];
          setState((s) => ({ ...s, streamingBlocks: [] }));
          break;

        case "auth_required":
          setState((s) => ({ ...s, authState: "required" }));
          break;

        case "auth_ok":
          storeToken(event.token);
          setState((s) => ({ ...s, authState: "authenticated", authError: null }));
          break;

        case "auth_error":
          clearToken();
          setState((s) => ({ ...s, authState: "required", authError: event.error }));
          break;

        default:
          break;
      }
    },
    [scheduleRender, flushRender],
  );

  // Connect WebSocket
  useEffect(() => {
    const protocol = window.location.protocol === "https:" ? "wss:" : "ws:";
    const wsUrl = `${protocol}//${window.location.host}/ws`;
    const ws = new WebSocketClient(wsUrl);
    wsRef.current = ws;

    ws.onStatus((connected) => setState((s) => ({ ...s, connected })));
    ws.onMessage(handleMessage);
    ws.connect().catch(() => {});

    return () => ws.close();
  }, [handleMessage]);

  // ── Actions ──────────────────────────────────────────────────────

  const sendMessage = useCallback((content: string) => {
    wsRef.current?.send({ type: "send_message", content });
    // Optimistically add user message
    setState((s) => ({
      ...s,
      messages: [...s.messages, { role: "user" as const, content }],
    }));
  }, []);

  const approveTools = useCallback((approvedIds: string[]) => {
    wsRef.current?.send({ type: "approve", approvedIds });
    setState((s) => ({ ...s, pendingApproval: null }));
  }, []);

  const denyAllTools = useCallback(() => {
    wsRef.current?.send({ type: "deny_all" });
    setState((s) => ({ ...s, pendingApproval: null }));
  }, []);

  const answerQuestion = useCallback((answer: string) => {
    wsRef.current?.send({ type: "answer_question", answer });
    setState((s) => ({ ...s, pendingQuestion: null }));
  }, []);

  const acceptPlan = useCallback((tasks?: Array<{ title: string; detail: string }>) => {
    wsRef.current?.send({ type: "plan_accept", tasks });
    setState((s) => ({ ...s, pendingPlanReview: null }));
  }, []);

  const rejectPlan = useCallback((feedback: string) => {
    wsRef.current?.send({ type: "plan_reject", feedback });
    setState((s) => ({ ...s, pendingPlanReview: null }));
  }, []);

  const abort = useCallback(() => {
    wsRef.current?.send({ type: "abort" });
  }, []);

  const loadSession = useCallback((sessionId: string) => {
    wsRef.current?.send({ type: "load_session", sessionId });
  }, []);

  const newSession = useCallback((name?: string) => {
    wsRef.current?.send({ type: "new_session", name });
  }, []);

  const listSessions = useCallback(() => {
    wsRef.current?.send({ type: "list_sessions" });
  }, []);

  const setPermissionMode = useCallback((mode: PermissionMode) => {
    wsRef.current?.send({ type: "set_permission_mode", mode });
  }, []);

  const deleteSession = useCallback((sessionId: string) => {
    wsRef.current?.send({ type: "delete_session", sessionId });
  }, []);

  const dismissError = useCallback(() => {
    setState((s) => ({ ...s, error: null }));
  }, []);

  const dismissSessionList = useCallback(() => {
    setState((s) => ({ ...s, sessionList: null }));
  }, []);

  const listModels = useCallback(() => {
    wsRef.current?.send({ type: "list_models" });
  }, []);

  const setModel = useCallback((model: string) => {
    wsRef.current?.send({ type: "set_model", model });
  }, []);

  const setScreenshots = useCallback((enabled: boolean) => {
    wsRef.current?.send({ type: "set_screenshots", enabled });
  }, []);

  const getConfig = useCallback(() => {
    wsRef.current?.send({ type: "get_config" });
  }, []);

  const setConfig = useCallback((config: Record<string, unknown>) => {
    wsRef.current?.send({ type: "set_config", config });
  }, []);

  const getSubAgentDetail = useCallback((id: string) => {
    wsRef.current?.send({ type: "get_sub_agent_detail", id });
  }, []);

  const abortSubAgent = useCallback((id: string) => {
    wsRef.current?.send({ type: "abort_sub_agent", id });
  }, []);

  const dismissSubAgentDetail = useCallback(() => {
    setState((s) => ({ ...s, subAgentDetail: null }));
  }, []);

  const login = useCallback((password: string) => {
    wsRef.current?.send({ type: "auth_login", password });
  }, []);

  // ── Worker actions ──────────────────────────────────────────────

  const listWorkers = useCallback(() => {
    wsRef.current?.send({ type: "list_workers" });
  }, []);

  const createWorker = useCallback((worker: WorkerCreateInput) => {
    wsRef.current?.send({ type: "create_worker", worker });
  }, []);

  const updateWorker = useCallback((id: string, patch: WorkerUpdateInput) => {
    wsRef.current?.send({ type: "update_worker", id, patch });
  }, []);

  const deleteWorker = useCallback((id: string) => {
    wsRef.current?.send({ type: "delete_worker", id });
  }, []);

  const toggleWorker = useCallback((id: string, enabled: boolean) => {
    wsRef.current?.send({ type: "toggle_worker", id, enabled });
  }, []);

  const getWorkerDetail = useCallback((id: string) => {
    wsRef.current?.send({ type: "get_worker_detail", id });
  }, []);

  const getWorkerRunDetail = useCallback((workerId: string, runId: string) => {
    wsRef.current?.send({ type: "get_worker_run_detail", workerId, runId });
  }, []);

  const dismissWorkerDetail = useCallback(() => {
    setState((s) => ({ ...s, workerDetail: null, workerRunDetail: null }));
  }, []);

  return {
    ...state,
    sendMessage,
    approveTools,
    denyAllTools,
    answerQuestion,
    acceptPlan,
    rejectPlan,
    abort,
    loadSession,
    newSession,
    listSessions,
    deleteSession,
    setPermissionMode,
    dismissError,
    dismissSessionList,
    listModels,
    setModel,
    setScreenshots,
    getConfig,
    setConfig,
    getSubAgentDetail,
    abortSubAgent,
    dismissSubAgentDetail,
    login,
    listWorkers,
    createWorker,
    updateWorker,
    deleteWorker,
    toggleWorker,
    getWorkerDetail,
    getWorkerRunDetail,
    dismissWorkerDetail,
  };
}
