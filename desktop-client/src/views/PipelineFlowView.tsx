import { useState, useEffect, useCallback, useMemo } from "react";
import {
  ReactFlow,
  Background,
  type Node,
  type Edge,
  type EdgeProps,
  type NodeTypes,
  type EdgeTypes,
  type OnNodesChange,
  type OnConnect,
  type Connection,
  Position,
  Handle,
  BackgroundVariant,
  applyNodeChanges,
  getBezierPath,
  BaseEdge,
} from "@xyflow/react";
import "@xyflow/react/dist/style.css";
import {
  ArrowLeft,
  Play,
  Loader2,
  CheckCircle2,
  XCircle,
  AlertTriangle,
  Clock,
  Coins,
  MoreVertical,
  X,
  Terminal,
  Pencil,
  Plus,
  Trash2,
  Square,
  Settings,
  Download,
  Upload,
} from "lucide-react";
import type { PipelineDetail, PipelineRun, PipelineStep, PipelineTrigger, PipelineCreateInput, StepRun, StepConnection } from "../lib/types";
import {
  getPipelineDetail,
  getPipelineRun,
  triggerPipeline,
  cancelPipeline,
  updatePipeline,
  onPipelineNotification,
  listTools,
  listProviders,
  getModels,
} from "../lib/tauri";

// ── Helpers ──

function formatTokens(n: number): string {
  if (n >= 1_000_000) return (n / 1_000_000).toFixed(1) + "M";
  if (n >= 1_000) return (n / 1_000).toFixed(1) + "k";
  return String(n);
}

function formatElapsed(startedAt: string, completedAt?: string): string {
  const start = new Date(startedAt).getTime();
  const end = completedAt ? new Date(completedAt).getTime() : Date.now();
  const secs = Math.floor((end - start) / 1000);
  if (secs < 60) return `${secs}s`;
  const mins = Math.floor(secs / 60);
  if (mins < 60) return `${mins}m${secs % 60}s`;
  return `${Math.floor(mins / 60)}h${mins % 60}m`;
}

// ── Step Node ──

interface StepNodeData {
  label: string;
  prompt: string;
  stepId: string;
  model?: string;
  provider?: string;
  stepRun?: StepRun;
  stepIndex: number;
  totalSteps: number;
  onViewOutput?: (stepRun: StepRun) => void;
  onEditStep?: (stepId: string) => void;
  onDeleteStep?: (stepId: string) => void;
  [key: string]: unknown;
}

const STATUS_COLOR: Record<string, string> = {
  running: "#f59e0b",
  success: "#22c55e",
  error: "#ef4444",
  partial: "#f59e0b",
  skipped: "#52525b",
};

const menuItemStyle: React.CSSProperties = {
  display: "flex", alignItems: "center", gap: 6, width: "100%",
  border: "none", background: "transparent", color: "#ccc",
  cursor: "pointer", padding: "5px 8px", borderRadius: 3,
  fontSize: 10, fontFamily: "monospace", whiteSpace: "nowrap",
};

function StepNode({ data }: { data: StepNodeData }) {
  const { label, stepRun, stepId, stepIndex, totalSteps, model, provider, onViewOutput, onEditStep, onDeleteStep } = data;
  const [menuOpen, setMenuOpen] = useState(false);
  const status = stepRun?.status;
  const c = (status && STATUS_COLOR[status]) || "#3c3c3c";
  const isRunning = status === "running";
  const tokens = stepRun ? stepRun.tokenUsage.input + stepRun.tokenUsage.output : 0;
  const hasOutput = stepRun && (stepRun.output || stepRun.error) && status !== "pending";

  useEffect(() => {
    if (!menuOpen) return;
    const close = () => setMenuOpen(false);
    document.addEventListener("pointerdown", close);
    return () => document.removeEventListener("pointerdown", close);
  }, [menuOpen]);

  return (
    <div
      style={{
        background: "#111",
        border: `1px solid ${c}`,
        borderLeft: `3px solid ${c}`,
        borderRadius: 4,
        width: 200,
        fontFamily: "monospace",
        boxShadow: isRunning ? `0 0 12px ${c}30` : "none",
        transition: "border-color 0.3s, box-shadow 0.3s",
      }}
    >
      <Handle type="target" position={Position.Top} style={{ background: c, border: "2px solid #0a0a0a", width: 8, height: 8 }} />

      {/* Header */}
      <div style={{ display: "flex", alignItems: "center", gap: 6, padding: "5px 8px", borderBottom: `1px solid ${c}20` }}>
        <span style={{ fontSize: 9, color: c, fontWeight: 700, opacity: 0.7 }}>
          {String(stepIndex + 1).padStart(2, "0")}
        </span>
        <span style={{ fontSize: 11, fontWeight: 600, color: "#e0e0e0", flex: 1, overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>
          {label}
        </span>
        {status === "running" && <Loader2 size={11} style={{ color: c, animation: "spin 1s linear infinite" }} />}
        {status === "success" && <CheckCircle2 size={11} style={{ color: c }} />}
        {status === "error" && <XCircle size={11} style={{ color: c }} />}
        {status === "partial" && <AlertTriangle size={11} style={{ color: c }} />}

        {/* 3-dot menu */}
        <div
          className="nopan nodrag nowheel"
          onPointerDown={(e) => e.stopPropagation()}
          style={{ display: "flex", position: "relative" }}
        >
          <button
            onClick={() => setMenuOpen(!menuOpen)}
            style={{ border: "none", background: "transparent", color: "#555", cursor: "pointer", padding: 2, display: "flex", alignItems: "center" }}
          >
            <MoreVertical size={11} />
          </button>

          {menuOpen && (
            <div
              onPointerDown={(e) => e.stopPropagation()}
              style={{
                position: "absolute",
                top: -4,
                left: "calc(100% + 4px)",
                background: "#1a1a1a",
                border: "1px solid #333",
                borderRadius: 4,
                padding: 2,
                zIndex: 50,
                boxShadow: "0 4px 12px rgba(0,0,0,0.5)",
              }}
            >
              <button
                onClick={() => { setMenuOpen(false); onEditStep?.(stepId); }}
                style={menuItemStyle}
                onMouseEnter={(e) => { e.currentTarget.style.background = "#2a2a2a"; }}
                onMouseLeave={(e) => { e.currentTarget.style.background = "transparent"; }}
              >
                <Pencil size={10} /> Edit step
              </button>
              {hasOutput && (
                <button
                  onClick={() => { setMenuOpen(false); if (stepRun) onViewOutput?.(stepRun); }}
                  style={menuItemStyle}
                  onMouseEnter={(e) => { e.currentTarget.style.background = "#2a2a2a"; }}
                  onMouseLeave={(e) => { e.currentTarget.style.background = "transparent"; }}
                >
                  <Terminal size={10} /> View output
                </button>
              )}
              {totalSteps > 1 && (
                <button
                  onClick={() => { setMenuOpen(false); onDeleteStep?.(stepId); }}
                  style={{ ...menuItemStyle, color: "#ef4444" }}
                  onMouseEnter={(e) => { e.currentTarget.style.background = "#2a2a2a"; }}
                  onMouseLeave={(e) => { e.currentTarget.style.background = "transparent"; }}
                >
                  <Trash2 size={10} /> Delete step
                </button>
              )}
            </div>
          )}
        </div>
      </div>

      {/* Model badge */}
      {(model || provider) && (
        <div style={{ padding: "2px 8px", fontSize: 8, color: "#555", overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>
          {model || provider}
        </div>
      )}

      {/* Stats */}
      {stepRun && status !== "pending" && (
        <div style={{ display: "flex", gap: 8, padding: "3px 8px 4px", fontSize: 9, color: "#666" }}>
          {isRunning && (
            <span style={{ color: c, display: "flex", alignItems: "center", gap: 2, overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap", maxWidth: 180 }}>
              <Loader2 size={8} style={{ animation: "spin 1s linear infinite", flexShrink: 0 }} />
              {stepRun.error?.startsWith("[activity]") ? stepRun.error.slice(11) : "processing..."}
            </span>
          )}
          {stepRun.completedAt && stepRun.startedAt && (
            <span style={{ display: "flex", alignItems: "center", gap: 2 }}>
              <Clock size={8} /> {formatElapsed(stepRun.startedAt, stepRun.completedAt)}
            </span>
          )}
          {tokens > 0 && (
            <span style={{ display: "flex", alignItems: "center", gap: 2 }}>
              <Coins size={8} /> {formatTokens(tokens)}
            </span>
          )}
          {stepRun.turns > 0 && <span>{stepRun.turns}t</span>}
        </div>
      )}

      {stepRun?.error && !stepRun.error.startsWith("[activity]") && status !== "running" && (
        <div style={{ fontSize: 9, color: "#ef4444", padding: "2px 8px 4px", overflow: "hidden", whiteSpace: "nowrap", textOverflow: "ellipsis" }}>
          {stepRun.error.slice(0, 50)}
        </div>
      )}

      <Handle type="source" position={Position.Bottom} style={{ background: c, border: "2px solid #0a0a0a", width: 8, height: 8 }} />
    </div>
  );
}

const nodeTypes: NodeTypes = { step: StepNode };

// ── Deletable Edge ──

function DeletableEdge({
  id, sourceX, sourceY, targetX, targetY, sourcePosition, targetPosition, style, data,
}: EdgeProps) {
  const [edgePath, labelX, labelY] = getBezierPath({ sourceX, sourceY, targetX, targetY, sourcePosition, targetPosition });
  const [hovered, setHovered] = useState(false);
  const onDelete = (data as Record<string, unknown>)?.onDelete as ((id: string) => void) | undefined;

  return (
    <g
      onMouseEnter={() => setHovered(true)}
      onMouseLeave={() => setHovered(false)}
    >
      {/* Invisible fat path for easier hover */}
      <path d={edgePath} fill="none" stroke="transparent" strokeWidth={14} />
      <BaseEdge id={id} path={edgePath} style={style} />
      {hovered && onDelete && (
        <foreignObject x={labelX - 8} y={labelY - 8} width={16} height={16} style={{ overflow: "visible" }}>
          <button
            onClick={(e) => { e.stopPropagation(); onDelete(id); }}
            style={{
              width: 16, height: 16, borderRadius: "50%", border: "1px solid #555",
              background: "#1a1a1a", color: "#ef4444", cursor: "pointer",
              display: "flex", alignItems: "center", justifyContent: "center",
              fontSize: 10, fontWeight: 700, padding: 0, lineHeight: 1,
            }}
          >
            ×
          </button>
        </foreignObject>
      )}
    </g>
  );
}

const edgeTypes: EdgeTypes = { deletable: DeletableEdge };

// ── Edit Step Sheet ──

function EditStepSheet({
  step,
  onSave,
  onClose,
}: {
  step: PipelineStep;
  onSave: (updated: PipelineStep) => void;
  onClose: () => void;
}) {
  const [name, setName] = useState(step.name);
  const [prompt, setPrompt] = useState(step.prompt);
  const [saving, setSaving] = useState(false);
  const [availableTools, setAvailableTools] = useState<{ name: string; source: string | null }[]>([]);
  const [selectedTools, setSelectedTools] = useState<Set<string>>(new Set(step.tools ?? []));
  const [toolsEnabled, setToolsEnabled] = useState(!!step.tools);
  const [toolFilter, setToolFilter] = useState("");
  const [providers, setProviders] = useState<{ name: string; currentModel?: string; defaultModel: string }[]>([]);
  const [allModels, setAllModels] = useState<string[]>([]);
  const [selectedProvider, setSelectedProvider] = useState(step.provider ?? "");
  const [selectedModel, setSelectedModel] = useState(step.model ?? "");

  useEffect(() => {
    listTools().then(setAvailableTools).catch(() => {});
    Promise.all([listProviders(), getModels()]).then(([p, m]) => {
      setProviders(p);
      setAllModels(m);
    }).catch(() => {});
  }, []);

  // Models for the selected provider (format: "provider:model_id")
  const providerModels = selectedProvider
    ? allModels.filter((m) => m.startsWith(`${selectedProvider}:`)).map((m) => m.substring(selectedProvider.length + 1))
    : [];

  const filteredTools = toolFilter
    ? availableTools.filter((t) => t.name.toLowerCase().includes(toolFilter.toLowerCase()))
    : availableTools;

  const toggleTool = (toolName: string) => {
    setSelectedTools((prev) => {
      const next = new Set(prev);
      if (next.has(toolName)) next.delete(toolName);
      else next.add(toolName);
      return next;
    });
  };

  const inputStyle: React.CSSProperties = {
    width: "100%", border: "1px solid #333", background: "#111",
    color: "#e0e0e0", borderRadius: 4, padding: "6px 8px",
    fontSize: 12, fontFamily: "monospace", outline: "none", boxSizing: "border-box",
  };

  const handleSave = () => {
    setSaving(true);
    onSave({
      ...step,
      name: name.trim() || step.name,
      prompt: prompt.trim() || step.prompt,
      tools: toolsEnabled ? Array.from(selectedTools) : undefined,
      provider: selectedProvider || undefined,
      model: selectedModel || undefined,
    });
  };

  return (
    <div
      style={{
        position: "absolute", top: 0, right: 0, bottom: 0, width: 340,
        background: "#0e0e0e", borderLeft: "1px solid #222",
        display: "flex", flexDirection: "column", zIndex: 100,
        fontFamily: "monospace",
      }}
    >
      {/* Header */}
      <div style={{ display: "flex", alignItems: "center", gap: 8, padding: "8px 12px", borderBottom: "1px solid #1e1e1e", flexShrink: 0 }}>
        <Pencil size={12} style={{ color: "#888" }} />
        <span style={{ fontSize: 12, fontWeight: 600, color: "#e0e0e0", flex: 1 }}>Edit Step</span>
        <button onClick={onClose} style={{ border: "none", background: "transparent", color: "#555", cursor: "pointer", padding: 2, display: "flex" }}>
          <X size={14} />
        </button>
      </div>

      {/* Form */}
      <div style={{ flex: 1, overflow: "auto", padding: "12px" }}>
        <div style={{ marginBottom: 12 }}>
          <label style={{ display: "block", fontSize: 10, color: "#666", marginBottom: 4 }}>Name</label>
          <input type="text" value={name} onChange={(e) => setName(e.target.value)} style={inputStyle} />
        </div>

        <div style={{ marginBottom: 12 }}>
          <label style={{ display: "block", fontSize: 10, color: "#666", marginBottom: 4 }}>Prompt</label>
          <textarea
            value={prompt}
            onChange={(e) => setPrompt(e.target.value)}
            rows={8}
            style={{ ...inputStyle, resize: "vertical", minHeight: 100 }}
          />
          <div style={{ fontSize: 9, color: "#444", marginTop: 3 }}>
            {"Use {{input}} to reference the previous step's output"}
          </div>
        </div>

        {/* Provider / Model */}
        <div style={{ marginBottom: 12 }}>
          <label style={{ display: "block", fontSize: 10, color: "#666", marginBottom: 4 }}>Provider</label>
          <select
            value={selectedProvider}
            onChange={(e) => { setSelectedProvider(e.target.value); setSelectedModel(""); }}
            style={{ ...inputStyle, cursor: "pointer" }}
          >
            <option value="">Default (global config)</option>
            {providers.map((p) => (
              <option key={p.name} value={p.name}>{p.name}{p.currentModel ? ` (${p.currentModel})` : ""}</option>
            ))}
          </select>
        </div>

        {selectedProvider && (
          <div style={{ marginBottom: 12 }}>
            <label style={{ display: "block", fontSize: 10, color: "#666", marginBottom: 4 }}>Model</label>
            <select
              value={selectedModel}
              onChange={(e) => setSelectedModel(e.target.value)}
              style={{ ...inputStyle, cursor: "pointer" }}
            >
              <option value="">Default</option>
              {providerModels.map((m) => (
                <option key={m} value={m}>{m}</option>
              ))}
            </select>
            {providerModels.length === 0 && (
              <div style={{ fontSize: 9, color: "#444", marginTop: 3 }}>
                No models configured for {selectedProvider}
              </div>
            )}
          </div>
        )}

        {/* Tools */}
        <div style={{ marginBottom: 12 }}>
          <div style={{ display: "flex", alignItems: "center", justifyContent: "space-between", marginBottom: 6 }}>
            <label style={{ fontSize: 10, color: "#666" }}>Tools</label>
            <label style={{ display: "flex", alignItems: "center", gap: 4, fontSize: 10, color: "#888", cursor: "pointer" }}>
              <input
                type="checkbox"
                checked={toolsEnabled}
                onChange={(e) => setToolsEnabled(e.target.checked)}
                style={{ accentColor: "#a78bfa" }}
              />
              Restrict tools
            </label>
          </div>

          {toolsEnabled && (
            <>
              <input
                type="text"
                value={toolFilter}
                onChange={(e) => setToolFilter(e.target.value)}
                placeholder="Filter tools..."
                style={{ ...inputStyle, marginBottom: 6, fontSize: 10 }}
              />
              <div style={{ fontSize: 9, color: "#555", marginBottom: 4 }}>
                {selectedTools.size}/{availableTools.length} selected
                {selectedTools.size > 0 && (
                  <button
                    onClick={() => setSelectedTools(new Set())}
                    style={{ border: "none", background: "transparent", color: "#ef4444", cursor: "pointer", fontSize: 9, marginLeft: 6 }}
                  >
                    clear all
                  </button>
                )}
                <button
                  onClick={() => setSelectedTools(new Set(availableTools.map((t) => t.name)))}
                  style={{ border: "none", background: "transparent", color: "#a78bfa", cursor: "pointer", fontSize: 9, marginLeft: 6 }}
                >
                  select all
                </button>
              </div>
              <div style={{ maxHeight: 180, overflowY: "auto", border: "1px solid #222", borderRadius: 4, padding: 4 }}>
                {filteredTools.map((tool) => (
                  <label
                    key={tool.name}
                    style={{
                      display: "flex", alignItems: "center", gap: 6,
                      padding: "2px 4px", cursor: "pointer", borderRadius: 2,
                      fontSize: 10, color: selectedTools.has(tool.name) ? "#e0e0e0" : "#666",
                    }}
                    onMouseEnter={(e) => { e.currentTarget.style.background = "#1a1a1a"; }}
                    onMouseLeave={(e) => { e.currentTarget.style.background = "transparent"; }}
                  >
                    <input
                      type="checkbox"
                      checked={selectedTools.has(tool.name)}
                      onChange={() => toggleTool(tool.name)}
                      style={{ accentColor: "#a78bfa", flexShrink: 0 }}
                    />
                    <span style={{ flex: 1, overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>
                      {tool.name}
                    </span>
                    {tool.source && (
                      <span style={{ fontSize: 8, color: "#444", flexShrink: 0 }}>{tool.source}</span>
                    )}
                  </label>
                ))}
                {filteredTools.length === 0 && (
                  <div style={{ fontSize: 10, color: "#444", padding: "4px 8px", textAlign: "center" }}>
                    {toolFilter ? "No matching tools" : "No tools available"}
                  </div>
                )}
              </div>
            </>
          )}
          {!toolsEnabled && (
            <div style={{ fontSize: 9, color: "#444" }}>All tools available (no restrictions)</div>
          )}
        </div>
      </div>

      {/* Footer */}
      <div style={{ display: "flex", justifyContent: "flex-end", gap: 6, padding: "8px 12px", borderTop: "1px solid #1e1e1e", flexShrink: 0 }}>
        <button
          onClick={onClose}
          style={{ border: "1px solid #333", background: "transparent", color: "#888", cursor: "pointer", padding: "4px 12px", borderRadius: 3, fontSize: 11 }}
        >
          Cancel
        </button>
        <button
          onClick={handleSave}
          disabled={saving}
          style={{
            border: "1px solid #333", background: "#fafafa", color: "#0a0a0a",
            cursor: saving ? "not-allowed" : "pointer", padding: "4px 12px",
            borderRadius: 3, fontSize: 11, fontWeight: 600, opacity: saving ? 0.5 : 1,
          }}
        >
          {saving ? "Saving..." : "Save"}
        </button>
      </div>
    </div>
  );
}

// ── Pipeline Settings Sheet ──

function PipelineSettingsSheet({
  detail,
  onSave,
  onClose,
}: {
  detail: PipelineDetail;
  onSave: (patch: { name?: string; description?: string; trigger?: PipelineTrigger }) => void;
  onClose: () => void;
}) {
  const [name, setName] = useState(detail.name);
  const [description, setDescription] = useState(detail.description ?? "");
  const [triggerType, setTriggerType] = useState<PipelineTrigger["type"]>(detail.trigger.type);
  const [schedule, setSchedule] = useState(detail.trigger.schedule ?? "every:5m");
  const [webhookSecret, setWebhookSecret] = useState(detail.trigger.secret ?? "");
  const [eventSource, setEventSource] = useState(detail.trigger.source ?? "");
  const [eventLevel, setEventLevel] = useState(detail.trigger.level ?? "");
  const [watchPath, setWatchPath] = useState(detail.trigger.path ?? "");
  const [debounceSecs, setDebounceSecs] = useState(String(detail.trigger.debounceSecs ?? 5));
  const [saving, setSaving] = useState(false);

  const inputStyle: React.CSSProperties = {
    width: "100%", border: "1px solid #333", background: "#111",
    color: "#e0e0e0", borderRadius: 4, padding: "6px 8px",
    fontSize: 12, fontFamily: "monospace", outline: "none", boxSizing: "border-box",
  };

  const handleSave = () => {
    setSaving(true);
    const trigger: PipelineTrigger =
      triggerType === "schedule"
        ? { type: "schedule", schedule }
        : triggerType === "webhook"
          ? { type: "webhook", secret: webhookSecret.trim() || undefined }
          : triggerType === "event"
            ? { type: "event", source: eventSource.trim(), level: eventLevel.trim() || undefined }
            : triggerType === "fileWatch"
              ? { type: "fileWatch", path: watchPath.trim(), debounceSecs: parseInt(debounceSecs) || 5 }
              : { type: "manual" };

    onSave({
      name: name.trim() || detail.name,
      description: description.trim() || undefined,
      trigger,
    });
  };

  return (
    <div
      style={{
        position: "absolute", top: 0, right: 0, bottom: 0, width: 340,
        background: "#0e0e0e", borderLeft: "1px solid #222",
        display: "flex", flexDirection: "column", zIndex: 100,
        fontFamily: "monospace",
      }}
    >
      <div style={{ display: "flex", alignItems: "center", gap: 8, padding: "8px 12px", borderBottom: "1px solid #1e1e1e", flexShrink: 0 }}>
        <Settings size={12} style={{ color: "#888" }} />
        <span style={{ fontSize: 12, fontWeight: 600, color: "#e0e0e0", flex: 1 }}>Pipeline Settings</span>
        <button onClick={onClose} style={{ border: "none", background: "transparent", color: "#555", cursor: "pointer", padding: 2, display: "flex" }}>
          <X size={14} />
        </button>
      </div>

      <div style={{ flex: 1, overflow: "auto", padding: "12px" }}>
        <div style={{ marginBottom: 12 }}>
          <label style={{ display: "block", fontSize: 10, color: "#666", marginBottom: 4 }}>Name</label>
          <input type="text" value={name} onChange={(e) => setName(e.target.value)} style={inputStyle} />
        </div>

        <div style={{ marginBottom: 12 }}>
          <label style={{ display: "block", fontSize: 10, color: "#666", marginBottom: 4 }}>Description</label>
          <input type="text" value={description} onChange={(e) => setDescription(e.target.value)} placeholder="Optional" style={inputStyle} />
        </div>

        <div style={{ marginBottom: 12 }}>
          <label style={{ display: "block", fontSize: 10, color: "#666", marginBottom: 6 }}>Trigger</label>
          <div style={{ display: "flex", gap: 6, marginBottom: 8, flexWrap: "wrap" }}>
            {(["manual", "schedule", "webhook", "event", "fileWatch"] as const).map((t) => (
              <button
                key={t}
                type="button"
                onClick={() => setTriggerType(t)}
                style={{
                  border: `1px solid ${triggerType === t ? "#a78bfa" : "#333"}`,
                  background: triggerType === t ? "#a78bfa20" : "transparent",
                  color: triggerType === t ? "#a78bfa" : "#888",
                  cursor: "pointer", padding: "2px 8px", borderRadius: 3, fontSize: 10,
                }}
              >
                {t === "fileWatch" ? "File Watch" : t.charAt(0).toUpperCase() + t.slice(1)}
              </button>
            ))}
          </div>

          {triggerType === "schedule" && (
            <input type="text" value={schedule} onChange={(e) => setSchedule(e.target.value)} placeholder="every:5m" style={inputStyle} />
          )}
          {triggerType === "webhook" && (
            <>
              <input type="text" value={webhookSecret} onChange={(e) => setWebhookSecret(e.target.value)} placeholder="Secret (optional)" style={inputStyle} />
              <div style={{ fontSize: 9, color: "#444", marginTop: 3 }}>POST /api/pipelines/ID/webhook?secret=TOKEN</div>
            </>
          )}
          {triggerType === "event" && (
            <>
              <input type="text" value={eventSource} onChange={(e) => setEventSource(e.target.value)} placeholder="Event source" style={{ ...inputStyle, marginBottom: 4 }} />
              <input type="text" value={eventLevel} onChange={(e) => setEventLevel(e.target.value)} placeholder="Level (optional)" style={inputStyle} />
            </>
          )}
          {triggerType === "fileWatch" && (
            <>
              <input type="text" value={watchPath} onChange={(e) => setWatchPath(e.target.value)} placeholder="/path/to/watch" style={{ ...inputStyle, marginBottom: 4 }} />
              <input type="text" value={debounceSecs} onChange={(e) => setDebounceSecs(e.target.value)} placeholder="Debounce secs" style={inputStyle} />
            </>
          )}
        </div>
      </div>

      <div style={{ display: "flex", justifyContent: "flex-end", gap: 6, padding: "8px 12px", borderTop: "1px solid #1e1e1e", flexShrink: 0 }}>
        <button onClick={onClose} style={{ border: "1px solid #333", background: "transparent", color: "#888", cursor: "pointer", padding: "4px 12px", borderRadius: 3, fontSize: 11 }}>
          Cancel
        </button>
        <button
          onClick={handleSave}
          disabled={saving}
          style={{
            border: "1px solid #333", background: "#fafafa", color: "#0a0a0a",
            cursor: saving ? "not-allowed" : "pointer", padding: "4px 12px",
            borderRadius: 3, fontSize: 11, fontWeight: 600, opacity: saving ? 0.5 : 1,
          }}
        >
          {saving ? "Saving..." : "Save"}
        </button>
      </div>
    </div>
  );
}

// ── Output Panel ──

function OutputPanel({ stepRun, onClose }: { stepRun: StepRun; onClose: () => void }) {
  const c = STATUS_COLOR[stepRun.status] || "#3c3c3c";
  const tokens = stepRun.tokenUsage.input + stepRun.tokenUsage.output;

  return (
    <div
      style={{
        borderTop: "1px solid #222", background: "#0e0e0e", flexShrink: 0,
        maxHeight: "40%", display: "flex", flexDirection: "column", fontFamily: "monospace",
      }}
    >
      <div style={{ display: "flex", alignItems: "center", gap: 8, padding: "4px 10px", borderBottom: "1px solid #1e1e1e", flexShrink: 0 }}>
        <span style={{ width: 6, height: 6, borderRadius: "50%", background: c, flexShrink: 0 }} />
        <span style={{ fontSize: 11, fontWeight: 600, color: "#e0e0e0", flex: 1 }}>{stepRun.stepName}</span>
        <span style={{ fontSize: 9, color: "#555" }}>{stepRun.status}</span>
        {stepRun.completedAt && stepRun.startedAt && (
          <span style={{ fontSize: 9, color: "#555", display: "flex", alignItems: "center", gap: 2 }}>
            <Clock size={8} /> {formatElapsed(stepRun.startedAt, stepRun.completedAt)}
          </span>
        )}
        {tokens > 0 && (
          <span style={{ fontSize: 9, color: "#555", display: "flex", alignItems: "center", gap: 2 }}>
            <Coins size={8} /> {formatTokens(tokens)}
          </span>
        )}
        <button onClick={onClose} style={{ border: "none", background: "transparent", color: "#555", cursor: "pointer", padding: 2, display: "flex" }}>
          <X size={12} />
        </button>
      </div>
      <div style={{ flex: 1, overflow: "auto", padding: "8px 12px" }}>
        {stepRun.error && (
          <div style={{ fontSize: 11, color: "#ef4444", marginBottom: 8, padding: "4px 8px", background: "rgba(239,68,68,0.06)", borderRadius: 3 }}>
            {stepRun.error}
          </div>
        )}
        <pre style={{ fontSize: 11, color: "#b0b0b0", whiteSpace: "pre-wrap", wordBreak: "break-word", margin: 0, lineHeight: 1.5 }}>
          {stepRun.output || "(no output)"}
        </pre>
      </div>
    </div>
  );
}

// ── Layout Helpers ──

interface LevelInfo {
  level: number;
  indexInLevel: number;
  levelSize: number;
}

function computeTopologicalLevels(
  steps: PipelineStep[],
  connections: StepConnection[],
): Map<string, LevelInfo> {
  const result = new Map<string, LevelInfo>();
  const stepIds = new Set(steps.map((s) => s.id));
  const inDegree = new Map<string, number>();
  const adjacency = new Map<string, string[]>();

  for (const s of steps) {
    inDegree.set(s.id, 0);
    adjacency.set(s.id, []);
  }

  for (const c of connections) {
    if (c.fromStep === "__trigger__" || !stepIds.has(c.fromStep) || !stepIds.has(c.toStep)) continue;
    inDegree.set(c.toStep, (inDegree.get(c.toStep) ?? 0) + 1);
    adjacency.get(c.fromStep)?.push(c.toStep);
  }

  let currentLevel = steps.filter((s) => (inDegree.get(s.id) ?? 0) === 0).map((s) => s.id);
  let level = 0;

  while (currentLevel.length > 0) {
    const nextLevel: string[] = [];
    for (let i = 0; i < currentLevel.length; i++) {
      result.set(currentLevel[i], { level, indexInLevel: i, levelSize: currentLevel.length });
      for (const neighbor of adjacency.get(currentLevel[i]) ?? []) {
        const deg = (inDegree.get(neighbor) ?? 1) - 1;
        inDegree.set(neighbor, deg);
        if (deg === 0) nextLevel.push(neighbor);
      }
    }
    currentLevel = nextLevel;
    level++;
  }

  // Any remaining steps (orphans or cycles) get placed at the end
  for (const s of steps) {
    if (!result.has(s.id)) {
      result.set(s.id, { level, indexInLevel: 0, levelSize: 1 });
    }
  }

  return result;
}

// ── Main View ──

interface PipelineFlowViewProps {
  pipelineId: string | null;
  onBack: () => void;
}

export default function PipelineFlowView({ pipelineId, onBack }: PipelineFlowViewProps) {
  const [detail, setDetail] = useState<PipelineDetail | null>(null);
  const [activeRun, setActiveRun] = useState<PipelineRun | null>(null);
  const [triggering, setTriggering] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [viewingStep, setViewingStep] = useState<StepRun | null>(null);
  const [editingStep, setEditingStep] = useState<PipelineStep | null>(null);
  const [showSettings, setShowSettings] = useState(false);
  const [flowNodes, setFlowNodes] = useState<Node[]>([]);

  const handleViewOutput = useCallback((sr: StepRun) => {
    setViewingStep(sr);
    setEditingStep(null);
  }, []);

  const handleEditStep = useCallback((stepId: string) => {
    if (!detail) return;
    const step = detail.steps.find((s) => s.id === stepId);
    if (step) { setEditingStep(step); setViewingStep(null); }
  }, [detail]);

  const handleSaveStep = useCallback(async (updated: PipelineStep) => {
    if (!detail || !pipelineId) return;
    const newSteps = detail.steps.map((s) => s.id === updated.id ? updated : s);
    try {
      await updatePipeline(pipelineId, { steps: newSteps });
      setEditingStep(null);
      const d = await getPipelineDetail(pipelineId);
      setDetail(d);
    } catch (e) {
      setError(String(e));
    }
  }, [detail, pipelineId]);

  const handleSaveSettings = useCallback(async (patch: { name?: string; description?: string; trigger?: PipelineTrigger }) => {
    if (!pipelineId) return;
    try {
      await updatePipeline(pipelineId, patch);
      setShowSettings(false);
      const d = await getPipelineDetail(pipelineId);
      setDetail(d);
    } catch (e) {
      setError(String(e));
    }
  }, [pipelineId]);

  const handleAddStep = useCallback(async () => {
    if (!detail || !pipelineId) return;
    const n = detail.steps.length + 1;
    const newStepId = `step-${Date.now()}`;
    const newStep: PipelineStep = {
      id: newStepId,
      name: `Step ${n}`,
      prompt: "Describe the task for this step...",
    };
    const newSteps = [...detail.steps, newStep];
    // No auto-connections — user drags handles to connect
    try {
      await updatePipeline(pipelineId, { steps: newSteps });
      const d = await getPipelineDetail(pipelineId);
      setDetail(d);
    } catch (e) {
      setError(String(e));
    }
  }, [detail, pipelineId]);

  const handleDeleteStep = useCallback(async (stepId: string) => {
    if (!detail || !pipelineId) return;
    const newSteps = detail.steps.filter((s) => s.id !== stepId);
    if (newSteps.length === 0) return;

    // Remove connections involving this step
    // Reconnect: if A→deleted→B, create A→B
    const incoming = detail.connections.filter((c) => c.toStep === stepId).map((c) => c.fromStep);
    const outgoing = detail.connections.filter((c) => c.fromStep === stepId).map((c) => c.toStep);
    let newConnections = detail.connections.filter(
      (c) => c.fromStep !== stepId && c.toStep !== stepId
    );
    // Bridge: connect each incoming to each outgoing
    for (const from of incoming) {
      for (const to of outgoing) {
        if (!newConnections.some((c) => c.fromStep === from && c.toStep === to)) {
          newConnections.push({ fromStep: from, toStep: to });
        }
      }
    }

    try {
      await updatePipeline(pipelineId, { steps: newSteps, connections: newConnections });
      const d = await getPipelineDetail(pipelineId);
      setDetail(d);
    } catch (e) {
      setError(String(e));
    }
  }, [detail, pipelineId]);

  const handleDeleteEdge = useCallback(async (edgeId: string) => {
    if (!detail || !pipelineId) return;
    // edgeId format: "fromStep->toStep"
    const [fromStep, toStep] = edgeId.split("->");
    if (!fromStep || !toStep) return;
    const newConnections = detail.connections.filter(
      (c) => !(c.fromStep === fromStep && c.toStep === toStep)
    );
    try {
      await updatePipeline(pipelineId, { connections: newConnections });
      const d = await getPipelineDetail(pipelineId);
      setDetail(d);
    } catch (e) {
      setError(String(e));
    }
  }, [detail, pipelineId]);

  const handleConnect: OnConnect = useCallback(async (connection: Connection) => {
    if (!detail || !pipelineId || !connection.source || !connection.target) return;
    // Prevent duplicate connections
    if (detail.connections.some(
      (c) => c.fromStep === connection.source && c.toStep === connection.target
    )) return;
    // Prevent self-connection
    if (connection.source === connection.target) return;

    const newConnection: StepConnection = {
      fromStep: connection.source,
      toStep: connection.target,
    };
    const newConnections = [...detail.connections, newConnection];

    try {
      await updatePipeline(pipelineId, { connections: newConnections });
      const d = await getPipelineDetail(pipelineId);
      setDetail(d);
    } catch (e) {
      setError(String(e));
    }
  }, [detail, pipelineId]);

  const loadDetail = useCallback(async () => {
    if (!pipelineId) return;
    try {
      const d = await getPipelineDetail(pipelineId);
      setDetail(d);
      if (d.recentRuns.length > 0) {
        const latest = d.recentRuns[0];
        if (latest.status === "running" || activeRun?.id === latest.id) {
          setActiveRun(latest);
        } else if (!activeRun) {
          setActiveRun(latest);
        }
      }
    } catch (e) {
      setError(String(e));
    }
  }, [pipelineId]);

  useEffect(() => {
    setActiveRun(null);
    setViewingStep(null);
    setEditingStep(null);
    loadDetail();
  }, [loadDetail]);

  useEffect(() => {
    if (!activeRun || !pipelineId || activeRun.status !== "running") return;
    const interval = setInterval(async () => {
      try {
        const run = await getPipelineRun(pipelineId, activeRun.id);
        if (run) {
          setActiveRun(run);
          if (viewingStep) {
            const updated = run.stepRuns.find((s) => s.stepId === viewingStep.stepId);
            if (updated) setViewingStep(updated);
          }
          if (run.status !== "running") loadDetail();
        }
      } catch { /* ignore */ }
    }, 1500);
    return () => clearInterval(interval);
  }, [activeRun?.id, activeRun?.status, pipelineId, loadDetail, viewingStep?.stepId]);

  useEffect(() => {
    let unlisten: (() => void) | undefined;
    onPipelineNotification(() => { loadDetail(); }).then((fn) => { unlisten = fn; });
    return () => { unlisten?.(); };
  }, [loadDetail]);


  const handleTrigger = async () => {
    if (!pipelineId) return;
    try {
      setTriggering(true);
      setError(null);
      setViewingStep(null);
      setEditingStep(null);
      await triggerPipeline(pipelineId);
      setTimeout(async () => {
        try {
          const d = await getPipelineDetail(pipelineId);
          setDetail(d);
          if (d.recentRuns.length > 0) setActiveRun(d.recentRuns[0]);
        } catch { /* ignore */ }
        setTriggering(false);
      }, 1500);
    } catch (e) {
      setError(String(e));
      setTriggering(false);
    }
  };

  const handleCancel = async () => {
    if (!pipelineId) return;
    try {
      await cancelPipeline(pipelineId);
      setTimeout(() => loadDetail(), 1000);
    } catch (e) {
      setError(String(e));
    }
  };

  const isRunning = activeRun?.status === "running";

  const handleSelectRun = async (runId: string) => {
    if (!pipelineId) return;
    try {
      const run = await getPipelineRun(pipelineId, runId);
      if (run) { setActiveRun(run); setViewingStep(null); }
    } catch { /* ignore */ }
  };

  // Build nodes with automatic layout by topological levels
  useEffect(() => {
    if (!detail) { setFlowNodes([]); return; }
    const srMap = new Map<string, StepRun>();
    if (activeRun) for (const sr of activeRun.stepRuns) srMap.set(sr.stepId, sr);

    // Compute topological levels for layout
    const levels = computeTopologicalLevels(detail.steps, detail.connections);

    // Auto-layout: always recompute positions from topology
    // (user can still drag nodes, but they snap back on connection changes)
    const nodes = detail.steps.map((step, idx) => {
      const levelInfo = levels.get(step.id) ?? { level: idx, indexInLevel: 0, levelSize: 1 };
      const autoX = (levelInfo.indexInLevel - (levelInfo.levelSize - 1) / 2) * 240;
      const autoY = levelInfo.level * 120;
      return {
        id: step.id,
        type: "step" as const,
        position: { x: autoX, y: autoY },
        data: {
          label: step.name,
          prompt: step.prompt,
          stepId: step.id,
          model: step.model,
          provider: step.provider,
          stepRun: srMap.get(step.id),
          stepIndex: idx,
          totalSteps: detail.steps.length,
          onViewOutput: handleViewOutput,
          onEditStep: handleEditStep,
          onDeleteStep: handleDeleteStep,
        } satisfies StepNodeData,
      };
    });
    setFlowNodes(nodes);
  }, [detail, activeRun, handleViewOutput, handleEditStep, handleDeleteStep]);

  const onNodesChange: OnNodesChange = useCallback((changes) => {
    setFlowNodes((nds) => applyNodeChanges(changes, nds));
  }, []);

  const edges = useMemo<Edge[]>(() => {
    if (!detail) return [];
    const srMap = new Map<string, StepRun>();
    if (activeRun) for (const sr of activeRun.stepRuns) srMap.set(sr.stepId, sr);

    return detail.connections
      .filter((c) => c.fromStep !== "__trigger__")
      .map((conn) => {
        const fs = srMap.get(conn.fromStep)?.status;
        const ts = srMap.get(conn.toStep)?.status;

        let stroke = "#2a2a2a";
        let animated = false;
        if (fs === "success" && ts === "running") { stroke = "#f59e0b"; animated = true; }
        else if (fs === "success") stroke = "#22c55e";
        else if (fs === "error") stroke = "#ef4444";

        return {
          id: `${conn.fromStep}->${conn.toStep}`,
          source: conn.fromStep,
          target: conn.toStep,
          type: "deletable",
          animated,
          style: { stroke, strokeWidth: 1.5 },
          data: { onDelete: handleDeleteEdge },
        };
      });
  }, [detail, activeRun, handleDeleteEdge]);

  if (!pipelineId) {
    return (
      <div style={{ display: "flex", alignItems: "center", justifyContent: "center", height: "100%", color: "#52525b", fontFamily: "monospace", fontSize: 12 }}>
        Select a pipeline from the sidebar
      </div>
    );
  }

  if (!detail) {
    return (
      <div style={{ display: "flex", alignItems: "center", justifyContent: "center", height: "100%", color: "#52525b" }}>
        <Loader2 size={16} style={{ animation: "spin 1s linear infinite", marginRight: 8 }} /> Loading...
      </div>
    );
  }

  const totalTokens = activeRun ? activeRun.tokenUsage.input + activeRun.tokenUsage.output : 0;

  return (
    <div style={{ display: "flex", flexDirection: "column", height: "100%", background: "#0a0a0a", position: "relative" }}>
      {/* Toolbar */}
      <div
        style={{
          display: "flex", alignItems: "center", gap: 8, padding: "5px 10px",
          borderBottom: "1px solid #1e1e1e", background: "#0c0c0c", flexShrink: 0, fontFamily: "monospace",
        }}
      >
        <button onClick={onBack} style={{ border: "none", background: "transparent", color: "#666", cursor: "pointer", padding: 3, display: "flex" }}>
          <ArrowLeft size={14} />
        </button>
        <span style={{ fontSize: 12, fontWeight: 600, color: "#e0e0e0" }}>{detail.name}</span>
        <button
          onClick={() => { setShowSettings(true); setEditingStep(null); setViewingStep(null); }}
          style={{ border: "none", background: "transparent", color: "#555", cursor: "pointer", padding: 2, display: "flex" }}
          title="Pipeline settings"
        >
          <Settings size={12} />
        </button>
        <button
          onClick={() => {
            const template = {
              name: detail.name,
              description: detail.description,
              trigger: detail.trigger,
              steps: detail.steps,
              connections: detail.connections,
            };
            const blob = new Blob([JSON.stringify(template, null, 2)], { type: "application/json" });
            const url = URL.createObjectURL(blob);
            const a = document.createElement("a");
            a.href = url;
            a.download = `${detail.name.replace(/\s+/g, "-").toLowerCase()}.pipeline.json`;
            a.click();
            URL.revokeObjectURL(url);
          }}
          style={{ border: "none", background: "transparent", color: "#555", cursor: "pointer", padding: 2, display: "flex" }}
          title="Export pipeline"
        >
          <Download size={12} />
        </button>
        <span style={{ fontSize: 10, color: "#555" }}>{detail.steps.length} steps</span>
        <div style={{ flex: 1 }} />

        {detail.recentRuns.length > 0 && (
          <select
            value={activeRun?.id ?? ""}
            onChange={(e) => handleSelectRun(e.target.value)}
            style={{ background: "#141414", border: "1px solid #2a2a2a", color: "#888", borderRadius: 3, padding: "2px 6px", fontSize: 10, fontFamily: "monospace", outline: "none" }}
          >
            {detail.recentRuns.map((r) => (
              <option key={r.id} value={r.id}>{r.id} {r.status}</option>
            ))}
          </select>
        )}

        {activeRun && (
          <div style={{ display: "flex", gap: 6, fontSize: 10, color: "#555", alignItems: "center" }}>
            {activeRun.status === "running" && <span style={{ color: "#f59e0b", display: "flex", alignItems: "center", gap: 3 }}><Loader2 size={10} style={{ animation: "spin 1s linear infinite" }} /> running</span>}
            {activeRun.status === "success" && <span style={{ color: "#22c55e", display: "flex", alignItems: "center", gap: 3 }}><CheckCircle2 size={10} /> done</span>}
            {activeRun.status === "error" && <span style={{ color: "#ef4444", display: "flex", alignItems: "center", gap: 3 }}><XCircle size={10} /> error</span>}
            {activeRun.completedAt && <span style={{ display: "flex", alignItems: "center", gap: 2 }}><Clock size={9} /> {formatElapsed(activeRun.startedAt, activeRun.completedAt)}</span>}
            {totalTokens > 0 && <span style={{ display: "flex", alignItems: "center", gap: 2 }}><Coins size={9} /> {formatTokens(totalTokens)}</span>}
          </div>
        )}

        <button
          onClick={handleAddStep}
          style={{
            border: "1px solid #333", background: "transparent",
            color: "#888", cursor: "pointer",
            padding: "3px 10px", borderRadius: 3, fontSize: 10, fontFamily: "monospace",
            display: "flex", alignItems: "center", gap: 4,
          }}
          title="Add step"
        >
          <Plus size={10} /> step
        </button>
        {isRunning ? (
          <button
            onClick={handleCancel}
            style={{
              border: "1px solid #ef4444", background: "transparent",
              color: "#ef4444", cursor: "pointer",
              padding: "3px 10px", borderRadius: 3, fontSize: 10, fontWeight: 600, fontFamily: "monospace",
              display: "flex", alignItems: "center", gap: 4,
            }}
          >
            <Square size={10} /> stop
          </button>
        ) : (
          <button
            onClick={handleTrigger}
            disabled={triggering}
            style={{
              border: "1px solid #333", background: triggering ? "transparent" : "#fafafa",
              color: triggering ? "#666" : "#0a0a0a", cursor: triggering ? "not-allowed" : "pointer",
              padding: "3px 10px", borderRadius: 3, fontSize: 10, fontWeight: 600, fontFamily: "monospace",
              display: "flex", alignItems: "center", gap: 4, opacity: triggering ? 0.5 : 1,
            }}
          >
            {triggering ? <Loader2 size={10} style={{ animation: "spin 1s linear infinite" }} /> : <Play size={10} />}
            {triggering ? "running" : "run"}
          </button>
        )}
      </div>

      {error && <div style={{ padding: "3px 10px", fontSize: 10, color: "#ef4444", fontFamily: "monospace" }}>{error}</div>}

      {/* Canvas */}
      <div style={{ flex: 1 }}>
        <ReactFlow
          nodes={flowNodes}
          edges={edges}
          onNodesChange={onNodesChange}
          nodeTypes={nodeTypes}
          edgeTypes={edgeTypes}
          fitView
          fitViewOptions={{ padding: 0.5 }}
          proOptions={{ hideAttribution: true }}
          onConnect={handleConnect}
          nodesConnectable
          panOnDrag
          zoomOnScroll
          minZoom={0.3}
          maxZoom={2.5}
          style={{ background: "#0a0a0a" }}
        >
          <Background color="#1a1a1a" gap={20} variant={BackgroundVariant.Dots} size={1} />
        </ReactFlow>
      </div>

      {/* Output panel */}
      {viewingStep && <OutputPanel stepRun={viewingStep} onClose={() => setViewingStep(null)} />}

      {/* Edit step sheet */}
      {editingStep && (
        <EditStepSheet key={editingStep.id} step={editingStep} onSave={handleSaveStep} onClose={() => setEditingStep(null)} />
      )}

      {/* Pipeline settings sheet */}
      {showSettings && detail && (
        <PipelineSettingsSheet detail={detail} onSave={handleSaveSettings} onClose={() => setShowSettings(false)} />
      )}
    </div>
  );
}
