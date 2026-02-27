import { useState, useEffect, useCallback, useRef } from "react";
import type {
  Message,
  StreamEvent,
  PermissionMode,
  SessionSummary,
  TokenUsage,
  TurnComplete,
  PlannerQuestion,
  PlanTask,
  ToolApprovalRequest,
} from "../lib/types";
import {
  appendTextDelta,
  appendThinkingDelta,
  setToolInput,
  setToolProgress,
  setToolResult,
  startTool,
} from "../lib/streaming-blocks";
import * as api from "../lib/tauri";

// ── Types ────────────────────────────────────────────────────────────

export interface ToolStatus {
  id: string;
  name: string;
  isRunning: boolean;
  isHistorical?: boolean;
  rawInput?: Record<string, unknown>;
  content?: string;
  isError?: boolean;
  diff?: string;
  image?: string;
}

export type StreamingBlock =
  | { type: "text"; id: string; text: string }
  | { type: "thinking"; id: string; text: string }
  | { type: "tool"; id: string; tool: ToolStatus }
  | { type: "approval"; id: string; tools: ToolApprovalRequest[] };

export interface PendingApproval {
  tools: ToolApprovalRequest[];
}

export interface PendingQuestion {
  id: string;
  questions: PlannerQuestion[];
}

export interface PendingPlanReview {
  id: string;
  title: string;
  plan: string;
  tasks: PlanTask[];
}

export interface ChatState {
  initialized: boolean;
  sessionId: string;
  messages: Message[];
  streamingBlocks: StreamingBlock[];
  isProcessing: boolean;
  pendingApproval: PendingApproval | null;
  pendingQuestion: PendingQuestion | null;
  pendingPlanReview: PendingPlanReview | null;
  permissionMode: PermissionMode;
  tokenUsage: TokenUsage;
  contextSize: number;
  providerName: string;
  modelName: string;
  error: string | null;
  sessionList: SessionSummary[] | null;
  toolImages: Record<string, string>;
}

// ── Hook ─────────────────────────────────────────────────────────────

export function useChat() {
  const [state, setState] = useState<ChatState>({
    initialized: false,
    sessionId: "",
    messages: [],
    streamingBlocks: [],
    isProcessing: false,
    pendingApproval: null,
    pendingQuestion: null,
    pendingPlanReview: null,
    permissionMode: "auto",
    tokenUsage: { input: 0, output: 0, cacheCreation: null, cacheRead: null },
    contextSize: 0,
    providerName: "",
    modelName: "",
    error: null,
    sessionList: null,
    toolImages: {},
  });

  const blocksRef = useRef<StreamingBlock[]>([]);
  const blockIdCounter = useRef(0);
  const renderTimer = useRef<ReturnType<typeof setTimeout> | null>(null);
  const imageCacheRef = useRef<Record<string, string>>({});
  // Pending text extracted before tool calls — gets prepended to the next text block
  // so text split mid-word by tool calls merges back into one continuous block.
  const pendingTextRef = useRef<string>("");

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

  // Handle a stream event from the backend
  const handleStreamEvent = useCallback(
    (event: StreamEvent) => {
      switch (event.type) {
        case "message_start":
          pendingTextRef.current = "";
          setState((s) => ({ ...s, isProcessing: true, error: null }));
          break;

        case "text_delta": {
          // If there's pending text from before a tool call, prepend it
          // so text split mid-word by tool calls merges back together.
          const prefix = pendingTextRef.current;
          if (prefix) {
            pendingTextRef.current = "";
            appendTextDelta(blocksRef.current, prefix + event.text, () => `txt-${blockIdCounter.current++}`);
          } else {
            appendTextDelta(blocksRef.current, event.text, () => `txt-${blockIdCounter.current++}`);
          }
          scheduleRender();
          break;
        }

        case "thinking_delta":
          appendThinkingDelta(blocksRef.current, event.text, () => `thk-${blockIdCounter.current++}`);
          scheduleRender();
          break;

        case "tool_use_start": {
          flushRender();
          // Extract the last text block and save it as pending text.
          // When the next text_delta arrives after the tool, it will be
          // prepended — merging text that was split mid-word by tool calls.
          const lastBlock = blocksRef.current[blocksRef.current.length - 1];
          if (lastBlock?.type === "text" && lastBlock.text) {
            pendingTextRef.current += lastBlock.text;
            blocksRef.current.pop();
          }
          startTool(blocksRef.current, event.id, event.name, (tool) => ({
            type: "tool",
            id: event.id,
            tool,
          }));
          setState((s) => ({ ...s, streamingBlocks: [...blocksRef.current] }));
          break;
        }

        case "tool_use_end":
          setToolInput(blocksRef.current, event.id, event.input);
          scheduleRender();
          break;

        case "tool_result":
          setToolResult(blocksRef.current, event.id, {
            content: event.content,
            isError: event.isError,
            diff: event.diff,
            image: event.image,
          });
          if (event.image) {
            imageCacheRef.current[event.id] = event.image;
          }
          flushRender();
          break;

        case "tool_progress":
          setToolProgress(blocksRef.current, event.id, event.content);
          scheduleRender();
          break;

        case "explore_progress": {
          // Replace (not accumulate) the Explore tool card content with latest activity
          const exploreBlock = blocksRef.current.find(
            (b): b is Extract<StreamingBlock, { type: "tool" }> => b.type === "tool" && b.tool.id === event.id,
          );
          if (exploreBlock) {
            exploreBlock.tool = { ...exploreBlock.tool, content: event.activity };
          }
          scheduleRender();
          break;
        }

        case "approval_required":
          blocksRef.current.push({
            type: "approval",
            id: `approval-${blockIdCounter.current++}`,
            tools: event.tools,
          } as StreamingBlock);
          flushRender();
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
              cacheCreation: (s.tokenUsage.cacheCreation ?? 0) + (event.cacheCreationTokens ?? 0),
              cacheRead: (s.tokenUsage.cacheRead ?? 0) + (event.cacheReadTokens ?? 0),
            },
            contextSize: event.inputTokens,
          }));
          break;

        case "mode_changed":
          setState((s) => ({ ...s, permissionMode: event.mode as PermissionMode }));
          break;

        case "error":
          blocksRef.current = [];
          blockIdCounter.current = 0;
          pendingTextRef.current = "";
          setState((s) => ({ ...s, error: event.error, isProcessing: false, streamingBlocks: [] }));
          break;

        default:
          break;
      }
    },
    [scheduleRender, flushRender],
  );

  // Handle turn complete
  const handleTurnComplete = useCallback((complete: TurnComplete) => {
    blocksRef.current = [];
    blockIdCounter.current = 0;
    pendingTextRef.current = "";
    setState((s) => ({
      ...s,
      isProcessing: false,
      messages: complete.messages,
      streamingBlocks: [],
      toolImages: { ...s.toolImages, ...imageCacheRef.current },
      contextSize: complete.contextSize ?? s.contextSize,
      tokenUsage: {
        input: complete.tokenUsage.input,
        output: complete.tokenUsage.output,
        cacheCreation: complete.tokenUsage.cacheCreation,
        cacheRead: complete.tokenUsage.cacheRead,
      },
    }));
  }, []);

  // Subscribe to Tauri events
  useEffect(() => {
    let mounted = true;

    const setup = async () => {
      // Initialize chat
      try {
        const init = await api.initializeChat();
        if (!mounted) return;
        setState((s) => ({
          ...s,
          initialized: true,
          sessionId: init.sessionId,
          messages: init.messages,
          providerName: init.providerName,
          modelName: init.modelName,
          permissionMode: init.permissionMode as PermissionMode,
          tokenUsage: {
            input: init.tokenUsage.input,
            output: init.tokenUsage.output,
            cacheCreation: init.tokenUsage.cacheCreation,
            cacheRead: init.tokenUsage.cacheRead,
          },
          contextSize: init.contextSize,
        }));
      } catch (e) {
        if (!mounted) return;
        setState((s) => ({ ...s, error: `Init failed: ${e}`, initialized: true }));
      }

      // Subscribe to stream events
      const unlistenStream = await api.onStreamEvent((payload) => {
        if (!mounted) return;
        try {
          const event: StreamEvent = JSON.parse(payload);
          handleStreamEvent(event);
        } catch {
          // ignore parse errors
        }
      });

      const unlistenComplete = await api.onTurnComplete((payload) => {
        if (!mounted) return;
        try {
          const complete: TurnComplete = JSON.parse(payload);
          handleTurnComplete(complete);
        } catch {
          // ignore parse errors
        }
      });

      return () => {
        mounted = false;
        unlistenStream();
        unlistenComplete();
      };
    };

    const cleanupPromise = setup();

    return () => {
      mounted = false;
      cleanupPromise.then((cleanup) => cleanup?.());
    };
  }, [handleStreamEvent, handleTurnComplete]);

  // Listen for provider/model changes from Toolbar or ProvidersTab
  useEffect(() => {
    const handleProviderChanged = async () => {
      try {
        const init = await api.initializeChat();
        setState((s) => ({
          ...s,
          providerName: init.providerName,
          modelName: init.modelName,
          // If agent was cleared (provider changed), reset session state
          ...(init.sessionId === "" ? {
            sessionId: "",
            messages: [],
            streamingBlocks: [],
            toolImages: {},
            tokenUsage: { input: 0, output: 0, cacheCreation: null, cacheRead: null },
            contextSize: 0,
          } : {}),
        }));
      } catch {
        // ignore
      }
    };
    window.addEventListener("provider-changed", handleProviderChanged);
    return () => window.removeEventListener("provider-changed", handleProviderChanged);
  }, []);

  // ── Actions ──────────────────────────────────────────────────────

  const sendMessage = useCallback((content: string) => {
    // Optimistically add user message
    setState((s) => ({
      ...s,
      messages: [...s.messages, { role: "user" as const, content }],
    }));
    api.sendMessage(content).catch((e) => {
      setState((s) => ({ ...s, error: `Send failed: ${e}` }));
    });
  }, []);

  const abort = useCallback(() => {
    api.cancelStream().catch(() => {});
    blocksRef.current = [];
    blockIdCounter.current = 0;
    setState((s) => ({ ...s, isProcessing: false, streamingBlocks: [] }));
  }, []);

  const clearApprovalBlocks = useCallback(() => {
    blocksRef.current = blocksRef.current.filter((b) => b.type !== "approval");
    flushRender();
  }, [flushRender]);

  const approveTools = useCallback((approvedIds: string[]) => {
    api.approveTools(approvedIds).catch(() => {});
    clearApprovalBlocks();
    setState((s) => ({ ...s, pendingApproval: null }));
  }, [clearApprovalBlocks]);

  const alwaysAllowTools = useCallback((approvedIds: string[], tools: ToolApprovalRequest[]) => {
    api.alwaysAllowTools(approvedIds, tools).catch(() => {});
    clearApprovalBlocks();
    setState((s) => ({ ...s, pendingApproval: null }));
  }, [clearApprovalBlocks]);

  const denyAllTools = useCallback(() => {
    api.denyAllTools().catch(() => {});
    clearApprovalBlocks();
    setState((s) => ({ ...s, pendingApproval: null }));
  }, [clearApprovalBlocks]);

  const answerQuestion = useCallback((answer: string) => {
    api.answerQuestion(answer).catch(() => {});
    setState((s) => ({ ...s, pendingQuestion: null }));
  }, []);

  const acceptPlan = useCallback((tasks?: Array<{ title: string; detail: string }>) => {
    api.acceptPlan(tasks).catch(() => {});
    setState((s) => ({ ...s, pendingPlanReview: null }));
  }, []);

  const rejectPlan = useCallback((feedback: string) => {
    api.rejectPlan(feedback).catch(() => {});
    setState((s) => ({ ...s, pendingPlanReview: null }));
  }, []);

  const doListSessions = useCallback(async () => {
    try {
      const sessions = await api.listSessions();
      setState((s) => ({ ...s, sessionList: sessions }));
    } catch (e) {
      setState((s) => ({ ...s, error: `Failed to list sessions: ${e}` }));
    }
  }, []);

  const doLoadSession = useCallback(async (id: string) => {
    try {
      imageCacheRef.current = {};
      const init = await api.loadSession(id);
      setState((s) => ({
        ...s,
        sessionId: init.sessionId,
        messages: init.messages,
        streamingBlocks: [],
        toolImages: {},
        tokenUsage: {
          input: init.tokenUsage.input,
          output: init.tokenUsage.output,
          cacheCreation: init.tokenUsage.cacheCreation,
          cacheRead: init.tokenUsage.cacheRead,
        },
        contextSize: init.contextSize,
        sessionList: null,
      }));
    } catch (e) {
      setState((s) => ({ ...s, error: `Failed to load session: ${e}` }));
    }
  }, []);

  const doNewSession = useCallback(async () => {
    try {
      imageCacheRef.current = {};
      const init = await api.newSession();
      setState((s) => ({
        ...s,
        sessionId: init.sessionId,
        messages: [],
        streamingBlocks: [],
        toolImages: {},
        tokenUsage: { input: 0, output: 0, cacheCreation: null, cacheRead: null },
        contextSize: 0,
      }));
    } catch (e) {
      setState((s) => ({ ...s, error: `Failed to create session: ${e}` }));
    }
  }, []);

  const doSetPermissionMode = useCallback((mode: PermissionMode) => {
    api.setPermissionMode(mode).catch(() => {});
    setState((s) => ({ ...s, permissionMode: mode }));
  }, []);

  const dismissError = useCallback(() => {
    setState((s) => ({ ...s, error: null }));
  }, []);

  const dismissSessionList = useCallback(() => {
    setState((s) => ({ ...s, sessionList: null }));
  }, []);

  return {
    ...state,
    sendMessage,
    abort,
    approveTools,
    alwaysAllowTools,
    denyAllTools,
    answerQuestion,
    acceptPlan,
    rejectPlan,
    listSessions: doListSessions,
    loadSession: doLoadSession,
    newSession: doNewSession,
    setPermissionMode: doSetPermissionMode,
    dismissError,
    dismissSessionList,
  };
}
