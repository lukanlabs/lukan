import { useRef, useEffect, useMemo, useState, useCallback } from "react";
import { AlertCircle } from "lucide-react";
import logoUrl from "../assets/logo.png";
import { useChat } from "../hooks/useChat";
import type { ToolResultInfo } from "../components/chat/MessageBubble";
import type { Message } from "../lib/types";
import { StatusBar } from "../components/chat/StatusBar";
import { MessageBubble } from "../components/chat/MessageBubble";
import { StreamingText } from "../components/chat/StreamingText";
import { ToolCallCard } from "../components/chat/ToolCallCard";
import { ChatInput } from "../components/chat/ChatInput";
import { InlineApproval } from "../components/chat/InlineApproval";
import { PlanReviewer } from "../components/chat/PlanReviewer";
import { QuestionPicker } from "../components/chat/QuestionPicker";

function buildToolResultsMap(
  messages: Message[],
  toolImages?: Record<string, string>,
): Map<string, ToolResultInfo> {
  const map = new Map<string, ToolResultInfo>();
  for (const msg of messages) {
    if (!Array.isArray(msg.content)) continue;
    for (const block of msg.content) {
      if (block.type === "tool_result") {
        map.set(block.toolUseId, {
          content: block.content,
          isError: block.isError,
          diff: block.diff,
          image: toolImages?.[block.toolUseId] ?? block.image,
        });
      }
    }
  }
  return map;
}

export default function ChatView() {
  const chat = useChat();
  const scrollRef = useRef<HTMLDivElement>(null);
  const scrollContainerRef = useRef<HTMLDivElement>(null);
  const [autoScroll, setAutoScroll] = useState(true);
  // Listen for sidebar session events
  useEffect(() => {
    const onLoad = (e: Event) => {
      const id = (e as CustomEvent<string>).detail;
      chat.loadSession(id);
    };
    const onNew = () => {
      chat.newSession();
    };
    const onInjectEvent = (e: Event) => {
      const text = (e as CustomEvent<string>).detail;
      if (text && !chat.isProcessing) {
        setAutoScroll(true);
        chat.sendMessage(text);
      }
    };
    window.addEventListener("load-session", onLoad);
    window.addEventListener("new-session", onNew);
    window.addEventListener("inject-event", onInjectEvent);
    return () => {
      window.removeEventListener("load-session", onLoad);
      window.removeEventListener("new-session", onNew);
      window.removeEventListener("inject-event", onInjectEvent);
    };
  }, [chat.loadSession, chat.newSession, chat.sendMessage, chat.isProcessing]);

  // Auto-scroll to bottom on new content
  useEffect(() => {
    if (autoScroll) {
      scrollRef.current?.scrollIntoView({ behavior: "smooth" });
    }
  }, [chat.messages, chat.streamingBlocks, autoScroll]);

  // Pause auto-scroll when user scrolls up
  const handleScroll = useCallback(() => {
    const el = scrollContainerRef.current;
    if (!el) return;
    const atBottom = el.scrollHeight - el.scrollTop - el.clientHeight < 100;
    setAutoScroll(atBottom);
  }, []);

  // Resume auto-scroll on send
  const handleSend = useCallback(
    (content: string) => {
      setAutoScroll(true);
      chat.sendMessage(content);
    },
    [chat.sendMessage],
  );

  // Keyboard shortcuts
  useEffect(() => {
    const handler = (e: KeyboardEvent) => {
      if (e.ctrlKey && e.key === "n") {
        e.preventDefault();
        chat.newSession();
      }
      if (e.ctrlKey && e.key === "k") {
        e.preventDefault();
        // Focus input — handled by ChatInput's own focus
      }
      if (e.key === "Escape" && chat.isProcessing) {
        chat.abort();
      }
    };
    window.addEventListener("keydown", handler);
    return () => window.removeEventListener("keydown", handler);
  }, [chat.newSession, chat.abort, chat.isProcessing]);

  const isEmpty = chat.messages.length === 0 && chat.streamingBlocks.length === 0;

  const toolResultsMap = useMemo(
    () => buildToolResultsMap(chat.messages, chat.toolImages),
    [chat.messages, chat.toolImages],
  );

  return (
    <div className="flex flex-1 flex-col min-h-0 bg-zinc-950">
      <StatusBar
        tokenUsage={chat.tokenUsage}
        contextSize={chat.contextSize}
        onNewSession={chat.newSession}
      />

      <div className="flex flex-1 min-h-0">
        {/* Main chat area */}
        <div className="flex flex-1 flex-col min-h-0">
          <div
            ref={scrollContainerRef}
            className="flex-1 overflow-y-auto px-4 py-6"
            onScroll={handleScroll}
          >
            {isEmpty && (
              <div className="flex flex-col items-center justify-center h-full text-zinc-400 gap-4 pt-32">
                <div className="relative">
                  <img src={logoUrl} alt="lukan" className="h-32 w-32 animate-pulse-subtle" style={{ imageRendering: "auto" }} />
                </div>
                <div className="text-center space-y-2">
                  <h2 className="text-xl font-semibold text-zinc-200">Welcome to lukan</h2>
                  <p className="text-sm text-zinc-500 max-w-xs">
                    Your AI-powered assistant. Tell me what to do and I'll get it done.
                  </p>
                </div>
              </div>
            )}

            {/* Messages */}
            <div className="space-y-1 max-w-4xl mx-auto">
              {chat.messages.map((msg, i) => (
                <MessageBubble key={`msg-${i}`} message={msg} toolResultsMap={toolResultsMap} />
              ))}
            </div>

            {/* Streaming blocks */}
            {chat.streamingBlocks.length > 0 && (
              <div className="mb-4 flex justify-start animate-fade-in max-w-4xl mx-auto">
                <div className="flex gap-3 w-full max-w-4xl">
                  <div className="mt-1 flex h-8 w-8 shrink-0 items-center justify-center" style={{ perspective: 200 }}>
                    <img src={logoUrl} alt="" className={`h-5 w-5 ${chat.isProcessing ? "animate-logo-rock" : ""}`} style={{ imageRendering: "auto" }} />
                  </div>
                  <div className="min-w-0 flex-1">
                    <div className="space-y-1.5">
                      {chat.streamingBlocks.map((block) => {
                        switch (block.type) {
                          case "text":
                            return <StreamingText key={block.id} text={block.text} />;
                          case "thinking":
                            return (
                              <div
                                key={block.id}
                                className="rounded-lg bg-zinc-900/30 border border-zinc-800/50 px-3 py-2 text-xs text-zinc-500 italic max-h-48 overflow-y-auto whitespace-pre-wrap break-words"
                                ref={(el) => {
                                  if (el) el.scrollTop = el.scrollHeight;
                                }}
                              >
                                {block.text}
                                <span className="inline-block w-0.5 h-3 bg-zinc-600 ml-0.5 align-text-bottom animate-blink" />
                              </div>
                            );
                          case "tool":
                            return <ToolCallCard key={block.id} tool={block.tool} />;
                          case "approval":
                            return (
                              <InlineApproval
                                key={block.id}
                                tools={block.tools}
                                onApprove={chat.approveTools}
                                onAlwaysAllow={chat.alwaysAllowTools}
                                onDenyAll={chat.denyAllTools}
                              />
                            );
                          default:
                            return null;
                        }
                      })}
                    </div>
                  </div>
                </div>
              </div>
            )}

            {/* Processing indicator */}
            {chat.isProcessing && chat.streamingBlocks.length === 0 && (
              <div className="flex items-center gap-3 py-4 animate-fade-in max-w-4xl mx-auto">
                <div className="flex h-8 w-8 items-center justify-center" style={{ perspective: 200 }}>
                  <img src={logoUrl} alt="" className="h-5 w-5 animate-logo-rock" style={{ imageRendering: "auto" }} />
                </div>
                <div className="flex gap-1">
                  <span className="typing-dot w-2 h-2 rounded-full bg-zinc-500" />
                  <span className="typing-dot w-2 h-2 rounded-full bg-zinc-500" />
                  <span className="typing-dot w-2 h-2 rounded-full bg-zinc-500" />
                </div>
              </div>
            )}

            {/* Error display */}
            {chat.error && (
              <div
                className="flex items-start gap-3 rounded-xl border border-red-500/20 bg-red-500/10 px-4 py-3 text-sm text-red-300 cursor-pointer hover:bg-red-500/15 transition-colors my-4 max-w-2xl mx-auto"
                onClick={chat.dismissError}
              >
                <AlertCircle className="h-4 w-4 shrink-0 mt-0.5" />
                <div className="flex-1">
                  <p className="font-medium text-red-200">Error</p>
                  <p className="text-red-400/80 mt-1">{chat.error}</p>
                </div>
                <span className="text-[10px] text-red-400/50">Click to dismiss</span>
              </div>
            )}

            <div ref={scrollRef} className="h-4" />
          </div>

          <ChatInput
            onSend={handleSend}
            onAbort={chat.abort}
            isProcessing={chat.isProcessing}
            permissionMode={chat.permissionMode}
            onSetPermissionMode={chat.setPermissionMode}
          />
        </div>
      </div>

      {/* Modals */}
      {chat.pendingPlanReview && (
        <PlanReviewer
          title={chat.pendingPlanReview.title}
          plan={chat.pendingPlanReview.plan}
          tasks={chat.pendingPlanReview.tasks}
          onAccept={chat.acceptPlan}
          onReject={chat.rejectPlan}
        />
      )}

      {chat.pendingQuestion && (
        <QuestionPicker
          questions={chat.pendingQuestion.questions}
          onSubmit={chat.answerQuestion}
        />
      )}
    </div>
  );
}
