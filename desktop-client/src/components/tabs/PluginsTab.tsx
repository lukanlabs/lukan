import { useState, useEffect, useCallback, useRef, useMemo } from "react";
import type { PluginInfo, RemotePlugin, WhatsAppGroup, PluginCommand, PluginToolsInfo } from "../../lib/types";
import {
  listPlugins,
  installPlugin,
  installRemotePlugin,
  removePlugin,
  startPlugin,
  stopPlugin,
  restartPlugin,
  getPluginConfig,
  setPluginConfigField,
  getPluginLogs,
  listRemotePlugins,
  getWhatsappQr,
  checkWhatsappAuth,
  fetchWhatsappGroups,
  getPluginCommands,
  runPluginCommand,
  getPluginManifestTools,
  listTools,
} from "../../lib/tauri";
import QRCode from "qrcode";
import { useToast } from "../ui/Toast";
import {
  Check,
  Play,
  Square,
  Trash2,
  Download,
  FolderOpen,
  ArrowLeft,
  RefreshCw,
  ScrollText,
  Settings2,
  Package,
  X,
  Loader2,
  RotateCw,
  QrCode,
  Plus,
  Phone,
  Users,
  ChevronRight,
  Eye,
  EyeOff,
  Wrench,
  RotateCcw,
} from "lucide-react";

/** Known core tool groups (same as ToolsTab) */
const CORE_GROUPS: Record<string, string> = {
  ReadFiles: "File ops",
  WriteFile: "File ops",
  EditFile: "File ops",
  Grep: "Search",
  Glob: "Search",
  Bash: "Execution",
  WebFetch: "Web",
  TaskAdd: "Tasks",
  TaskList: "Tasks",
  TaskUpdate: "Tasks",
  LoadSkill: "Skills",
  SubmitPlan: "Planner",
  PlannerQuestion: "Planner",
};

const GROUP_ORDER = ["File ops", "Search", "Execution", "Web", "Tasks", "Skills", "Planner"];

// ---------------------------------------------------------------------------
// Main Component
// ---------------------------------------------------------------------------

export default function PluginsTab() {
  const { toast } = useToast();

  const [plugins, setPlugins] = useState<PluginInfo[]>([]);
  const [selectedPlugin, setSelectedPlugin] = useState<string | null>(null);
  const [showRegistry, setShowRegistry] = useState(false);

  const [localPath, setLocalPath] = useState("");
  const [installingLocal, setInstallingLocal] = useState(false);

  const [actionLoading, setActionLoading] = useState<"start" | "stop" | "restart" | "remove" | null>(null);
  const [pluginConfig, setPluginConfig] = useState<Record<string, unknown>>({});
  const [pluginLogs, setPluginLogs] = useState("");
  const [logsRefreshing, setLogsRefreshing] = useState(false);
  const [pluginCommands, setPluginCommands] = useState<PluginCommand[]>([]);
  const [commandRunning, setCommandRunning] = useState<string | null>(null);
  const [commandOutput, setCommandOutput] = useState<string | null>(null);

  const [whatsappAuthed, setWhatsappAuthed] = useState(false);

  const [showQr, setShowQr] = useState(false);
  const [qrDataUrl, setQrDataUrl] = useState<string | null>(null);
  const [qrLoading, setQrLoading] = useState(false);
  const [qrLinked, setQrLinked] = useState(false);

  const lastQrRef = useRef<string | null>(null);
  useEffect(() => {
    if (!showQr || qrLinked) return;
    let cancelled = false;
    const poll = async () => {
      try {
        const qr = await getWhatsappQr();
        if (cancelled) return;
        if (!qr && qrDataUrl) {
          setQrLinked(true);
          toast("success", "WhatsApp linked successfully!");
        } else if (qr && qr !== lastQrRef.current) {
          lastQrRef.current = qr;
          const url = await QRCode.toDataURL(qr, { width: 256, margin: 2, color: { dark: "#000000", light: "#ffffff" } });
          if (!cancelled) {
            setQrDataUrl(url);
            setQrLoading(false);
          }
        }
      } catch {}
    };
    poll();
    const interval = setInterval(poll, 2000);
    return () => { cancelled = true; clearInterval(interval); };
  }, [showQr, qrDataUrl, qrLinked, toast]);

  const [remotePlugins, setRemotePlugins] = useState<RemotePlugin[]>([]);
  const [registryLoading, setRegistryLoading] = useState(false);
  const [installingRemote, setInstallingRemote] = useState<string | null>(null);

  // ── Data fetching ──

  const refreshPlugins = useCallback(async () => {
    try {
      const list = await listPlugins();
      setPlugins(list);
    } catch (e) {
      toast("error", `Failed to list plugins: ${e}`);
    }
  }, [toast]);

  useEffect(() => { refreshPlugins(); }, [refreshPlugins]);

  const loadPluginDetail = useCallback(async (name: string) => {
    try {
      const [cfg, logs, cmds] = await Promise.all([
        getPluginConfig(name),
        getPluginLogs(name, 200),
        getPluginCommands(name).catch(() => [] as PluginCommand[]),
      ]);
      setPluginConfig(cfg);
      setPluginLogs(logs);
      setPluginCommands(cmds);
      if (name === "whatsapp") {
        const authed = await checkWhatsappAuth().catch(() => false);
        setWhatsappAuthed(authed);
      }
    } catch (e) {
      toast("error", `Failed to load plugin details: ${e}`);
    }
  }, [toast]);

  const selectPlugin = useCallback((name: string) => {
    setSelectedPlugin(name);
    loadPluginDetail(name);
  }, [loadPluginDetail]);

  const goBack = useCallback(() => {
    setSelectedPlugin(null);
    setPluginConfig({});
    setPluginLogs("");
    setActionLoading(null);
    setPluginCommands([]);
    setCommandRunning(null);
    setCommandOutput(null);
    setWhatsappAuthed(false);
  }, []);

  // ── Handlers ──

  const handleInstallLocal = async () => {
    const path = localPath.trim();
    if (!path) return;
    setInstallingLocal(true);
    try {
      const installed = await installPlugin(path);
      toast("success", `Plugin '${installed}' installed`);
      setLocalPath("");
      await refreshPlugins();
    } catch (e) {
      toast("error", `${e}`);
    } finally {
      setInstallingLocal(false);
    }
  };

  const handleAction = async (action: "start" | "stop" | "restart" | "remove", name: string) => {
    setActionLoading(action);
    try {
      if (action === "start") await startPlugin(name);
      else if (action === "stop") await stopPlugin(name);
      else if (action === "restart") await restartPlugin(name);
      else if (action === "remove") { await removePlugin(name); await refreshPlugins(); goBack(); return; }
      toast("success", `Plugin '${name}' ${action === "restart" ? "restarted" : action + "ed"}`);
      await refreshPlugins();
      const logs = await getPluginLogs(name, 200);
      setPluginLogs(logs);
    } catch (e) {
      toast("error", `Failed to ${action} plugin: ${e}`);
    } finally {
      setActionLoading(null);
    }
  };

  const handleShowQr = () => {
    setShowQr(true);
    setQrLoading(true);
    setQrDataUrl(null);
    setQrLinked(false);
    lastQrRef.current = null;
    // useEffect poll will pick up the QR and update qrDataUrl
  };

  const handleConfigSave = async (pluginName: string, key: string, value: string) => {
    try {
      const parsed = (() => { try { return JSON.parse(value); } catch { return value; } })();
      await setPluginConfigField(pluginName, key, parsed);
      const cfg = await getPluginConfig(pluginName);
      setPluginConfig(cfg);
      toast("success", "Config updated");
    } catch (e) {
      toast("error", `Failed to save config: ${e}`);
    }
  };

  const handleRefreshLogs = async (name: string) => {
    setLogsRefreshing(true);
    try {
      const logs = await getPluginLogs(name, 200);
      setPluginLogs(logs);
    } catch (e) {
      toast("error", `Failed to refresh logs: ${e}`);
    } finally {
      setLogsRefreshing(false);
    }
  };

  const handleRunCommand = async (pluginName: string, command: string) => {
    setCommandRunning(command);
    setCommandOutput(null);
    try {
      const output = await runPluginCommand(pluginName, command);
      if (output?.trim()) setCommandOutput(output.trim());
      toast("success", `Command '${command}' completed`);
      const cfg = await getPluginConfig(pluginName);
      setPluginConfig(cfg);
    } catch (e) {
      toast("error", `Command '${command}' failed: ${e}`);
    } finally {
      setCommandRunning(null);
    }
  };

  const openRegistry = async () => {
    setShowRegistry(true);
    setRegistryLoading(true);
    try {
      const remotes = await listRemotePlugins();
      setRemotePlugins(remotes);
    } catch (e) {
      toast("error", `Failed to fetch registry: ${e}`);
    } finally {
      setRegistryLoading(false);
    }
  };

  const handleInstallRemote = async (name: string) => {
    setInstallingRemote(name);
    try {
      const installed = await installRemotePlugin(name);
      toast("success", `Plugin '${installed}' installed`);
      await refreshPlugins();
      const remotes = await listRemotePlugins();
      setRemotePlugins(remotes);
    } catch (e) {
      toast("error", `${e}`);
    } finally {
      setInstallingRemote(null);
    }
  };

  const activePlugin = selectedPlugin ? plugins.find((p) => p.name === selectedPlugin) ?? null : null;

  // ── Render ──

  return (
    <div style={{ animation: "fadeIn 0.15s ease-out" }}>
      {selectedPlugin && activePlugin ? (
        <DetailView
          plugin={activePlugin}
          config={pluginConfig}
          logs={pluginLogs}
          actionLoading={actionLoading}
          logsRefreshing={logsRefreshing}
          commands={pluginCommands}
          commandRunning={commandRunning}
          commandOutput={commandOutput}
          whatsappAuthed={whatsappAuthed}
          onBack={goBack}
          onAction={handleAction}
          onConfigSave={handleConfigSave}
          onRefreshLogs={handleRefreshLogs}
          onShowQr={handleShowQr}
          onRunCommand={handleRunCommand}
        />
      ) : (
        <MasterView
          plugins={plugins}
          localPath={localPath}
          installingLocal={installingLocal}
          onLocalPathChange={setLocalPath}
          onInstallLocal={handleInstallLocal}
          onSelectPlugin={selectPlugin}
          onOpenRegistry={openRegistry}
        />
      )}

      {/* QR Modal */}
      {showQr && (
        <QrModal
          qrDataUrl={qrDataUrl}
          qrLoading={qrLoading}
          qrLinked={qrLinked}
          onClose={() => setShowQr(false)}
          onDone={() => { setShowQr(false); refreshPlugins(); }}
        />
      )}

      {/* Registry Modal */}
      {showRegistry && (
        <RegistryModal
          remotePlugins={remotePlugins}
          loading={registryLoading}
          installingRemote={installingRemote}
          onInstall={handleInstallRemote}
          onClose={() => setShowRegistry(false)}
        />
      )}
    </div>
  );
}

// ===========================================================================
// Master View
// ===========================================================================

interface MasterViewProps {
  plugins: PluginInfo[];
  localPath: string;
  installingLocal: boolean;
  onLocalPathChange: (v: string) => void;
  onInstallLocal: () => void;
  onSelectPlugin: (name: string) => void;
  onOpenRegistry: () => void;
}

function MasterView({
  plugins, localPath, installingLocal,
  onLocalPathChange, onInstallLocal, onSelectPlugin, onOpenRegistry,
}: MasterViewProps) {
  return (
    <>
      {/* Header */}
      <div className="flex items-center justify-between mb-4">
        <div className="flex items-center gap-2">
          <span className="text-sm font-semibold" style={{ color: "var(--text-primary)" }}>
            Plugins
          </span>
          <span className="text-[10px] font-medium px-1.5 py-0.5 rounded" style={{
            background: "rgba(255,255,255,0.05)", color: "var(--text-muted)",
            border: "1px solid var(--border)",
          }}>{plugins.length}</span>
        </div>
        <button
          onClick={onOpenRegistry}
          className="inline-flex items-center gap-1 px-2.5 py-1.5 rounded-lg text-xs font-medium cursor-pointer"
          style={{ background: "var(--bg-tertiary)", color: "var(--text-primary)", border: "1px solid var(--border)" }}
        >
          <Download size={11} />
          Registry
        </button>
      </div>

      {/* Install local */}
      <div className="flex gap-2 mb-4">
        <input
          value={localPath}
          placeholder="/path/to/plugin"
          onChange={(e) => onLocalPathChange(e.target.value)}
          onKeyDown={(e) => { if (e.key === "Enter") onInstallLocal(); }}
          className="flex-1 px-2.5 py-1.5 rounded-lg text-xs outline-none"
          style={{ background: "var(--bg-tertiary)", border: "1px solid var(--border)", color: "var(--text-primary)" }}
        />
        <button
          onClick={onInstallLocal}
          disabled={installingLocal || !localPath.trim()}
          className="inline-flex items-center gap-1 px-2.5 py-1.5 rounded-lg text-xs font-medium cursor-pointer border-none shrink-0"
          style={{
            background: "#fafafa", color: "#09090b",
            opacity: installingLocal || !localPath.trim() ? 0.4 : 1,
            pointerEvents: installingLocal || !localPath.trim() ? "none" : "auto",
          }}
        >
          {installingLocal ? <Loader2 size={11} className="animate-spin" /> : <FolderOpen size={11} />}
          Install
        </button>
      </div>

      {/* Plugin list */}
      {plugins.length === 0 ? (
        <div className="text-center py-10 rounded-lg" style={{
          color: "var(--text-muted)", border: "1px dashed var(--border)",
        }}>
          <Package size={24} style={{ margin: "0 auto 8px", opacity: 0.3 }} />
          <p className="text-xs">No plugins installed</p>
        </div>
      ) : (
        <div className="flex flex-col gap-1">
          {plugins.map((plugin) => (
            <div
              key={plugin.name}
              className="flex items-center gap-3 px-3 py-2.5 rounded-lg cursor-pointer"
              style={{ background: "transparent", transition: "background 120ms" }}
              onClick={() => onSelectPlugin(plugin.name)}
              onMouseEnter={(e) => { e.currentTarget.style.background = "var(--bg-hover)"; }}
              onMouseLeave={(e) => { e.currentTarget.style.background = "transparent"; }}
            >
              <Package size={13} style={{ color: "var(--text-muted)" }} />
              <div className="flex-1 min-w-0">
                <div className="flex items-center gap-2">
                  <span className="text-xs font-medium" style={{ color: "var(--text-primary)" }}>
                    {plugin.name}
                  </span>
                  <span className="text-[10px] font-mono" style={{ color: "var(--text-muted)" }}>
                    v{plugin.version}
                  </span>
                  {plugin.alias && (
                    <span className="text-[10px] px-1 rounded" style={{
                      background: "rgba(255,255,255,0.05)", color: "var(--text-muted)",
                    }}>{plugin.alias}</span>
                  )}
                </div>
                {plugin.description && (
                  <span className="text-[10px] block truncate" style={{ color: "var(--text-muted)" }}>
                    {plugin.description}
                  </span>
                )}
              </div>
              <div className="flex items-center gap-2 shrink-0">
                {plugin.pluginType === "channel" && (
                  <span className="w-1.5 h-1.5 rounded-full" style={{
                    background: plugin.running ? "#4ade80" : "var(--text-muted)",
                    boxShadow: plugin.running ? "0 0 6px rgba(74,222,128,0.4)" : "none",
                  }} />
                )}
                <ChevronRight size={12} style={{ color: "var(--text-muted)" }} />
              </div>
            </div>
          ))}
        </div>
      )}
    </>
  );
}

// ===========================================================================
// Detail View
// ===========================================================================

interface DetailViewProps {
  plugin: PluginInfo;
  config: Record<string, unknown>;
  logs: string;
  actionLoading: "start" | "stop" | "restart" | "remove" | null;
  logsRefreshing: boolean;
  commands: PluginCommand[];
  commandRunning: string | null;
  commandOutput: string | null;
  whatsappAuthed: boolean;
  onBack: () => void;
  onAction: (action: "start" | "stop" | "restart" | "remove", name: string) => void;
  onConfigSave: (pluginName: string, key: string, value: string) => void;
  onRefreshLogs: (name: string) => void;
  onShowQr: () => void;
  onRunCommand: (pluginName: string, command: string) => void;
}

function DetailView({
  plugin, config, logs, actionLoading, logsRefreshing,
  commands, commandRunning, commandOutput, whatsappAuthed,
  onBack, onAction, onConfigSave, onRefreshLogs, onShowQr, onRunCommand,
}: DetailViewProps) {
  const logsEndRef = useRef<HTMLPreElement>(null);
  useEffect(() => {
    if (logsEndRef.current) logsEndRef.current.scrollTop = logsEndRef.current.scrollHeight;
  }, [logs]);

  const configKeys = Object.keys(config);
  const isWhatsapp = plugin.name.includes("whatsapp");

  return (
    <div style={{ animation: "fadeIn 0.15s ease-out" }}>
      {/* Header with back + uninstall */}
      <div className="flex items-center justify-between mb-1">
        <button
          onClick={onBack}
          className="inline-flex items-center gap-1 text-xs cursor-pointer border-none bg-transparent"
          style={{ color: "var(--text-muted)", padding: 0 }}
        >
          <ArrowLeft size={12} />
          Back
        </button>
        <button
          onClick={() => onAction("remove", plugin.name)}
          disabled={actionLoading !== null}
          className="inline-flex items-center gap-1 px-2 py-1 rounded-lg text-[10px] font-medium cursor-pointer border-none"
          style={{
            background: "rgba(220,38,38,0.12)", color: "#fb7185", border: "1px solid rgba(251,113,133,0.15)",
            opacity: actionLoading !== null ? 0.4 : 1, pointerEvents: actionLoading !== null ? "none" : "auto",
          }}
        >
          {actionLoading === "remove" ? <Loader2 size={10} className="animate-spin" /> : <Trash2 size={10} />}
          Uninstall
        </button>
      </div>

      {/* Plugin info */}
      <div className="mb-4">
        <div className="flex items-center gap-2 flex-wrap mb-1">
          <span className="text-sm font-semibold" style={{ color: "var(--text-primary)" }}>
            {plugin.name}
          </span>
          <span className="text-[10px] font-mono" style={{ color: "var(--text-muted)" }}>
            v{plugin.version}
          </span>
          <span className="text-[10px] px-1.5 py-0.5 rounded" style={{
            background: "rgba(255,255,255,0.05)", color: "var(--text-muted)",
            border: "1px solid var(--border)",
          }}>{plugin.pluginType}</span>
          {plugin.pluginType === "channel" && (
            <span className="text-[10px] font-semibold px-1.5 py-0.5 rounded" style={{
              background: plugin.running ? "rgba(74,222,128,0.12)" : "rgba(255,255,255,0.05)",
              color: plugin.running ? "#4ade80" : "var(--text-muted)",
            }}>{plugin.running ? "Running" : "Stopped"}</span>
          )}
        </div>
        {plugin.description && (
          <p className="text-[11px]" style={{ color: "var(--text-secondary)" }}>{plugin.description}</p>
        )}
      </div>

      {/* Action buttons (only for channel plugins) */}
      {plugin.pluginType === "channel" && (
        <div className="flex gap-1.5 flex-wrap mb-4">
          {plugin.running ? (
            <>
              <ActionBtn
                icon={actionLoading === "stop" ? <Loader2 size={11} className="animate-spin" /> : <Square size={11} />}
                label={actionLoading === "stop" ? "Stopping..." : "Stop"}
                onClick={() => onAction("stop", plugin.name)}
                disabled={actionLoading !== null}
              />
              <ActionBtn
                icon={actionLoading === "restart" ? <Loader2 size={11} className="animate-spin" /> : <RotateCw size={11} />}
                label={actionLoading === "restart" ? "Restarting..." : "Restart"}
                onClick={() => onAction("restart", plugin.name)}
                disabled={actionLoading !== null}
                variant="secondary"
              />
            </>
          ) : (
            <ActionBtn
              icon={actionLoading === "start" ? <Loader2 size={11} className="animate-spin" /> : <Play size={11} />}
              label={actionLoading === "start" ? "Starting..." : "Start"}
              onClick={() => onAction("start", plugin.name)}
              disabled={actionLoading !== null}
            />
          )}
        </div>
      )}

      {/* Commands */}
      {commands.length > 0 && (
        <Section icon={<Settings2 size={12} />} title="Commands">
          <div className="flex flex-col gap-2">
            {commands.map((cmd) => {
              const isAuth = cmd.name === "auth";
              const authed = isAuth && (plugin.name === "whatsapp" ? whatsappAuthed : !!config.accessToken);
              const btnLabel = isAuth
                ? (commandRunning === cmd.name ? "..." : authed ? "Re-authenticate" : "Authenticate")
                : (commandRunning === cmd.name ? "..." : "Run");
              return (
                <div key={cmd.name} className="flex items-center justify-between gap-2">
                  <div className="min-w-0">
                    <span className="text-xs font-medium" style={{ color: "var(--text-primary)" }}>
                      {cmd.name}
                    </span>
                    {isAuth && (
                      <span className="text-[10px] ml-1.5 font-medium" style={{
                        color: authed ? "#4ade80" : "var(--text-muted)",
                      }}>{authed ? "authenticated" : "not authenticated"}</span>
                    )}
                    {cmd.description && (
                      <p className="text-[10px]" style={{ color: "var(--text-muted)" }}>{cmd.description}</p>
                    )}
                  </div>
                  <ActionBtn
                    icon={commandRunning === cmd.name ? <Loader2 size={10} className="animate-spin" /> : <Play size={10} />}
                    label={btnLabel}
                    onClick={() => isAuth && isWhatsapp ? onShowQr() : onRunCommand(plugin.name, cmd.name)}
                    disabled={commandRunning !== null}
                    small
                  />
                </div>
              );
            })}
          </div>
          {commandOutput && (
            <pre className="text-[10px] mt-2 p-2 rounded-lg overflow-auto" style={{
              background: "var(--bg-base)", color: "var(--text-secondary)",
              border: "1px solid var(--border)", maxHeight: 120,
              whiteSpace: "pre-wrap", wordBreak: "break-word", margin: 0,
              fontFamily: "'JetBrains Mono', monospace",
            }}>{commandOutput}</pre>
          )}
        </Section>
      )}

      {/* WhatsApp QR section */}
      {isWhatsapp && (
        <Section icon={<QrCode size={12} />} title="WhatsApp Auth">
          <div className="flex items-center justify-between">
            <span className="text-[11px]" style={{ color: "var(--text-secondary)" }}>
              Start plugin, then scan QR.
            </span>
            <ActionBtn icon={<QrCode size={11} />} label="View QR" onClick={onShowQr} />
          </div>
        </Section>
      )}

      {/* Tools */}
      {plugin.pluginType === "channel" && (
        <PluginToolsSection pluginName={plugin.name} config={config} onConfigSave={onConfigSave} />
      )}

      {/* Configuration */}
      {configKeys.length > 0 && (
        isWhatsapp ? (
          <WhatsAppConfig
            pluginName={plugin.name}
            config={config}
            running={plugin.running}
            onConfigSave={onConfigSave}
          />
        ) : (
          <Section icon={<Settings2 size={12} />} title="Configuration">
            <div className="flex flex-col gap-2">
              {configKeys.map((key) => (
                <ConfigField key={key} fieldKey={key} value={config[key]} onSave={(v) => onConfigSave(plugin.name, key, v)} />
              ))}
            </div>
          </Section>
        )
      )}

      {/* Logs */}
      {plugin.pluginType === "channel" && (
        <Section
          icon={<ScrollText size={12} />}
          title="Logs"
          action={
            <button
              onClick={() => onRefreshLogs(plugin.name)}
              disabled={logsRefreshing}
              className="inline-flex items-center gap-1 text-[10px] cursor-pointer border-none bg-transparent"
              style={{ color: "var(--text-muted)", opacity: logsRefreshing ? 0.5 : 1 }}
            >
              <RefreshCw size={10} className={logsRefreshing ? "animate-spin" : ""} />
              Refresh
            </button>
          }
        >
          <pre
            ref={logsEndRef}
            className="text-[10px] p-3 rounded-lg overflow-auto"
            style={{
              fontFamily: "'JetBrains Mono', monospace",
              minHeight: 120, maxHeight: 250, width: "100%",
              background: "var(--bg-base)", color: "var(--text-secondary)",
              border: "1px solid var(--border)",
              lineHeight: 1.5, whiteSpace: "pre", margin: 0,
            }}
          >{logs || "No logs available"}</pre>
        </Section>
      )}
    </div>
  );
}

// ===========================================================================
// Plugin Tools Section
// ===========================================================================

function PluginToolsSection({ pluginName, config, onConfigSave }: {
  pluginName: string;
  config: Record<string, unknown>;
  onConfigSave: (pluginName: string, key: string, value: string) => void;
}) {
  const { toast } = useToast();
  const [manifestInfo, setManifestInfo] = useState<PluginToolsInfo | null>(null);
  const [pluginTools, setPluginTools] = useState<string[]>([]);
  const [loading, setLoading] = useState(true);

  useEffect(() => {
    (async () => {
      try {
        const [info, allTools] = await Promise.all([
          getPluginManifestTools(pluginName),
          listTools(),
        ]);
        setManifestInfo(info);
        setPluginTools(allTools.filter((t) => t.source !== null).map((t) => t.name));
      } catch (e) {
        toast("error", `Failed to load tools info: ${e}`);
      } finally {
        setLoading(false);
      }
    })();
  }, [pluginName]);

  // Current enabled tools from config (or default_tools from manifest)
  // Default = only core tools enabled. Plugin tools from other plugins must be enabled manually.
  const enabledTools: Set<string> = useMemo(() => {
    const configTools = config.tools;
    if (Array.isArray(configTools)) return new Set(configTools as string[]);
    if (!manifestInfo) return new Set<string>();
    // No override — use defaults. Empty defaults = all core tools only.
    if (manifestInfo.defaultTools.length === 0) return new Set(manifestInfo.allCoreTools);
    return new Set(manifestInfo.defaultTools);
  }, [config.tools, manifestInfo]);

  const isUsingDefaults = !Array.isArray(config.tools);

  // Group core tools
  const coreGroups: [string, string[]][] = useMemo(() => {
    if (!manifestInfo) return [];
    const map = new Map<string, string[]>();
    for (const tool of manifestInfo.allCoreTools) {
      const group = CORE_GROUPS[tool] ?? "Other";
      if (!map.has(group)) map.set(group, []);
      map.get(group)!.push(tool);
    }
    return [...map.entries()].sort(([a], [b]) => {
      const ai = GROUP_ORDER.indexOf(a);
      const bi = GROUP_ORDER.indexOf(b);
      if (ai !== -1 && bi !== -1) return ai - bi;
      if (ai !== -1) return -1;
      if (bi !== -1) return 1;
      return a.localeCompare(b);
    });
  }, [manifestInfo]);

  const saveTools = (next: Set<string>) => {
    onConfigSave(pluginName, "tools", JSON.stringify([...next].sort()));
  };

  const toggle = (name: string) => {
    const next = new Set(enabledTools);
    if (next.has(name)) next.delete(name);
    else next.add(name);
    saveTools(next);
  };

  const setGroup = (tools: string[], enable: boolean) => {
    const next = new Set(enabledTools);
    for (const t of tools) {
      if (enable) next.add(t);
      else next.delete(t);
    }
    saveTools(next);
  };

  const resetToDefaults = () => {
    // Remove the tools key from config so it falls back to manifest defaults
    onConfigSave(pluginName, "tools", "null");
  };

  if (loading) {
    return (
      <Section icon={<Wrench size={12} />} title="Tools">
        <div className="flex items-center gap-2 py-2">
          <Loader2 size={12} className="animate-spin" style={{ color: "var(--text-muted)" }} />
          <span className="text-[10px]" style={{ color: "var(--text-muted)" }}>Loading tools...</span>
        </div>
      </Section>
    );
  }

  if (!manifestInfo) return null;

  const allToolNames = [...manifestInfo.allCoreTools, ...pluginTools];
  const totalTools = allToolNames.length;
  const enabledCount = allToolNames.filter((t) => enabledTools.has(t)).length;

  return (
    <Section
      icon={<Wrench size={12} />}
      title="Tools"
      action={
        <div className="flex items-center gap-2">
          <span
            className="text-[10px] px-1.5 py-0.5 rounded-full font-medium"
            style={{ background: "rgba(63,63,70,0.5)", color: "#a1a1aa" }}
          >
            {enabledCount}/{totalTools} enabled
          </span>
          {!isUsingDefaults && (
            <button
              onClick={resetToDefaults}
              className="inline-flex items-center gap-1 text-[10px] cursor-pointer border-none bg-transparent"
              style={{ color: "var(--text-muted)" }}
              title="Reset to manifest defaults"
            >
              <RotateCcw size={10} />
              Reset
            </button>
          )}
        </div>
      }
    >
      <div className="flex flex-col gap-3">
        {/* Core tool groups */}
        {coreGroups.map(([group, tools]) => {
          const groupEnabled = tools.filter((t) => enabledTools.has(t)).length;
          const allEnabled = groupEnabled === tools.length;
          const allDisabled = groupEnabled === 0;
          return (
            <div key={group}>
              <div className="flex items-center justify-between mb-1.5">
                <div className="flex items-center gap-1.5">
                  <span className="text-[10px] font-semibold" style={{ color: "var(--text-secondary)" }}>
                    {group}
                  </span>
                  <span
                    className="text-[9px] px-1 py-px rounded-full"
                    style={{
                      background: allDisabled ? "rgba(239,68,68,0.1)" : "rgba(63,63,70,0.4)",
                      color: allDisabled ? "#f87171" : "var(--text-muted)",
                    }}
                  >
                    {groupEnabled}/{tools.length}
                  </span>
                </div>
                <button
                  onClick={() => setGroup(tools, !allEnabled)}
                  className="text-[9px] px-1.5 py-0.5 rounded cursor-pointer border-none"
                  style={{ background: "rgba(63,63,70,0.3)", color: "var(--text-muted)" }}
                >
                  {allEnabled ? "Disable all" : "Enable all"}
                </button>
              </div>
              <div
                className="flex flex-col rounded-lg overflow-hidden"
                style={{ border: "1px solid rgba(63,63,70,0.3)" }}
              >
                {tools.map((tool, i) => {
                  const enabled = enabledTools.has(tool);
                  return (
                    <div
                      key={tool}
                      className="flex items-center justify-between px-2.5 py-1.5"
                      style={{
                        background: "rgba(24,24,27,0.5)",
                        borderTop: i > 0 ? "1px solid rgba(63,63,70,0.2)" : undefined,
                      }}
                    >
                      <span className="text-[11px] font-mono" style={{ color: enabled ? "var(--text-primary)" : "var(--text-muted)" }}>
                        {tool}
                      </span>
                      <button
                        onClick={() => toggle(tool)}
                        className="relative w-7 h-[16px] rounded-full transition-colors"
                        style={{
                          background: enabled ? "rgba(34,197,94,0.35)" : "rgba(63,63,70,0.4)",
                          border: "none",
                          cursor: "pointer",
                          padding: 0,
                        }}
                      >
                        <span
                          className="absolute top-[2px] w-[12px] h-[12px] rounded-full transition-all"
                          style={{
                            background: enabled ? "#22c55e" : "#52525b",
                            left: enabled ? 12 : 2,
                          }}
                        />
                      </button>
                    </div>
                  );
                })}
              </div>
            </div>
          );
        })}

        {/* Plugin-provided tools (toggleable) */}
        {pluginTools.length > 0 && (() => {
          const groupEnabled = pluginTools.filter((t) => enabledTools.has(t)).length;
          const allEnabled = groupEnabled === pluginTools.length;
          const allDisabled = groupEnabled === 0;
          return (
            <div>
              <div className="flex items-center justify-between mb-1.5">
                <div className="flex items-center gap-1.5">
                  <span className="text-[10px] font-semibold" style={{ color: "var(--text-secondary)" }}>
                    Plugin Tools
                  </span>
                  <span
                    className="text-[9px] px-1 py-px rounded-full"
                    style={{
                      background: allDisabled ? "rgba(239,68,68,0.1)" : "rgba(63,63,70,0.4)",
                      color: allDisabled ? "#f87171" : "var(--text-muted)",
                    }}
                  >
                    {groupEnabled}/{pluginTools.length}
                  </span>
                </div>
                <button
                  onClick={() => setGroup(pluginTools, !allEnabled)}
                  className="text-[9px] px-1.5 py-0.5 rounded cursor-pointer border-none"
                  style={{ background: "rgba(63,63,70,0.3)", color: "var(--text-muted)" }}
                >
                  {allEnabled ? "Disable all" : "Enable all"}
                </button>
              </div>
              <div
                className="flex flex-col rounded-lg overflow-hidden"
                style={{ border: "1px solid rgba(63,63,70,0.3)" }}
              >
                {pluginTools.map((tool, i) => {
                  const enabled = enabledTools.has(tool);
                  return (
                    <div
                      key={tool}
                      className="flex items-center justify-between px-2.5 py-1.5"
                      style={{
                        background: "rgba(24,24,27,0.5)",
                        borderTop: i > 0 ? "1px solid rgba(63,63,70,0.2)" : undefined,
                      }}
                    >
                      <span className="text-[11px] font-mono" style={{ color: enabled ? "var(--text-primary)" : "var(--text-muted)" }}>
                        {tool}
                      </span>
                      <button
                        onClick={() => toggle(tool)}
                        className="relative w-7 h-[16px] rounded-full transition-colors"
                        style={{
                          background: enabled ? "rgba(34,197,94,0.35)" : "rgba(63,63,70,0.4)",
                          border: "none",
                          cursor: "pointer",
                          padding: 0,
                        }}
                      >
                        <span
                          className="absolute top-[2px] w-[12px] h-[12px] rounded-full transition-all"
                          style={{
                            background: enabled ? "#22c55e" : "#52525b",
                            left: enabled ? 12 : 2,
                          }}
                        />
                      </button>
                    </div>
                  );
                })}
              </div>
            </div>
          );
        })()}
      </div>
    </Section>
  );
}

// ===========================================================================
// Section wrapper
// ===========================================================================

function Section({ icon, title, action, children }: {
  icon: React.ReactNode; title: string; action?: React.ReactNode; children: React.ReactNode;
}) {
  return (
    <div className="mb-3 p-3 rounded-lg" style={{ background: "rgba(20,20,20,0.9)", border: "1px solid var(--border)" }}>
      <div className="flex items-center justify-between mb-2.5">
        <div className="flex items-center gap-1.5">
          <span style={{ color: "var(--text-muted)" }}>{icon}</span>
          <span className="text-[10px] font-semibold uppercase tracking-wider" style={{ color: "var(--text-muted)" }}>
            {title}
          </span>
        </div>
        {action}
      </div>
      {children}
    </div>
  );
}

// ===========================================================================
// Action button
// ===========================================================================

function ActionBtn({ icon, label, onClick, disabled, variant, small }: {
  icon: React.ReactNode; label: string; onClick: () => void;
  disabled?: boolean; variant?: "secondary" | "danger"; small?: boolean;
}) {
  const base: React.CSSProperties = variant === "danger"
    ? { background: "rgba(220,38,38,0.12)", color: "#fb7185", border: "1px solid rgba(251,113,133,0.15)" }
    : variant === "secondary"
      ? { background: "var(--bg-tertiary)", color: "var(--text-primary)", border: "1px solid var(--border)" }
      : { background: "#fafafa", color: "#09090b" };
  return (
    <button
      onClick={onClick}
      disabled={disabled}
      className={`inline-flex items-center gap-1 ${small ? "px-1.5 py-0.5 text-[10px]" : "px-2.5 py-1.5 text-xs"} rounded-lg font-medium cursor-pointer border-none`}
      style={{ ...base, opacity: disabled ? 0.4 : 1, pointerEvents: disabled ? "none" : "auto" }}
    >
      {icon}
      {label}
    </button>
  );
}

// ===========================================================================
// Config Field
// ===========================================================================

const SENSITIVE_KEYS = /token|key|secret|password|credential|api.?key|access.?token|refresh.?token|client.?id|client.?secret/i;

function ConfigField({ fieldKey, value, onSave }: { fieldKey: string; value: unknown; onSave: (v: string) => void }) {
  const displayValue = typeof value === "string" ? value : JSON.stringify(value);
  const [localValue, setLocalValue] = useState(displayValue);
  const [dirty, setDirty] = useState(false);
  const isSensitive = SENSITIVE_KEYS.test(fieldKey);
  const [visible, setVisible] = useState(!isSensitive);

  useEffect(() => {
    const next = typeof value === "string" ? value : JSON.stringify(value);
    setLocalValue(next);
    setDirty(false);
  }, [value]);

  return (
    <div className="flex items-center gap-2">
      <span className="text-[10px] font-mono shrink-0 px-2 py-1 rounded truncate" style={{
        color: "var(--text-secondary)", background: "var(--bg-tertiary)", maxWidth: 120,
      }} title={fieldKey}>{fieldKey}</span>
      <input
        type={visible ? "text" : "password"}
        value={localValue}
        onChange={(e) => { setLocalValue(e.target.value); setDirty(e.target.value !== displayValue); }}
        onBlur={() => { if (dirty) onSave(localValue); }}
        onKeyDown={(e) => { if (e.key === "Enter" && dirty) onSave(localValue); }}
        className="flex-1 px-2 py-1 rounded-lg text-xs outline-none"
        style={{ background: "var(--bg-tertiary)", border: "1px solid var(--border)", color: "var(--text-primary)" }}
      />
      {isSensitive && (
        <button
          type="button"
          onClick={() => setVisible(!visible)}
          className="shrink-0 p-1 rounded cursor-pointer border-none bg-transparent"
          style={{ color: "var(--text-muted)" }}
          title={visible ? "Hide" : "Show"}
        >
          {visible ? <EyeOff size={12} /> : <Eye size={12} />}
        </button>
      )}
    </div>
  );
}

// ===========================================================================
// WhatsApp Config
// ===========================================================================

const WHATSAPP_MANAGED_KEYS = ["bridgeUrl", "prefix", "whitelist", "allowedGroups", "transcriptionBackend", "whisperUrl"];

const COUNTRY_CODES: { value: string; label: string; code: string }[] = [
  { value: "54", label: "Argentina (+54)", code: "+54" },
  { value: "55", label: "Brazil (+55)", code: "+55" },
  { value: "56", label: "Chile (+56)", code: "+56" },
  { value: "86", label: "China (+86)", code: "+86" },
  { value: "57", label: "Colombia (+57)", code: "+57" },
  { value: "593", label: "Ecuador (+593)", code: "+593" },
  { value: "33", label: "France (+33)", code: "+33" },
  { value: "49", label: "Germany (+49)", code: "+49" },
  { value: "91", label: "India (+91)", code: "+91" },
  { value: "39", label: "Italy (+39)", code: "+39" },
  { value: "81", label: "Japan (+81)", code: "+81" },
  { value: "52", label: "Mexico (+52)", code: "+52" },
  { value: "51", label: "Peru (+51)", code: "+51" },
  { value: "82", label: "South Korea (+82)", code: "+82" },
  { value: "34", label: "Spain (+34)", code: "+34" },
  { value: "44", label: "United Kingdom (+44)", code: "+44" },
  { value: "1", label: "United States (+1)", code: "+1" },
  { value: "58", label: "Venezuela (+58)", code: "+58" },
];

const COUNTRY_CODE_PREFIXES = COUNTRY_CODES
  .map((c) => ({ prefix: c.value, display: c.code }))
  .sort((a, b) => b.prefix.length - a.prefix.length);

function formatPhoneNumber(num: string): string {
  for (const { prefix, display } of COUNTRY_CODE_PREFIXES) {
    if (num.startsWith(prefix)) return `${display} ${num.slice(prefix.length)}`;
  }
  return `+${num}`;
}

function WhatsAppConfig({ pluginName, config, running, onConfigSave }: {
  pluginName: string; config: Record<string, unknown>; running: boolean;
  onConfigSave: (pluginName: string, key: string, value: string) => void;
}) {
  const { toast } = useToast();
  const bridgeUrl = (config.bridgeUrl as string) ?? "";
  const [localBridgeUrl, setLocalBridgeUrl] = useState(bridgeUrl);
  useEffect(() => setLocalBridgeUrl((config.bridgeUrl as string) ?? ""), [config.bridgeUrl]);

  const prefixValue = config.prefix;
  const prefixStr = prefixValue === null || prefixValue === undefined ? "" : String(prefixValue);
  const [localPrefix, setLocalPrefix] = useState(prefixStr);
  useEffect(() => {
    const v = config.prefix;
    setLocalPrefix(v === null || v === undefined ? "" : String(v));
  }, [config.prefix]);

  const [groups, setGroups] = useState<WhatsAppGroup[]>([]);
  const [groupsLoading, setGroupsLoading] = useState(false);
  const [groupsFetched, setGroupsFetched] = useState(false);
  const allowedGroups: string[] = Array.isArray(config.allowedGroups) ? (config.allowedGroups as string[]) : [];

  const whitelist: string[] = Array.isArray(config.whitelist) ? (config.whitelist as string[]) : [];
  const [showAddPhone, setShowAddPhone] = useState(false);
  const [phoneCountry, setPhoneCountry] = useState("54");
  const [phoneNumber, setPhoneNumber] = useState("");

  const transcriptionBackend = (config.transcriptionBackend as string) ?? "openai";
  const whisperUrl = (config.whisperUrl as string) ?? "http://localhost:8787";
  const [localWhisperUrl, setLocalWhisperUrl] = useState(whisperUrl);
  useEffect(() => setLocalWhisperUrl((config.whisperUrl as string) ?? "http://localhost:8787"), [config.whisperUrl]);

  const saveField = (key: string, value: unknown) => onConfigSave(pluginName, key, JSON.stringify(value));

  const handleFetchGroups = async () => {
    setGroupsLoading(true);
    try {
      const result = await fetchWhatsappGroups(pluginName);
      setGroups(result);
      setGroupsFetched(true);
      if (result.length === 0) toast("info", "No groups found.");
    } catch (e) {
      toast("error", `Failed to fetch groups: ${e}`);
    } finally {
      setGroupsLoading(false);
    }
  };

  const handleToggleGroup = (groupId: string) => {
    const next = allowedGroups.includes(groupId)
      ? allowedGroups.filter((g) => g !== groupId)
      : [...allowedGroups, groupId];
    saveField("allowedGroups", next);
  };

  const handleAddPhone = () => {
    const digits = phoneNumber.replace(/\D/g, "");
    if (!digits) return;
    const full = phoneCountry + digits;
    if (whitelist.includes(full)) { toast("info", "Already in whitelist"); return; }
    saveField("whitelist", [...whitelist, full]);
    setPhoneNumber("");
    setShowAddPhone(false);
  };

  const handleRemovePhone = (num: string) => saveField("whitelist", whitelist.filter((n) => n !== num));

  const remainingKeys = Object.keys(config).filter((k) => !WHATSAPP_MANAGED_KEYS.includes(k));

  return (
    <>
      {/* Bridge URL */}
      <Section icon={<Settings2 size={12} />} title="Bridge URL">
        <input
          value={localBridgeUrl}
          placeholder="http://localhost:3000"
          onChange={(e) => setLocalBridgeUrl(e.target.value)}
          onBlur={() => { if (localBridgeUrl !== bridgeUrl) saveField("bridgeUrl", localBridgeUrl); }}
          onKeyDown={(e) => { if (e.key === "Enter" && localBridgeUrl !== bridgeUrl) saveField("bridgeUrl", localBridgeUrl); }}
          className="w-full px-2.5 py-1.5 rounded-lg text-xs outline-none"
          style={{ background: "var(--bg-tertiary)", border: "1px solid var(--border)", color: "var(--text-primary)" }}
        />
      </Section>

      {/* Prefix */}
      <Section icon={<Settings2 size={12} />} title="Prefix">
        <input
          value={localPrefix}
          placeholder="No prefix (all messages)"
          onChange={(e) => setLocalPrefix(e.target.value)}
          onBlur={() => {
            const next = localPrefix.trim() || null;
            const prev = config.prefix === undefined ? null : config.prefix;
            if (next !== prev) saveField("prefix", next);
          }}
          onKeyDown={(e) => {
            if (e.key === "Enter") {
              const next = localPrefix.trim() || null;
              const prev = config.prefix === undefined ? null : config.prefix;
              if (next !== prev) saveField("prefix", next);
            }
          }}
          className="w-full px-2.5 py-1.5 rounded-lg text-xs outline-none"
          style={{ background: "var(--bg-tertiary)", border: "1px solid var(--border)", color: "var(--text-primary)" }}
        />
        <p className="text-[10px] mt-1.5" style={{ color: "var(--text-muted)" }}>
          Only messages starting with this prefix will be processed.
        </p>
      </Section>

      {/* Transcription */}
      <Section icon={<Settings2 size={12} />} title="Audio Transcription">
        <select
          value={transcriptionBackend}
          onChange={(e) => saveField("transcriptionBackend", e.target.value)}
          className="w-full px-2.5 py-1.5 rounded-lg text-xs outline-none appearance-none"
          style={{
            background: "var(--bg-tertiary)", border: "1px solid var(--border)", color: "var(--text-primary)",
            backgroundImage: `url("data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' width='12' height='12' viewBox='0 0 24 24' fill='none' stroke='%2352525b' stroke-width='2'%3E%3Cpath d='m6 9 6 6 6-6'/%3E%3C/svg%3E")`,
            backgroundRepeat: "no-repeat", backgroundPosition: "right 8px center", paddingRight: "2rem",
          }}
        >
          <option value="openai">OpenAI API</option>
          <option value="local">Local (whisper.cpp)</option>
        </select>
        {transcriptionBackend === "local" && (
          <div className="mt-2">
            <input
              value={localWhisperUrl}
              placeholder="http://localhost:8787"
              onChange={(e) => setLocalWhisperUrl(e.target.value)}
              onBlur={() => { if (localWhisperUrl !== whisperUrl) saveField("whisperUrl", localWhisperUrl); }}
              onKeyDown={(e) => { if (e.key === "Enter" && localWhisperUrl !== whisperUrl) saveField("whisperUrl", localWhisperUrl); }}
              className="w-full px-2.5 py-1.5 rounded-lg text-xs outline-none"
              style={{ background: "var(--bg-tertiary)", border: "1px solid var(--border)", color: "var(--text-primary)" }}
            />
          </div>
        )}
      </Section>

      {/* Allowed Groups */}
      <Section
        icon={<Users size={12} />}
        title={`Groups${allowedGroups.length ? ` (${allowedGroups.length})` : ""}`}
        action={
          <button
            onClick={handleFetchGroups}
            disabled={!running || groupsLoading}
            className="inline-flex items-center gap-1 text-[10px] cursor-pointer border-none bg-transparent"
            style={{ color: "var(--text-muted)", opacity: !running || groupsLoading ? 0.4 : 1 }}
          >
            {groupsLoading ? <Loader2 size={10} className="animate-spin" /> : <RefreshCw size={10} />}
            Fetch
          </button>
        }
      >
        {!running && !groupsFetched && (
          <p className="text-[10px]" style={{ color: "var(--text-muted)" }}>Start plugin to fetch groups.</p>
        )}
        {groups.length > 0 && (
          <div className="flex flex-col gap-px rounded-lg overflow-auto" style={{
            maxHeight: 180, border: "1px solid var(--border)",
          }}>
            {groups.map((group) => (
              <label
                key={group.id}
                className="flex items-center gap-2 px-2.5 py-1.5 cursor-pointer text-xs"
                style={{ background: "var(--bg-secondary)" }}
                onMouseEnter={(e) => { e.currentTarget.style.background = "var(--bg-hover)"; }}
                onMouseLeave={(e) => { e.currentTarget.style.background = "var(--bg-secondary)"; }}
              >
                <input
                  type="checkbox"
                  checked={allowedGroups.includes(group.id)}
                  onChange={() => handleToggleGroup(group.id)}
                  style={{ accentColor: "#a1a1aa" }}
                />
                <span className="truncate" style={{ color: "var(--text-primary)" }}>{group.subject}</span>
              </label>
            ))}
          </div>
        )}
      </Section>

      {/* Whitelist */}
      <Section
        icon={<Phone size={12} />}
        title={`Whitelist${whitelist.length ? ` (${whitelist.length})` : ""}`}
        action={
          !showAddPhone ? (
            <button
              onClick={() => setShowAddPhone(true)}
              className="inline-flex items-center gap-1 text-[10px] cursor-pointer border-none bg-transparent"
              style={{ color: "var(--text-muted)" }}
            >
              <Plus size={10} />
              Add
            </button>
          ) : undefined
        }
      >
        {whitelist.length > 0 && (
          <div className="flex flex-wrap gap-1 mb-2">
            {whitelist.map((num) => (
              <span key={num} className="inline-flex items-center gap-1 px-2 py-0.5 rounded text-[10px] font-mono" style={{
                background: "var(--bg-tertiary)", border: "1px solid var(--border)", color: "var(--text-primary)",
              }}>
                {formatPhoneNumber(num)}
                <button
                  className="inline-flex border-none bg-transparent cursor-pointer p-0"
                  style={{ color: "var(--text-muted)" }}
                  onClick={() => handleRemovePhone(num)}
                >
                  <X size={10} />
                </button>
              </span>
            ))}
          </div>
        )}
        {whitelist.length === 0 && !showAddPhone && (
          <p className="text-[10px]" style={{ color: "var(--text-muted)" }}>No numbers. All allowed.</p>
        )}
        {showAddPhone && (
          <div className="flex flex-wrap gap-1.5 p-2 rounded-lg" style={{ background: "var(--bg-base)", border: "1px solid var(--border)" }}>
            <select
              value={phoneCountry}
              onChange={(e) => setPhoneCountry(e.target.value)}
              className="px-2 py-1 rounded text-[10px] outline-none appearance-none"
              style={{
                background: "var(--bg-tertiary)", border: "1px solid var(--border)", color: "var(--text-primary)",
                width: "100%",
                backgroundImage: `url("data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' width='12' height='12' viewBox='0 0 24 24' fill='none' stroke='%2352525b' stroke-width='2'%3E%3Cpath d='m6 9 6 6 6-6'/%3E%3C/svg%3E")`,
                backgroundRepeat: "no-repeat", backgroundPosition: "right 8px center", paddingRight: "2rem",
              }}
            >
              {COUNTRY_CODES.map((c) => <option key={c.value} value={c.value}>{c.label}</option>)}
            </select>
            <div className="flex gap-1.5 w-full">
              <input
                value={phoneNumber}
                placeholder="Phone number"
                onChange={(e) => setPhoneNumber(e.target.value.replace(/\D/g, ""))}
                onKeyDown={(e) => { if (e.key === "Enter") handleAddPhone(); if (e.key === "Escape") setShowAddPhone(false); }}
                className="flex-1 px-2 py-1 rounded text-[10px] outline-none"
                style={{ background: "var(--bg-tertiary)", border: "1px solid var(--border)", color: "var(--text-primary)" }}
              />
              <button
                onClick={handleAddPhone}
                disabled={!phoneNumber.trim()}
                className="inline-flex items-center gap-0.5 px-2 py-1 rounded text-[10px] font-medium cursor-pointer border-none"
                style={{ background: "#fafafa", color: "#09090b", opacity: !phoneNumber.trim() ? 0.4 : 1 }}
              >
                <Plus size={10} /> Add
              </button>
              <button
                onClick={() => { setShowAddPhone(false); setPhoneNumber(""); }}
                className="px-2 py-1 rounded text-[10px] cursor-pointer border-none"
                style={{ background: "transparent", color: "var(--text-muted)" }}
              >
                Cancel
              </button>
            </div>
          </div>
        )}
      </Section>

      {/* Remaining config */}
      {remainingKeys.length > 0 && (
        <Section icon={<Settings2 size={12} />} title="Other Settings">
          <div className="flex flex-col gap-2">
            {remainingKeys.map((key) => (
              <ConfigField key={key} fieldKey={key} value={config[key]} onSave={(v) => onConfigSave(pluginName, key, v)} />
            ))}
          </div>
        </Section>
      )}
    </>
  );
}

// ===========================================================================
// QR Modal
// ===========================================================================

function QrModal({ qrDataUrl, qrLoading, qrLinked, onClose, onDone }: {
  qrDataUrl: string | null; qrLoading: boolean; qrLinked: boolean;
  onClose: () => void; onDone: () => void;
}) {
  useEffect(() => {
    const handler = (e: KeyboardEvent) => { if (e.key === "Escape") onClose(); };
    window.addEventListener("keydown", handler);
    return () => window.removeEventListener("keydown", handler);
  }, [onClose]);

  return (
    <div
      className="fixed inset-0 flex items-center justify-center z-50"
      style={{ background: "rgba(0,0,0,0.7)", backdropFilter: "blur(6px)" }}
      onClick={onClose}
    >
      <div
        className="rounded-xl p-5 flex flex-col items-center gap-3"
        style={{ background: "rgba(20,20,20,0.98)", border: "1px solid var(--border)", maxWidth: 300 }}
        onClick={(e) => e.stopPropagation()}
      >
        {qrLinked ? (
          <>
            <div className="flex items-center justify-center" style={{
              width: 56, height: 56, borderRadius: "50%",
              background: "rgba(74,222,128,0.15)", border: "2px solid rgba(74,222,128,0.4)",
            }}>
              <Check size={28} style={{ color: "#4ade80" }} />
            </div>
            <span className="text-sm font-semibold" style={{ color: "var(--text-primary)" }}>Linked!</span>
            <p className="text-[11px] text-center" style={{ color: "var(--text-secondary)" }}>
              WhatsApp connected successfully.
            </p>
            <button onClick={onDone} className="px-3 py-1.5 rounded-lg text-xs font-medium cursor-pointer border-none"
              style={{ background: "#fafafa", color: "#09090b" }}>Done</button>
          </>
        ) : (
          <>
            <div className="flex items-center gap-1.5">
              <QrCode size={14} style={{ color: "var(--text-secondary)" }} />
              <span className="text-xs font-semibold" style={{ color: "var(--text-primary)" }}>Scan QR Code</span>
            </div>
            <p className="text-[10px] text-center" style={{ color: "var(--text-muted)" }}>
              Open WhatsApp &gt; Settings &gt; Linked Devices
            </p>
            {qrLoading ? (
              <div className="flex items-center justify-center" style={{ width: 220, height: 220 }}>
                <Loader2 size={24} className="animate-spin" style={{ color: "var(--text-muted)" }} />
              </div>
            ) : qrDataUrl ? (
              <img src={qrDataUrl} alt="QR" style={{ width: 220, height: 220, borderRadius: 8, imageRendering: "pixelated" }} />
            ) : (
              <div className="flex items-center justify-center text-[10px] text-center" style={{
                width: 220, height: 140, color: "var(--text-muted)", background: "var(--bg-tertiary)",
                borderRadius: 8, border: "1px dashed var(--border)", padding: 16,
              }}>
                No QR available. Is the plugin running?
              </div>
            )}
            <button onClick={onClose} className="px-3 py-1.5 rounded-lg text-xs cursor-pointer"
              style={{ background: "var(--bg-tertiary)", color: "var(--text-primary)", border: "1px solid var(--border)" }}>
              Close
            </button>
          </>
        )}
      </div>
    </div>
  );
}

// ===========================================================================
// Registry Modal
// ===========================================================================

function RegistryModal({ remotePlugins, loading, installingRemote, onInstall, onClose }: {
  remotePlugins: RemotePlugin[]; loading: boolean; installingRemote: string | null;
  onInstall: (name: string) => void; onClose: () => void;
}) {
  useEffect(() => {
    const handler = (e: KeyboardEvent) => { if (e.key === "Escape") onClose(); };
    window.addEventListener("keydown", handler);
    return () => window.removeEventListener("keydown", handler);
  }, [onClose]);

  return (
    <div
      className="fixed inset-0 flex items-center justify-center z-40"
      style={{ background: "rgba(0,0,0,0.7)", backdropFilter: "blur(6px)" }}
      onClick={onClose}
    >
      <div
        className="rounded-xl p-4 flex flex-col"
        style={{
          background: "rgba(20,20,20,0.98)", border: "1px solid var(--border)",
          width: "min(440px, 90vw)", maxHeight: "70vh",
        }}
        onClick={(e) => e.stopPropagation()}
      >
        <div className="flex items-center justify-between mb-3 shrink-0">
          <div className="flex items-center gap-2">
            <span className="text-sm font-semibold" style={{ color: "var(--text-primary)" }}>
              Plugin Registry
            </span>
            {!loading && (
              <span className="text-[10px] px-1.5 py-0.5 rounded" style={{
                background: "rgba(255,255,255,0.05)", color: "var(--text-muted)",
              }}>{remotePlugins.length}</span>
            )}
          </div>
          <button
            onClick={onClose}
            className="p-1 rounded cursor-pointer border-none"
            style={{ background: "transparent", color: "var(--text-muted)" }}
          >
            <X size={14} />
          </button>
        </div>

        <div className="overflow-y-auto flex-1">
          {loading ? (
            <div className="flex items-center justify-center gap-2 py-8" style={{ color: "var(--text-muted)" }}>
              <Loader2 size={14} className="animate-spin" />
              <span className="text-xs">Loading...</span>
            </div>
          ) : remotePlugins.length === 0 ? (
            <div className="py-8 text-center text-xs" style={{ color: "var(--text-muted)" }}>
              No plugins available.
            </div>
          ) : (
            <div className="flex flex-col gap-1">
              {remotePlugins.map((rp) => (
                <div
                  key={rp.name}
                  className="flex items-center justify-between px-3 py-2.5 rounded-lg"
                  style={{ background: "var(--bg-secondary)", transition: "background 120ms" }}
                  onMouseEnter={(e) => { e.currentTarget.style.background = "var(--bg-hover)"; }}
                  onMouseLeave={(e) => { e.currentTarget.style.background = "var(--bg-secondary)"; }}
                >
                  <div className="min-w-0 flex-1 mr-2">
                    <div className="flex items-center gap-1.5">
                      <span className="text-xs font-medium" style={{ color: "var(--text-primary)" }}>{rp.name}</span>
                      <span className="text-[10px] font-mono" style={{ color: "var(--text-muted)" }}>v{rp.version}</span>
                      {rp.installed && <Check size={10} style={{ color: "#4ade80" }} />}
                    </div>
                    {rp.description && (
                      <p className="text-[10px] truncate" style={{ color: "var(--text-muted)" }}>{rp.description}</p>
                    )}
                  </div>
                  {!rp.installed && rp.available && (
                    <button
                      onClick={() => onInstall(rp.name)}
                      disabled={installingRemote !== null}
                      className="inline-flex items-center gap-1 px-2 py-1 rounded text-[10px] font-medium cursor-pointer border-none shrink-0"
                      style={{
                        background: "#fafafa", color: "#09090b",
                        opacity: installingRemote !== null ? 0.4 : 1,
                        pointerEvents: installingRemote !== null ? "none" : "auto",
                      }}
                    >
                      {installingRemote === rp.name ? <Loader2 size={10} className="animate-spin" /> : <Download size={10} />}
                      Install
                    </button>
                  )}
                </div>
              ))}
            </div>
          )}
        </div>
      </div>
    </div>
  );
}
