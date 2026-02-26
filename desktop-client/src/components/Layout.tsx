import type { ReactNode } from "react";
import type { TabId } from "../lib/types";
import logoUrl from "../assets/logo.png";
import {
  MessageSquare,
  TerminalSquare,
  Settings,
  KeyRound,
  Puzzle,
  Server,
  Brain,
} from "lucide-react";

const tabs: { id: TabId; label: string; desc: string; icon: ReactNode }[] = [
  { id: "chat", label: "Chat", desc: "AI Assistant", icon: <MessageSquare size={18} /> },
  { id: "terminal", label: "Terminal", desc: "Shell", icon: <TerminalSquare size={18} /> },
  { id: "config", label: "Config", desc: "LLM & general", icon: <Settings size={18} /> },
  { id: "credentials", label: "Credentials", desc: "API keys", icon: <KeyRound size={18} /> },
  { id: "plugins", label: "Plugins", desc: "Extensions", icon: <Puzzle size={18} /> },
  { id: "providers", label: "Providers", desc: "Models", icon: <Server size={18} /> },
  { id: "memory", label: "Memory", desc: "Context", icon: <Brain size={18} /> },
];

interface LayoutProps {
  activeTab: TabId;
  onTabChange: (tab: TabId) => void;
  children: ReactNode;
}

export default function Layout({ activeTab, onTabChange, children }: LayoutProps) {
  return (
    <div className="flex h-screen" style={{ background: "var(--bg-base)" }}>
      {/* Sidebar — monochrome matching web */}
      <nav
        className="w-56 flex-shrink-0 flex flex-col"
        style={{
          background: "#0a0a0a",
          borderRight: "1px solid rgba(60, 60, 60, 0.4)",
        }}
      >
        {/* Brand / Logo */}
        <div className="px-4 pt-5 pb-4">
          <div className="flex items-center gap-2.5">
            <div
              className="w-8 h-8 rounded-lg flex items-center justify-center"
              style={{
                background: "rgba(40, 40, 40, 0.8)",
                border: "1px solid rgba(60, 60, 60, 0.5)",
              }}
            >
              <img src={logoUrl} alt="lukan" className="h-5 w-5" />
            </div>
            <div>
              <h1
                className="text-sm font-semibold"
                style={{ color: "#fafafa" }}
              >
                lukan
              </h1>
              <p
                className="text-[10px]"
                style={{ color: "#52525b" }}
              >
                Settings
              </p>
            </div>
          </div>
        </div>

        {/* Divider */}
        <div className="mx-3" style={{ borderTop: "1px solid rgba(60, 60, 60, 0.4)" }} />

        {/* Tabs */}
        <div className="flex flex-col gap-0.5 px-3 pt-3 flex-1">
          {tabs.map((tab) => {
            const isActive = activeTab === tab.id;
            return (
              <button
                key={tab.id}
                onClick={() => onTabChange(tab.id)}
                className="relative flex items-center gap-3 px-3 py-2.5 rounded-lg text-sm cursor-pointer border-none text-left transition-all"
                style={{
                  background: isActive
                    ? "rgba(60, 60, 60, 0.3)"
                    : "transparent",
                  color: isActive ? "#fafafa" : "#a1a1aa",
                  transitionDuration: "150ms",
                }}
                onMouseEnter={(e) => {
                  if (!isActive) {
                    e.currentTarget.style.background = "rgba(50, 50, 50, 0.2)";
                    e.currentTarget.style.color = "#fafafa";
                  }
                }}
                onMouseLeave={(e) => {
                  if (!isActive) {
                    e.currentTarget.style.background = "transparent";
                    e.currentTarget.style.color = "#a1a1aa";
                  }
                }}
              >
                {/* Active indicator bar */}
                {isActive && (
                  <span
                    className="absolute left-0 top-2 bottom-2 w-[3px] rounded-full"
                    style={{
                      background: "#fafafa",
                    }}
                  />
                )}
                <span
                  className="shrink-0 transition-colors"
                  style={{
                    color: isActive ? "#fafafa" : "inherit",
                  }}
                >
                  {tab.icon}
                </span>
                <div className="min-w-0">
                  <div className="font-medium leading-tight text-[13px]">{tab.label}</div>
                  <div
                    className="text-[11px] leading-tight mt-0.5 truncate"
                    style={{ color: "#52525b" }}
                  >
                    {tab.desc}
                  </div>
                </div>
              </button>
            );
          })}
        </div>

        {/* Bottom version */}
        <div className="px-4 py-3" style={{ borderTop: "1px solid rgba(60, 60, 60, 0.4)" }}>
          <span className="text-[10px] font-mono" style={{ color: "#52525b" }}>
            v0.1.0
          </span>
        </div>
      </nav>

      {/* Content */}
      <main
        className="flex-1 flex flex-col min-h-0"
        style={{ background: "var(--bg-base)" }}
      >
        {children}
      </main>
    </div>
  );
}
