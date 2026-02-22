import {
  Coins,
  PanelLeft,
  Cpu,
  Activity,
  ChevronDown,
  Check,
  Bot,
  Timer,
  ArrowUpRight,
  ArrowDownRight,
  Database,
  Layers,
} from "lucide-react";
import React, { useState, useRef, useEffect, useCallback } from "react";
import { createPortal } from "react-dom";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Tooltip } from "@/components/ui/tooltip";

interface HeaderProps {
  providerName: string;
  modelName: string;
  tokenUsage: { input: number; output: number; cacheCreation: number; cacheRead: number };
  contextSize: number;
  isProcessing: boolean;
  availableModels: string[] | null;
  subAgentCount: number;
  runningSubAgentCount: number;
  workerCount: number;
  enabledWorkerCount: number;
  onToggleSidebar: () => void;
  onListModels: () => void;
  onSetModel: (model: string) => void;
  onOpenSubAgentViewer: () => void;
  onOpenWorkersPanel: () => void;
}

function formatTokens(n: number): string {
  if (n >= 1_000_000) return (n / 1_000_000).toFixed(1) + "M";
  if (n >= 1_000) return (n / 1_000).toFixed(1) + "k";
  return String(n);
}

export function Header({
  providerName,
  modelName,
  tokenUsage,
  contextSize,
  isProcessing,
  availableModels,
  subAgentCount,
  runningSubAgentCount,
  workerCount,
  enabledWorkerCount,
  onToggleSidebar,
  onListModels,
  onSetModel,
  onOpenSubAgentViewer,
  onOpenWorkersPanel,
}: HeaderProps) {
  const total = tokenUsage.input + tokenUsage.output;
  const [dropdownOpen, setDropdownOpen] = useState(false);
  const triggerRef = useRef<HTMLButtonElement>(null);
  const dropdownRef = useRef<HTMLDivElement>(null);
  const [dropdownPos, setDropdownPos] = useState({ top: 0, left: 0 });

  const [tokenDropdownOpen, setTokenDropdownOpen] = useState(false);
  const tokenTriggerRef = useRef<HTMLButtonElement>(null);
  const tokenDropdownRef = useRef<HTMLDivElement>(null);
  const [tokenDropdownPos, setTokenDropdownPos] = useState({ top: 0, left: 0 });

  const currentModelKey = `${providerName}:${modelName}`;

  // Position the dropdown below the trigger button
  const updatePosition = useCallback(() => {
    if (!triggerRef.current) return;
    const rect = triggerRef.current.getBoundingClientRect();
    setDropdownPos({
      top: rect.bottom + 4,
      left: rect.left,
    });
  }, []);

  const updateTokenPosition = useCallback(() => {
    if (!tokenTriggerRef.current) return;
    const rect = tokenTriggerRef.current.getBoundingClientRect();
    setTokenDropdownPos({
      top: rect.bottom + 4,
      left: rect.right - 220, // 220 = dropdown width, anchor right edge
    });
  }, []);

  // Close dropdowns on outside click
  useEffect(() => {
    if (!dropdownOpen && !tokenDropdownOpen) return;
    const handleClick = (e: MouseEvent) => {
      const target = e.target as Node;
      if (dropdownOpen) {
        if (!triggerRef.current?.contains(target) && !dropdownRef.current?.contains(target)) {
          setDropdownOpen(false);
        }
      }
      if (tokenDropdownOpen) {
        if (
          !tokenTriggerRef.current?.contains(target) &&
          !tokenDropdownRef.current?.contains(target)
        ) {
          setTokenDropdownOpen(false);
        }
      }
    };
    document.addEventListener("mousedown", handleClick);
    return () => document.removeEventListener("mousedown", handleClick);
  }, [dropdownOpen, tokenDropdownOpen]);

  // Close on Escape
  useEffect(() => {
    if (!dropdownOpen && !tokenDropdownOpen) return;
    const handleKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        setDropdownOpen(false);
        setTokenDropdownOpen(false);
      }
    };
    document.addEventListener("keydown", handleKey);
    return () => document.removeEventListener("keydown", handleKey);
  }, [dropdownOpen, tokenDropdownOpen]);

  const handleToggleDropdown = () => {
    if (!dropdownOpen) {
      if (!availableModels) onListModels();
      updatePosition();
    }
    setDropdownOpen(!dropdownOpen);
  };

  const handleSelectModel = (model: string) => {
    if (model !== currentModelKey) {
      onSetModel(model);
    }
    setDropdownOpen(false);
  };

  return (
    <header className="flex items-center justify-between border-b border-white/5 px-4 py-2.5 shrink-0 bg-background/50 backdrop-blur-sm">
      <div className="flex items-center gap-3">
        {/* Mobile menu button */}
        <Button
          variant="ghost"
          size="icon"
          className="md:hidden h-8 w-8 text-zinc-400 hover:text-zinc-200 hover:bg-white/5"
          onClick={onToggleSidebar}
        >
          <PanelLeft className="h-4 w-4" />
        </Button>

        {/* Model selector trigger */}
        <button
          ref={triggerRef}
          onClick={handleToggleDropdown}
          className="flex items-center gap-2 rounded-lg px-2.5 py-1.5 transition-colors hover:bg-white/5 group"
        >
          <div className="flex h-6 w-6 items-center justify-center rounded-md bg-purple-500/20">
            <Cpu className="h-3.5 w-3.5 text-purple-400" />
          </div>
          <span className="text-xs font-medium text-zinc-400">
            <span className="text-zinc-300">{providerName}</span>
            <span className="text-zinc-600 mx-1">/</span>
            <span className="text-purple-300">{modelName}</span>
          </span>
          <ChevronDown
            className={`h-3 w-3 text-zinc-500 group-hover:text-zinc-300 transition-transform ${
              dropdownOpen ? "rotate-180" : ""
            }`}
          />
        </button>

        {/* Dropdown — rendered via portal so it's not clipped */}
        {dropdownOpen &&
          createPortal(
            <div
              ref={dropdownRef}
              className="fixed z-[9999] min-w-[280px] rounded-lg border border-zinc-800 bg-zinc-900 shadow-xl shadow-black/50 animate-fade-in"
              style={{ top: dropdownPos.top, left: dropdownPos.left }}
            >
              <div className="px-3 py-2 border-b border-zinc-800">
                <span className="text-[10px] font-semibold uppercase tracking-wider text-zinc-500">
                  Select Model
                </span>
              </div>
              <div className="max-h-[calc(100vh-120px)] overflow-y-auto py-1 overscroll-contain">
                {availableModels ? (
                  availableModels.map((model) => {
                    const isCurrent = model === currentModelKey;
                    const colonIdx = model.indexOf(":");
                    const mProvider = model.slice(0, colonIdx);
                    const mModel = model.slice(colonIdx + 1);
                    return (
                      <button
                        key={model}
                        onClick={() => handleSelectModel(model)}
                        className={`w-full flex items-center gap-2.5 px-3 py-2 text-left text-sm transition-colors ${
                          isCurrent
                            ? "bg-purple-500/10 text-zinc-100"
                            : "text-zinc-400 hover:bg-white/5 hover:text-zinc-200"
                        }`}
                      >
                        <div className="flex-1 min-w-0 truncate">
                          <span className="text-zinc-500 text-xs">{mProvider}/</span>
                          <span
                            className={`text-xs font-medium ${isCurrent ? "text-purple-300" : "text-zinc-300"}`}
                          >
                            {mModel}
                          </span>
                        </div>
                        {isCurrent && <Check className="h-3.5 w-3.5 text-purple-400 shrink-0" />}
                      </button>
                    );
                  })
                ) : (
                  <div className="px-3 py-4 text-center text-xs text-zinc-500">
                    Loading models...
                  </div>
                )}
              </div>
            </div>,
            document.body,
          )}

        {/* Processing indicator */}
        {isProcessing && (
          <Badge className="text-[10px] bg-purple-500/20 text-purple-300 border-purple-500/30 gap-1.5 animate-pulse-glow">
            <Activity className="h-2.5 w-2.5" />
            thinking...
          </Badge>
        )}
      </div>

      <div className="flex items-center gap-2 shrink-0">
        {/* Workers indicator */}
        <Tooltip
          side="bottom"
          content={
            workerCount > 0
              ? `${workerCount} worker${workerCount > 1 ? "s" : ""} (${enabledWorkerCount} enabled)`
              : "Workers"
          }
        >
          <button
            onClick={onOpenWorkersPanel}
            className={`relative flex items-center gap-1.5 text-xs font-mono px-2.5 py-1 rounded-lg border transition-colors shrink-0 ${
              workerCount > 0
                ? "bg-amber-500/10 border-amber-500/20 hover:bg-amber-500/20"
                : "bg-white/5 border-white/5 hover:bg-white/10 hover:border-white/10"
            }`}
          >
            <Timer className={`h-3 w-3 ${workerCount > 0 ? "text-amber-400" : "text-zinc-500"}`} />
            {workerCount > 0 && <span className="text-amber-300">{workerCount}</span>}
            {enabledWorkerCount > 0 && (
              <span className="absolute -top-1 -right-1 flex h-2.5 w-2.5">
                <span className="animate-ping absolute inline-flex h-full w-full rounded-full bg-amber-400 opacity-75" />
                <span className="relative inline-flex rounded-full h-2.5 w-2.5 bg-amber-500" />
              </span>
            )}
          </button>
        </Tooltip>

        {/* Sub-agent indicator */}
        {subAgentCount > 0 && (
          <Tooltip
            side="bottom"
            content={`${subAgentCount} sub-agent${subAgentCount > 1 ? "s" : ""} (${runningSubAgentCount} running)`}
          >
            <button
              onClick={onOpenSubAgentViewer}
              className="relative flex items-center gap-1.5 text-xs font-mono bg-purple-500/10 px-2.5 py-1 rounded-lg border border-purple-500/20 hover:bg-purple-500/20 transition-colors shrink-0"
            >
              <Bot className="h-3 w-3 text-purple-400" />
              <span className="text-purple-300">{subAgentCount}</span>
              {runningSubAgentCount > 0 && (
                <span className="absolute -top-1 -right-1 flex h-2.5 w-2.5">
                  <span className="animate-ping absolute inline-flex h-full w-full rounded-full bg-purple-400 opacity-75" />
                  <span className="relative inline-flex rounded-full h-2.5 w-2.5 bg-purple-500" />
                </span>
              )}
            </button>
          </Tooltip>
        )}

        {/* Token usage */}
        {total > 0 && (
          <>
            <button
              ref={tokenTriggerRef}
              onClick={() => {
                if (!tokenDropdownOpen) updateTokenPosition();
                setTokenDropdownOpen(!tokenDropdownOpen);
              }}
              className="flex items-center gap-1.5 text-xs text-zinc-500 font-mono bg-white/5 px-2.5 py-1 rounded-lg border border-white/5 shrink-0 hover:bg-white/10 hover:border-white/10 transition-colors"
            >
              <Coins className="h-3 w-3 text-zinc-400" />
              <span className="text-zinc-300">{formatTokens(total)}</span>
              <span className="text-zinc-600">tokens</span>
              <ChevronDown
                className={`h-2.5 w-2.5 text-zinc-600 transition-transform ${tokenDropdownOpen ? "rotate-180" : ""}`}
              />
            </button>
            {tokenDropdownOpen &&
              createPortal(
                <div
                  ref={tokenDropdownRef}
                  className="fixed z-[9999] w-[220px] rounded-lg border border-zinc-800 bg-zinc-900 shadow-xl shadow-black/50 animate-tooltip-in"
                  style={{ top: tokenDropdownPos.top, left: tokenDropdownPos.left }}
                >
                  <div className="px-3 py-2 border-b border-zinc-800">
                    <span className="text-[10px] font-semibold uppercase tracking-wider text-zinc-500">
                      Token Usage
                    </span>
                  </div>
                  <div className="px-3 py-2 space-y-2">
                    <div className="flex items-center justify-between">
                      <div className="flex items-center gap-1.5">
                        <ArrowUpRight className="h-3 w-3 text-blue-400" />
                        <span className="text-xs text-zinc-400">Input</span>
                      </div>
                      <span className="text-xs font-mono text-zinc-200">
                        {formatTokens(tokenUsage.input)}
                      </span>
                    </div>
                    <div className="flex items-center justify-between">
                      <div className="flex items-center gap-1.5">
                        <ArrowDownRight className="h-3 w-3 text-green-400" />
                        <span className="text-xs text-zinc-400">Output</span>
                      </div>
                      <span className="text-xs font-mono text-zinc-200">
                        {formatTokens(tokenUsage.output)}
                      </span>
                    </div>
                    <div className="flex items-center justify-between">
                      <div className="flex items-center gap-1.5">
                        <Database className="h-3 w-3 text-amber-400" />
                        <span className="text-xs text-zinc-400">Cache read</span>
                      </div>
                      <span
                        className={`text-xs font-mono ${tokenUsage.cacheRead > 0 ? "text-zinc-200" : "text-zinc-600"}`}
                      >
                        {formatTokens(tokenUsage.cacheRead)}
                      </span>
                    </div>
                    <div className="flex items-center justify-between">
                      <div className="flex items-center gap-1.5">
                        <Database className="h-3 w-3 text-orange-400" />
                        <span className="text-xs text-zinc-400">Cache write</span>
                      </div>
                      <span
                        className={`text-xs font-mono ${tokenUsage.cacheCreation > 0 ? "text-zinc-200" : "text-zinc-600"}`}
                      >
                        {formatTokens(tokenUsage.cacheCreation)}
                      </span>
                    </div>
                    <div className="flex items-center justify-between">
                      <div className="flex items-center gap-1.5">
                        <Layers className="h-3 w-3 text-purple-400" />
                        <span className="text-xs text-zinc-400">Context</span>
                      </div>
                      <span
                        className={`text-xs font-mono ${contextSize > 0 ? "text-purple-300" : "text-zinc-600"}`}
                      >
                        {contextSize > 0 ? formatTokens(contextSize) : "—"}
                      </span>
                    </div>
                    <div className="border-t border-zinc-800 pt-2 flex items-center justify-between">
                      <span className="text-xs text-zinc-300 font-medium">Total</span>
                      <span className="text-xs font-mono text-zinc-100 font-medium">
                        {formatTokens(total)}
                      </span>
                    </div>
                  </div>
                </div>,
                document.body,
              )}
          </>
        )}
      </div>
    </header>
  );
}
