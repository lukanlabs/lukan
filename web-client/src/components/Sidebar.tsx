import {
  Plus,
  Loader2,
  Trash2,
  MessageCircle,
  Settings,
  ChevronDown,
  ChevronRight,
  ArrowLeft,
} from "lucide-react";
import React, { useState, useCallback, useRef, useEffect } from "react";
import type { SessionSummary } from "../lib/types.ts";
import logoUrl from "../assets/logo.png";
import { SettingsPanel } from "./SettingsPanel.tsx";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
  DialogDescription,
  DialogFooter,
} from "@/components/ui/dialog";
import { ScrollArea } from "@/components/ui/scroll-area";
import { Separator } from "@/components/ui/separator";
import { cn } from "@/lib/utils";

const ITEMS_PER_PAGE = 10;

interface SidebarProps {
  sessions: SessionSummary[] | null;
  currentSessionId: string;
  configValues: Record<string, unknown> | null;
  onSelectSession: (id: string) => void;
  onNewSession: (name?: string) => void;
  onListSessions: () => void;
  onDeleteSession?: (id: string) => void;
  onRequestConfig: () => void;
  onSaveConfig: (config: Record<string, unknown>) => void;
}

export function Sidebar({
  sessions,
  currentSessionId,
  configValues,
  onSelectSession,
  onNewSession,
  onListSessions,
  onDeleteSession,
  onRequestConfig,
  onSaveConfig,
}: SidebarProps) {
  const [view, setView] = useState<"sessions" | "settings">("sessions");
  const [visibleCount, setVisibleCount] = useState(ITEMS_PER_PAGE);
  const [isLoadingMore, setIsLoadingMore] = useState(false);
  const [deletingId, setDeletingId] = useState<string | null>(null);
  const [confirmDeleteId, setConfirmDeleteId] = useState<string | null>(null);
  const [sessionsOpen, setSessionsOpen] = useState(true);
  const scrollRef = useRef<HTMLDivElement>(null);

  const visibleSessions = sessions?.slice(0, visibleCount) ?? [];
  const hasMore = sessions && visibleCount < sessions.length;

  // Auto-load sessions on mount
  useEffect(() => {
    if (!sessions) {
      onListSessions();
    }
  }, [sessions, onListSessions]);

  const handleLoadMore = useCallback(() => {
    if (isLoadingMore || !hasMore) return;
    setIsLoadingMore(true);
    setTimeout(() => {
      setVisibleCount((prev) => Math.min(prev + ITEMS_PER_PAGE, sessions?.length ?? 0));
      setIsLoadingMore(false);
    }, 100);
  }, [isLoadingMore, hasMore, sessions?.length]);

  // Infinite scroll handler
  useEffect(() => {
    const scrollElement = scrollRef.current;
    if (!scrollElement) return;

    const handleScroll = () => {
      const isAtBottom =
        scrollElement.scrollHeight - scrollElement.scrollTop <= scrollElement.clientHeight + 50;
      if (isAtBottom) {
        handleLoadMore();
      }
    };

    scrollElement.addEventListener("scroll", handleScroll);
    return () => scrollElement.removeEventListener("scroll", handleScroll);
  }, [handleLoadMore]);

  // Reset visible count when sessions change
  useEffect(() => {
    if (sessions) {
      setVisibleCount(Math.min(ITEMS_PER_PAGE, sessions.length));
    }
  }, [sessions]);

  const handleDelete = (e: React.MouseEvent, sessionId: string) => {
    e.stopPropagation();
    setConfirmDeleteId(sessionId);
  };

  const confirmDelete = () => {
    if (!confirmDeleteId) return;
    setDeletingId(confirmDeleteId);
    onDeleteSession?.(confirmDeleteId);
    setConfirmDeleteId(null);
    setTimeout(() => setDeletingId(null), 500);
  };

  const formatDate = (dateStr: string) => {
    const date = new Date(dateStr);
    const now = new Date();
    const diffMs = now.getTime() - date.getTime();
    const diffDays = Math.floor(diffMs / (1000 * 60 * 60 * 24));

    if (diffDays === 0) {
      return "Today";
    } else if (diffDays === 1) {
      return "Yesterday";
    } else if (diffDays < 7) {
      return `${diffDays}d ago`;
    } else {
      return date.toLocaleDateString("en-US", { month: "short", day: "numeric" });
    }
  };

  return (
    <div className="flex h-full w-64 flex-col bg-zinc-950 border-r border-zinc-800">
      {/* Logo / Brand */}
      <div className="flex items-center gap-3 px-4 py-4 border-b border-zinc-800">
        <div className="flex h-8 w-8 items-center justify-center rounded-lg bg-zinc-800 border border-zinc-700">
          <img src={logoUrl} alt="lukan" className="h-5 w-5" />
        </div>
        <div className="flex flex-col">
          <span className="text-sm font-semibold text-zinc-100">lukan</span>
          <span className="text-[10px] text-zinc-500">AI Assistant</span>
        </div>
      </div>

      {/* New Session button */}
      <div className="px-3 pt-3 pb-2">
        <Button
          className="w-full justify-start gap-2 bg-zinc-100 text-zinc-900 hover:bg-zinc-200 transition-colors border-0"
          size="sm"
          onClick={() => onNewSession()}
        >
          <Plus className="h-3.5 w-3.5" />
          New Session
        </Button>
      </div>

      <Separator className="bg-zinc-800 mx-3" />

      {/* Navigation */}
      <div className="flex-1 flex flex-col min-h-0">
        {view === "settings" ? (
          <>
            {/* Settings header with back button */}
            <div className="px-3 pt-3">
              <button
                onClick={() => setView("sessions")}
                className="w-full flex items-center gap-2.5 px-3 py-2 rounded-md bg-zinc-800/50 text-zinc-200 text-sm font-medium hover:bg-zinc-800 transition-colors"
              >
                <ArrowLeft className="h-4 w-4 text-zinc-400" />
                <span className="flex-1 text-left">Settings</span>
              </button>
            </div>
            <SettingsPanel
              configValues={configValues}
              onRequestConfig={onRequestConfig}
              onSaveConfig={onSaveConfig}
            />
          </>
        ) : (
          <>
            {/* Agent Sessions — collapsible */}
            <div className="px-3 pt-3">
              <button
                onClick={() => setSessionsOpen(!sessionsOpen)}
                className="w-full flex items-center gap-2.5 px-3 py-2 rounded-md bg-zinc-800/50 text-zinc-200 text-sm font-medium hover:bg-zinc-800 transition-colors"
              >
                <MessageCircle className="h-4 w-4 text-zinc-400" />
                <span className="flex-1 text-left">Agent Sessions</span>
                {sessions && <span className="text-[10px] text-zinc-500">{sessions.length}</span>}
                {sessionsOpen ? (
                  <ChevronDown className="h-3.5 w-3.5 text-zinc-500" />
                ) : (
                  <ChevronRight className="h-3.5 w-3.5 text-zinc-500" />
                )}
              </button>
            </div>

            {/* Sessions list — shown when expanded */}
            {sessionsOpen && (
              <ScrollArea className="flex-1 px-3 mt-1" ref={scrollRef}>
                {sessions && sessions.length > 0 ? (
                  <div className="space-y-0.5 py-1 pl-2">
                    {visibleSessions.map((s, index) => (
                      <button
                        key={s.id}
                        onClick={() => onSelectSession(s.id)}
                        className={cn(
                          "w-full rounded-md px-3 py-2 text-left text-sm transition-all duration-150 group relative",
                          s.id === currentSessionId
                            ? "bg-zinc-800 border border-zinc-700 text-zinc-100"
                            : "text-zinc-400 hover:bg-zinc-900 hover:text-zinc-200 border border-transparent",
                        )}
                        style={{ animationDelay: `${index * 15}ms` }}
                      >
                        <div className="flex items-start justify-between gap-2">
                          <div className="min-w-0 flex-1">
                            <div className="flex items-center gap-2">
                              <span className="truncate font-medium text-xs text-zinc-300">
                                {s.name || s.lastUserMessage?.slice(0, 25) || "New Session"}
                              </span>
                              <span className="shrink-0 text-[10px] text-zinc-600">
                                {formatDate(s.updatedAt)}
                              </span>
                            </div>
                            <div className="mt-0.5 truncate text-[11px] text-zinc-500">
                              {s.lastUserMessage?.slice(0, 40) || "Empty conversation"}
                            </div>
                          </div>

                          {/* Delete button */}
                          {onDeleteSession && (
                            <button
                              onClick={(e) => handleDelete(e, s.id)}
                              disabled={deletingId === s.id}
                              className={cn(
                                "absolute right-2 top-1/2 -translate-y-1/2 p-1 rounded transition-all duration-150",
                                "opacity-0 group-hover:opacity-100",
                                "hover:bg-red-500/20 text-zinc-600 hover:text-red-400",
                                deletingId === s.id && "opacity-50",
                              )}
                              title="Delete session"
                            >
                              {deletingId === s.id ? (
                                <Loader2 className="h-3.5 w-3.5 animate-spin" />
                              ) : (
                                <Trash2 className="h-3.5 w-3.5" />
                              )}
                            </button>
                          )}
                        </div>
                      </button>
                    ))}

                    {/* Load more */}
                    {hasMore && (
                      <button
                        onClick={handleLoadMore}
                        disabled={isLoadingMore}
                        className="w-full py-2 text-xs text-zinc-500 hover:text-zinc-300 transition-colors flex items-center justify-center gap-1.5"
                      >
                        {isLoadingMore ? (
                          <>
                            <Loader2 className="h-3 w-3 animate-spin" />
                            Loading...
                          </>
                        ) : (
                          <>
                            <ChevronDown className="h-3 w-3" />
                            Load more ({sessions.length - visibleCount})
                          </>
                        )}
                      </button>
                    )}
                  </div>
                ) : (
                  <div className="py-6 text-center pl-2">
                    {sessions === null ? (
                      <div className="space-y-2">
                        <Loader2 className="h-4 w-4 animate-spin text-zinc-600 mx-auto" />
                        <p className="text-xs text-zinc-500">Loading sessions...</p>
                      </div>
                    ) : (
                      <div className="space-y-1">
                        <p className="text-xs text-zinc-500">No sessions yet</p>
                        <p className="text-[10px] text-zinc-600">Start a new conversation</p>
                      </div>
                    )}
                  </div>
                )}

                {/* Refresh link */}
                {sessions && (
                  <div className="py-2 pl-2 text-center">
                    <button
                      onClick={onListSessions}
                      className="text-[10px] text-zinc-600 hover:text-zinc-400 transition-colors"
                    >
                      Refresh
                    </button>
                  </div>
                )}
              </ScrollArea>
            )}

            {/* Collapsed state — just show count */}
            {!sessionsOpen && <div className="flex-1" />}

            {/* Settings button */}
            <div className="px-3 pb-3">
              <button
                onClick={() => setView("settings")}
                className="w-full flex items-center gap-2.5 px-3 py-2 rounded-md text-zinc-500 text-sm hover:text-zinc-300 hover:bg-zinc-900/50 transition-colors"
              >
                <Settings className="h-4 w-4" />
                <span>Settings</span>
              </button>
            </div>
          </>
        )}
      </div>

      {/* Footer */}
      <div className="p-3 border-t border-zinc-800">
        <p className="text-[10px] text-zinc-600 text-center">lukan v1.0</p>
      </div>

      {/* Delete confirmation modal */}
      <Dialog open={!!confirmDeleteId}>
        <DialogContent onClose={() => setConfirmDeleteId(null)}>
          <DialogHeader>
            <DialogTitle>Delete Session</DialogTitle>
            <DialogDescription>
              This will permanently delete this session and all associated screenshots. This action
              cannot be undone.
            </DialogDescription>
          </DialogHeader>
          <DialogFooter>
            <Button
              variant="outline"
              size="sm"
              onClick={() => setConfirmDeleteId(null)}
              className="flex-1"
            >
              Cancel
            </Button>
            <Button
              size="sm"
              onClick={confirmDelete}
              className="flex-1 bg-red-500/20 text-red-400 border border-red-500/30 hover:bg-red-500/30"
            >
              <Trash2 className="h-3.5 w-3.5 mr-1.5" />
              Delete
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </div>
  );
}
