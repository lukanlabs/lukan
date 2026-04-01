import { useState, useEffect, useCallback, useRef, useMemo } from "react";
import type {
  PluginInfo,
  RemotePlugin,
  PluginCommand,
  PluginToolsInfo,
  ConfigFieldSchemaDto,
  AuthDeclarationDto,
} from "../../lib/types";
import {
  listPlugins,
  installPlugin,
  installRemotePlugin,
  listRemotePlugins,
  removePlugin,
  startPlugin,
  stopPlugin,
  restartPlugin,
  getPluginConfig,
  setPluginConfigField,
  getPluginLogs,
  getPluginAuthQr,
  checkPluginAuth,
  getPluginManifestInfo,
  getPluginCommands,
  runPluginCommand,
  getPluginManifestTools,
  listTools,
} from "../../lib/tauri";
import { COUNTRY_CODES, formatPhoneNumber } from "../../lib/phone-utils";
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
  Search,
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

const GROUP_ORDER = [
  "File ops",
  "Search",
  "Execution",
  "Web",
  "Tasks",
  "Skills",
  "Planner",
];

// ---------------------------------------------------------------------------
// Main Component
// ---------------------------------------------------------------------------

export default function PluginsTab() {
  const { toast } = useToast();

  const [plugins, setPlugins] = useState<PluginInfo[]>([]);
  const [selectedPlugin, setSelectedPlugin] = useState<string | null>(null);
  const [mainTab, setMainTab] = useState<"installed" | "addons">("installed");
  const [remotePlugins, setRemotePlugins] = useState<RemotePlugin[]>([]);
  const [addonsLoading, setAddonsLoading] = useState(false);
  const [installingRemote, setInstallingRemote] = useState<string | null>(null);
  const [addonsSearch, setAddonsSearch] = useState("");

  const [localPath, setLocalPath] = useState("");
  const [installingLocal, setInstallingLocal] = useState(false);

  const [actionLoading, setActionLoading] = useState<
    "start" | "stop" | "restart" | "remove" | "update" | null
  >(null);
  const [pluginConfig, setPluginConfig] = useState<Record<string, unknown>>({});
  const [pluginLogs, setPluginLogs] = useState("");
  const [logsRefreshing, setLogsRefreshing] = useState(false);
  const [pluginCommands, setPluginCommands] = useState<PluginCommand[]>([]);
  const [commandRunning, setCommandRunning] = useState<string | null>(null);
  const [commandOutput, setCommandOutput] = useState<string | null>(null);

  const [pluginAuthed, setPluginAuthed] = useState(false);
  const [configSchema, setConfigSchema] = useState<Record<
    string,
    ConfigFieldSchemaDto
  > | null>(null);
  const [authDeclaration, setAuthDeclaration] =
    useState<AuthDeclarationDto | null>(null);

  const [showQr, setShowQr] = useState(false);
  const [qrPluginName, setQrPluginName] = useState<string | null>(null);
  const [qrDataUrl, setQrDataUrl] = useState<string | null>(null);
  const [qrLoading, setQrLoading] = useState(false);
  const [qrLinked, setQrLinked] = useState(false);

  const lastQrRef = useRef<string | null>(null);
  useEffect(() => {
    if (!showQr || qrLinked || !qrPluginName) return;
    let cancelled = false;
    const poll = async () => {
      try {
        const qr = await getPluginAuthQr(qrPluginName);
        if (cancelled) return;
        if (!qr && qrDataUrl) {
          setQrLinked(true);
          toast("success", "Linked successfully!");
        } else if (qr && qr !== lastQrRef.current) {
          lastQrRef.current = qr;
          const url = await QRCode.toDataURL(qr, {
            width: 256,
            margin: 2,
            color: { dark: "#000000", light: "#ffffff" },
          });
          if (!cancelled) {
            setQrDataUrl(url);
            setQrLoading(false);
          }
        }
      } catch {}
    };
    poll();
    const interval = setInterval(poll, 2000);
    return () => {
      cancelled = true;
      clearInterval(interval);
    };
  }, [showQr, qrDataUrl, qrLinked, qrPluginName, toast]);

  // ── Data fetching ──

  const refreshPlugins = useCallback(async () => {
    try {
      const list = await listPlugins();
      setPlugins(list);
    } catch (e) {
      toast("error", `Failed to list plugins: ${e}`);
    }
  }, [toast]);

  useEffect(() => {
    refreshPlugins();
  }, [refreshPlugins]);

  const loadPluginDetail = useCallback(
    async (name: string) => {
      try {
        const [cfg, logs, cmds, manifestInfo] = await Promise.all([
          getPluginConfig(name),
          getPluginLogs(name, 200),
          getPluginCommands(name).catch(() => [] as PluginCommand[]),
          getPluginManifestInfo(name).catch(() => null),
        ]);
        setPluginConfig(cfg);
        setPluginLogs(logs);
        setPluginCommands(cmds);
        setConfigSchema(manifestInfo?.config ?? null);
        setAuthDeclaration(manifestInfo?.auth ?? null);
        const authed = await checkPluginAuth(name).catch(() => false);
        setPluginAuthed(authed);
      } catch (e) {
        toast("error", `Failed to load plugin details: ${e}`);
      }
    },
    [toast],
  );

  const selectPlugin = useCallback(
    (name: string) => {
      setSelectedPlugin(name);
      loadPluginDetail(name);
    },
    [loadPluginDetail],
  );

  const goBack = useCallback(() => {
    setSelectedPlugin(null);
    setPluginConfig({});
    setPluginLogs("");
    setActionLoading(null);
    setPluginCommands([]);
    setCommandRunning(null);
    setCommandOutput(null);
    setPluginAuthed(false);
    setConfigSchema(null);
    setAuthDeclaration(null);
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

  const handleAction = async (
    action: "start" | "stop" | "restart" | "remove" | "update",
    name: string,
  ) => {
    setActionLoading(action);
    try {
      if (action === "start") await startPlugin(name);
      else if (action === "stop") await stopPlugin(name);
      else if (action === "restart") await restartPlugin(name);
      else if (action === "remove") {
        await removePlugin(name);
        await refreshPlugins();
        goBack();
        return;
      } else if (action === "update") {
        const wasRunning = plugins.find((p) => p.name === name)?.running;
        if (wasRunning) await stopPlugin(name);
        await installRemotePlugin(name);
        if (wasRunning) await startPlugin(name);
      }
      toast(
        "success",
        `Plugin '${name}' ${action === "restart" ? "restarted" : action + (action === "update" ? "d" : "ed")}`,
      );
      await refreshPlugins();
      const logs = await getPluginLogs(name, 200);
      setPluginLogs(logs);
    } catch (e) {
      toast("error", `Failed to ${action} plugin: ${e}`);
    } finally {
      setActionLoading(null);
    }
  };

  const handleShowQr = (pluginName: string) => {
    setQrPluginName(pluginName);
    setShowQr(true);
    setQrLoading(true);
    setQrDataUrl(null);
    setQrLinked(false);
    lastQrRef.current = null;
  };

  const handleConfigSave = async (
    pluginName: string,
    key: string,
    value: string,
  ) => {
    try {
      const parsed = (() => {
        try {
          return JSON.parse(value);
        } catch {
          return value;
        }
      })();
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

  const loadAddons = async () => {
    setAddonsLoading(true);
    try {
      const remote = await listRemotePlugins();
      setRemotePlugins(remote);
    } catch (e) {
      toast("error", `Failed to load add-ons: ${e}`);
    } finally {
      setAddonsLoading(false);
    }
  };

  const handleInstallRemote = async (name: string) => {
    setInstallingRemote(name);
    try {
      await installRemotePlugin(name);
      toast("success", `${name} installed`);
      await refreshPlugins();
      await loadAddons();
    } catch (e) {
      toast("error", `Failed to install: ${e}`);
    } finally {
      setInstallingRemote(null);
    }
  };

  const activePlugin = selectedPlugin
    ? (plugins.find((p) => p.name === selectedPlugin) ?? null)
    : null;

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
          pluginAuthed={pluginAuthed}
          configSchema={configSchema}
          authDeclaration={authDeclaration}
          onBack={goBack}
          onAction={handleAction}
          onConfigSave={handleConfigSave}
          onRefreshLogs={handleRefreshLogs}
          onShowQr={handleShowQr}
          onRunCommand={handleRunCommand}
        />
      ) : (
        <>
          {/* Installed / Add-ons tabs */}
          <div style={{ display: "flex", gap: 2, marginBottom: 16 }}>
            <button
              className="s-btn"
              onClick={() => setMainTab("installed")}
              style={{
                background:
                  mainTab === "installed"
                    ? "rgba(255,255,255,0.08)"
                    : "transparent",
                color:
                  mainTab === "installed"
                    ? "var(--text-primary)"
                    : "var(--text-muted)",
                borderColor:
                  mainTab === "installed"
                    ? "rgba(255,255,255,0.12)"
                    : "rgba(255,255,255,0.06)",
              }}
            >
              Installed ({plugins.length})
            </button>
            <button
              className="s-btn"
              onClick={() => {
                setMainTab("addons");
                if (remotePlugins.length === 0) loadAddons();
              }}
              style={{
                background:
                  mainTab === "addons"
                    ? "rgba(255,255,255,0.08)"
                    : "transparent",
                color:
                  mainTab === "addons"
                    ? "var(--text-primary)"
                    : "var(--text-muted)",
                borderColor:
                  mainTab === "addons"
                    ? "rgba(255,255,255,0.12)"
                    : "rgba(255,255,255,0.06)",
              }}
            >
              Add-ons
            </button>
          </div>

          {mainTab === "installed" ? (
            <MasterView
              plugins={plugins}
              localPath={localPath}
              installingLocal={installingLocal}
              onLocalPathChange={setLocalPath}
              onInstallLocal={handleInstallLocal}
              onSelectPlugin={selectPlugin}
            />
          ) : (
            <AddonsView
              remotePlugins={remotePlugins}
              installedNames={new Set(plugins.map((p) => p.name))}
              loading={addonsLoading}
              installing={installingRemote}
              search={addonsSearch}
              onSearchChange={setAddonsSearch}
              onInstall={handleInstallRemote}
            />
          )}
        </>
      )}

      {/* QR Modal */}
      {showQr && (
        <QrModal
          qrDataUrl={qrDataUrl}
          qrLoading={qrLoading}
          qrLinked={qrLinked}
          onClose={() => setShowQr(false)}
          onDone={() => {
            setShowQr(false);
            refreshPlugins();
          }}
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
}

function MasterView({
  plugins,
  localPath,
  installingLocal,
  onLocalPathChange,
  onInstallLocal,
  onSelectPlugin,
}: MasterViewProps) {
  return (
    <>
      {/* Header */}
      <div
        style={{
          display: "flex",
          alignItems: "center",
          justifyContent: "space-between",
          marginBottom: 12,
        }}
      >
        <p style={{ fontSize: 12, color: "#71717a", margin: 0 }}>
          {plugins.length} installed
        </p>
      </div>

      {/* Install local */}
      <div className="flex gap-2 mb-4">
        <input
          value={localPath}
          placeholder="/path/to/plugin"
          onChange={(e) => onLocalPathChange(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Enter") onInstallLocal();
          }}
          className="flex-1 px-2.5 py-1.5 rounded-lg text-xs outline-none"
          style={{
            background: "var(--bg-tertiary)",
            border: "1px solid var(--border)",
            color: "var(--text-primary)",
          }}
        />
        <button
          onClick={onInstallLocal}
          disabled={installingLocal || !localPath.trim()}
          className="inline-flex items-center gap-1 px-2.5 py-1.5 rounded-lg text-xs font-medium cursor-pointer border-none shrink-0"
          style={{
            background: "#fafafa",
            color: "#09090b",
            opacity: installingLocal || !localPath.trim() ? 0.4 : 1,
            pointerEvents:
              installingLocal || !localPath.trim() ? "none" : "auto",
          }}
        >
          {installingLocal ? (
            <Loader2 size={11} className="animate-spin" />
          ) : (
            <FolderOpen size={11} />
          )}
          Install
        </button>
      </div>

      {/* Plugin list */}
      {plugins.length === 0 ? (
        <div
          className="text-center py-10 rounded-lg"
          style={{
            color: "var(--text-muted)",
            border: "1px dashed var(--border)",
          }}
        >
          <Package size={24} style={{ margin: "0 auto 8px", opacity: 0.3 }} />
          <p className="text-xs">No plugins installed</p>
        </div>
      ) : (
        <div className="flex flex-col gap-1">
          {plugins.map((plugin) => (
            <div
              key={plugin.name}
              className="flex items-center gap-3 px-3 py-2.5 rounded-lg cursor-pointer"
              style={{
                background: "transparent",
                transition: "background 120ms",
              }}
              onClick={() => onSelectPlugin(plugin.name)}
              onMouseEnter={(e) => {
                e.currentTarget.style.background = "var(--bg-hover)";
              }}
              onMouseLeave={(e) => {
                e.currentTarget.style.background = "transparent";
              }}
            >
              <Package size={13} style={{ color: "var(--text-muted)" }} />
              <div className="flex-1 min-w-0">
                <div className="flex items-center gap-2">
                  <span
                    className="text-xs font-medium"
                    style={{ color: "var(--text-primary)" }}
                  >
                    {plugin.name}
                  </span>
                  <span
                    className="text-[10px] font-mono"
                    style={{ color: "var(--text-muted)" }}
                  >
                    v{plugin.version}
                  </span>
                  {plugin.alias && (
                    <span
                      className="text-[10px] px-1 rounded"
                      style={{
                        background: "rgba(255,255,255,0.05)",
                        color: "var(--text-muted)",
                      }}
                    >
                      {plugin.alias}
                    </span>
                  )}
                </div>
                {plugin.description && (
                  <span
                    className="text-[10px] block truncate"
                    style={{ color: "var(--text-muted)" }}
                  >
                    {plugin.description}
                  </span>
                )}
              </div>
              <div className="flex items-center gap-2 shrink-0">
                {plugin.pluginType === "channel" ||
                  (plugin.activityBar && (
                    <span
                      className="w-1.5 h-1.5 rounded-full"
                      style={{
                        background: plugin.running
                          ? "#4ade80"
                          : "var(--text-muted)",
                        boxShadow: plugin.running
                          ? "0 0 6px rgba(74,222,128,0.4)"
                          : "none",
                      }}
                    />
                  ))}
                <ChevronRight
                  size={12}
                  style={{ color: "var(--text-muted)" }}
                />
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
  actionLoading: "start" | "stop" | "restart" | "remove" | "update" | null;
  logsRefreshing: boolean;
  commands: PluginCommand[];
  commandRunning: string | null;
  commandOutput: string | null;
  pluginAuthed: boolean;
  configSchema: Record<string, ConfigFieldSchemaDto> | null;
  authDeclaration: AuthDeclarationDto | null;
  onBack: () => void;
  onAction: (
    action: "start" | "stop" | "restart" | "remove" | "update",
    name: string,
  ) => void;
  onConfigSave: (pluginName: string, key: string, value: string) => void;
  onRefreshLogs: (name: string) => void;
  onShowQr: (pluginName: string) => void;
  onRunCommand: (pluginName: string, command: string) => void;
}

function DetailView({
  plugin,
  config,
  logs,
  actionLoading,
  logsRefreshing,
  commands,
  commandRunning,
  commandOutput,
  pluginAuthed,
  configSchema,
  authDeclaration,
  onBack,
  onAction,
  onConfigSave,
  onRefreshLogs,
  onShowQr,
  onRunCommand,
}: DetailViewProps) {
  const logsEndRef = useRef<HTMLPreElement>(null);
  useEffect(() => {
    if (logsEndRef.current)
      logsEndRef.current.scrollTop = logsEndRef.current.scrollHeight;
  }, [logs]);

  const configKeys = Object.keys(config);
  const hasQrAuth = authDeclaration?.type === "qr";

  return (
    <div style={{ animation: "fadeIn 0.15s ease-out" }}>
      {/* Header: back + plugin name + status + actions inline */}
      <div
        style={{
          display: "flex",
          alignItems: "center",
          gap: 8,
          marginBottom: 16,
        }}
      >
        <button
          onClick={onBack}
          style={{
            background: "transparent",
            border: "none",
            color: "var(--text-muted)",
            cursor: "pointer",
            padding: 2,
          }}
        >
          <ArrowLeft size={14} />
        </button>
        <span
          style={{
            fontSize: 14,
            fontWeight: 600,
            color: "var(--text-primary)",
          }}
        >
          {plugin.name}
        </span>
        <span
          style={{
            fontSize: 10,
            fontFamily: "var(--font-mono)",
            color: "var(--text-muted)",
          }}
        >
          v{plugin.version}
        </span>
        <span className="s-badge s-badge-gray">{plugin.pluginType}</span>
        {(plugin.pluginType === "channel" || plugin.activityBar) && (
          <span
            className={`s-badge ${plugin.running ? "s-badge-green" : "s-badge-gray"}`}
          >
            {plugin.running ? "Active" : "Inactive"}
          </span>
        )}
        <div style={{ flex: 1 }} />
        {/* Inline action buttons */}
        {(plugin.pluginType === "channel" || plugin.activityBar) &&
          (plugin.running ? (
            <>
              <button
                className="s-btn"
                onClick={() => onAction("stop", plugin.name)}
                disabled={actionLoading !== null}
                style={{ opacity: actionLoading !== null ? 0.4 : 1 }}
              >
                {actionLoading === "stop" ? (
                  <Loader2 size={10} className="animate-spin" />
                ) : (
                  <Square size={10} />
                )}
                Deactivate
              </button>
              <button
                className="s-btn"
                onClick={() => onAction("restart", plugin.name)}
                disabled={actionLoading !== null}
                style={{ opacity: actionLoading !== null ? 0.4 : 1 }}
              >
                {actionLoading === "restart" ? (
                  <Loader2 size={10} className="animate-spin" />
                ) : (
                  <RotateCw size={10} />
                )}
                Restart
              </button>
            </>
          ) : (
            <button
              className="s-btn"
              onClick={() => onAction("start", plugin.name)}
              disabled={actionLoading !== null}
              style={{ opacity: actionLoading !== null ? 0.4 : 1 }}
            >
              {actionLoading === "start" ? (
                <Loader2 size={10} className="animate-spin" />
              ) : (
                <Play size={10} />
              )}
              Activate
            </button>
          ))}
        <button
          className="s-btn"
          onClick={() => onAction("update", plugin.name)}
          disabled={actionLoading !== null}
          style={{ opacity: actionLoading !== null ? 0.4 : 1 }}
        >
          {actionLoading === "update" ? (
            <Loader2 size={10} className="animate-spin" />
          ) : (
            <Download size={10} />
          )}
          Update
        </button>
        <button
          className="s-btn"
          onClick={() => onAction("remove", plugin.name)}
          disabled={actionLoading !== null}
          style={{
            color: "#fb7185",
            borderColor: "rgba(251,113,133,0.2)",
            opacity: actionLoading !== null ? 0.4 : 1,
          }}
        >
          {actionLoading === "remove" ? (
            <Loader2 size={10} className="animate-spin" />
          ) : (
            <Trash2 size={10} />
          )}
          Uninstall
        </button>
      </div>

      {plugin.description && (
        <p
          style={{
            fontSize: 11.5,
            color: "var(--text-secondary)",
            marginBottom: 16,
            marginTop: -8,
          }}
        >
          {plugin.description}
        </p>
      )}

      {/* Commands */}
      {commands.length > 0 && (
        <div className="s-section">
          <div className="s-section-title">Commands</div>
          <div className="s-card">
            {commands.map((cmd) => {
              const isAuth = cmd.name === "auth";
              const btnLabel = isAuth
                ? commandRunning === cmd.name
                  ? "..."
                  : pluginAuthed
                    ? "Re-auth"
                    : "Authenticate"
                : commandRunning === cmd.name
                  ? "..."
                  : "Run";
              return (
                <div key={cmd.name} className="s-row">
                  <div style={{ minWidth: 0 }}>
                    <div className="s-row-label">
                      {cmd.name}
                      {isAuth && (
                        <span
                          style={{
                            marginLeft: 8,
                            fontSize: 10,
                            color: pluginAuthed
                              ? "#4ade80"
                              : "var(--text-muted)",
                          }}
                        >
                          {pluginAuthed ? "authenticated" : "not authenticated"}
                        </span>
                      )}
                    </div>
                    {cmd.description && (
                      <div className="s-row-hint">{cmd.description}</div>
                    )}
                  </div>
                  <button
                    className="s-btn"
                    onClick={() =>
                      isAuth && hasQrAuth
                        ? onShowQr(plugin.name)
                        : onRunCommand(plugin.name, cmd.name)
                    }
                    disabled={commandRunning !== null}
                    style={{ opacity: commandRunning !== null ? 0.4 : 1 }}
                  >
                    {commandRunning === cmd.name ? (
                      <Loader2 size={10} className="animate-spin" />
                    ) : (
                      <Play size={10} />
                    )}
                    {btnLabel}
                  </button>
                </div>
              );
            })}
          </div>
          {commandOutput && (
            <pre
              style={{
                fontSize: 10,
                padding: "8px 10px",
                marginTop: 6,
                borderRadius: 4,
                overflow: "auto",
                background: "rgba(0,0,0,0.2)",
                color: "var(--text-secondary)",
                maxHeight: 120,
                whiteSpace: "pre-wrap",
                wordBreak: "break-word",
                margin: "6px 0 0",
                fontFamily: "var(--font-mono)",
                border: "1px solid rgba(255,255,255,0.04)",
              }}
            >
              {commandOutput}
            </pre>
          )}
        </div>
      )}

      {/* QR Auth */}
      {hasQrAuth && (
        <div className="s-section">
          <div className="s-section-title">Authentication</div>
          <div className="s-card">
            <div className="s-row">
              <div className="s-row-label">Scan QR code to authenticate</div>
              <button className="s-btn" onClick={() => onShowQr(plugin.name)}>
                <QrCode size={11} /> View QR
              </button>
            </div>
          </div>
        </div>
      )}

      {/* Tools */}
      {(plugin.pluginType === "channel" || plugin.activityBar) && (
        <PluginToolsSection
          pluginName={plugin.name}
          config={config}
          onConfigSave={onConfigSave}
        />
      )}

      {/* Configuration */}
      {configKeys.length > 0 && (
        <GenericConfigForm
          pluginName={plugin.name}
          config={config}
          schema={configSchema}
          running={plugin.running}
          onConfigSave={onConfigSave}
        />
      )}

      {/* Logs */}
      {(plugin.pluginType === "channel" || plugin.activityBar) && (
        <div className="s-section">
          <div
            style={{
              display: "flex",
              alignItems: "center",
              justifyContent: "space-between",
              marginBottom: 8,
            }}
          >
            <div className="s-section-title" style={{ marginBottom: 0 }}>
              Logs
            </div>
            <button
              className="s-btn"
              onClick={() => onRefreshLogs(plugin.name)}
              disabled={logsRefreshing}
              style={{ opacity: logsRefreshing ? 0.4 : 1 }}
            >
              <RefreshCw
                size={10}
                className={logsRefreshing ? "animate-spin" : ""}
              />{" "}
              Refresh
            </button>
          </div>
          <pre
            ref={logsEndRef}
            style={{
              fontFamily: "var(--font-mono)",
              fontSize: 10,
              padding: "10px 12px",
              borderRadius: 4,
              minHeight: 100,
              maxHeight: 220,
              width: "100%",
              overflow: "auto",
              background: "rgba(0,0,0,0.2)",
              color: "var(--text-secondary)",
              border: "1px solid rgba(255,255,255,0.04)",
              lineHeight: 1.5,
              whiteSpace: "pre",
              margin: 0,
            }}
          >
            {logs || "No logs available"}
          </pre>
        </div>
      )}
    </div>
  );
}

// ===========================================================================
// Plugin Tools Section
// ===========================================================================

function PluginToolsSection({
  pluginName,
  config,
  onConfigSave,
}: {
  pluginName: string;
  config: Record<string, unknown>;
  onConfigSave: (pluginName: string, key: string, value: string) => void;
}) {
  const { toast } = useToast();
  const [manifestInfo, setManifestInfo] = useState<PluginToolsInfo | null>(
    null,
  );
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
        setPluginTools(
          allTools.filter((t) => t.source !== null).map((t) => t.name),
        );
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
    if (manifestInfo.defaultTools.length === 0)
      return new Set(manifestInfo.allCoreTools);
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
          <Loader2
            size={12}
            className="animate-spin"
            style={{ color: "var(--text-muted)" }}
          />
          <span className="text-[10px]" style={{ color: "var(--text-muted)" }}>
            Loading tools...
          </span>
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
                  <span
                    className="text-[10px] font-semibold"
                    style={{ color: "var(--text-secondary)" }}
                  >
                    {group}
                  </span>
                  <span
                    className="text-[9px] px-1 py-px rounded-full"
                    style={{
                      background: allDisabled
                        ? "rgba(239,68,68,0.1)"
                        : "rgba(63,63,70,0.4)",
                      color: allDisabled ? "#f87171" : "var(--text-muted)",
                    }}
                  >
                    {groupEnabled}/{tools.length}
                  </span>
                </div>
                <button
                  onClick={() => setGroup(tools, !allEnabled)}
                  className="text-[9px] px-1.5 py-0.5 rounded cursor-pointer border-none"
                  style={{
                    background: "rgba(63,63,70,0.3)",
                    color: "var(--text-muted)",
                  }}
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
                        borderTop:
                          i > 0 ? "1px solid rgba(63,63,70,0.2)" : undefined,
                      }}
                    >
                      <span
                        className="text-[11px] font-mono"
                        style={{
                          color: enabled
                            ? "var(--text-primary)"
                            : "var(--text-muted)",
                        }}
                      >
                        {tool}
                      </span>
                      <button
                        onClick={() => toggle(tool)}
                        className="relative w-7 h-[16px] rounded-full transition-colors"
                        style={{
                          background: enabled
                            ? "rgba(34,197,94,0.35)"
                            : "rgba(63,63,70,0.4)",
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
        {pluginTools.length > 0 &&
          (() => {
            const groupEnabled = pluginTools.filter((t) =>
              enabledTools.has(t),
            ).length;
            const allEnabled = groupEnabled === pluginTools.length;
            const allDisabled = groupEnabled === 0;
            return (
              <div>
                <div className="flex items-center justify-between mb-1.5">
                  <div className="flex items-center gap-1.5">
                    <span
                      className="text-[10px] font-semibold"
                      style={{ color: "var(--text-secondary)" }}
                    >
                      Plugin Tools
                    </span>
                    <span
                      className="text-[9px] px-1 py-px rounded-full"
                      style={{
                        background: allDisabled
                          ? "rgba(239,68,68,0.1)"
                          : "rgba(63,63,70,0.4)",
                        color: allDisabled ? "#f87171" : "var(--text-muted)",
                      }}
                    >
                      {groupEnabled}/{pluginTools.length}
                    </span>
                  </div>
                  <button
                    onClick={() => setGroup(pluginTools, !allEnabled)}
                    className="text-[9px] px-1.5 py-0.5 rounded cursor-pointer border-none"
                    style={{
                      background: "rgba(63,63,70,0.3)",
                      color: "var(--text-muted)",
                    }}
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
                          borderTop:
                            i > 0 ? "1px solid rgba(63,63,70,0.2)" : undefined,
                        }}
                      >
                        <span
                          className="text-[11px] font-mono"
                          style={{
                            color: enabled
                              ? "var(--text-primary)"
                              : "var(--text-muted)",
                          }}
                        >
                          {tool}
                        </span>
                        <button
                          onClick={() => toggle(tool)}
                          className="relative w-7 h-[16px] rounded-full transition-colors"
                          style={{
                            background: enabled
                              ? "rgba(34,197,94,0.35)"
                              : "rgba(63,63,70,0.4)",
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

function Section({
  icon,
  title,
  action,
  children,
}: {
  icon: React.ReactNode;
  title: string;
  action?: React.ReactNode;
  children: React.ReactNode;
}) {
  return (
    <div
      className="mb-3 p-3 rounded-lg"
      style={{
        background: "rgba(20,20,20,0.9)",
        border: "1px solid var(--border)",
      }}
    >
      <div className="flex items-center justify-between mb-2.5">
        <div className="flex items-center gap-1.5">
          <span style={{ color: "var(--text-muted)" }}>{icon}</span>
          <span
            className="text-[10px] font-semibold uppercase tracking-wider"
            style={{ color: "var(--text-muted)" }}
          >
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
// ===========================================================================
// Config Field
// ===========================================================================

const SENSITIVE_KEYS =
  /token|key|secret|password|credential|api.?key|access.?token|refresh.?token|client.?id|client.?secret/i;

function ConfigField({
  fieldKey,
  value,
  onSave,
}: {
  fieldKey: string;
  value: unknown;
  onSave: (v: string) => void;
}) {
  const displayValue =
    typeof value === "string" ? value : JSON.stringify(value);
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
      <span
        className="text-[10px] font-mono shrink-0 px-2 py-1 rounded truncate"
        style={{
          color: "var(--text-secondary)",
          background: "var(--bg-tertiary)",
          maxWidth: 120,
        }}
        title={fieldKey}
      >
        {fieldKey}
      </span>
      <input
        type={visible ? "text" : "password"}
        value={localValue}
        onChange={(e) => {
          setLocalValue(e.target.value);
          setDirty(e.target.value !== displayValue);
        }}
        onBlur={() => {
          if (dirty) onSave(localValue);
        }}
        onKeyDown={(e) => {
          if (e.key === "Enter" && dirty) onSave(localValue);
        }}
        className="flex-1 px-2 py-1 rounded-lg text-xs outline-none"
        style={{
          background: "var(--bg-tertiary)",
          border: "1px solid var(--border)",
          color: "var(--text-primary)",
        }}
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
// Generic Config Form
// ===========================================================================

function GenericConfigForm({
  pluginName,
  config,
  schema,
  running,
  onConfigSave,
}: {
  pluginName: string;
  config: Record<string, unknown>;
  schema: Record<string, ConfigFieldSchemaDto> | null;
  running: boolean;
  onConfigSave: (pluginName: string, key: string, value: string) => void;
}) {
  const { toast } = useToast();
  const saveField = (key: string, value: unknown) =>
    onConfigSave(pluginName, key, JSON.stringify(value));

  // If no schema, fall back to raw config field editor
  if (!schema) {
    const keys = Object.keys(config);
    return (
      <Section icon={<Settings2 size={12} />} title="Configuration">
        <div className="flex flex-col gap-2">
          {keys.map((key) => (
            <ConfigField
              key={key}
              fieldKey={key}
              value={config[key]}
              onSave={(v) => onConfigSave(pluginName, key, v)}
            />
          ))}
        </div>
      </Section>
    );
  }

  // Group fields by group, filter hidden, evaluate depends_on
  const visibleFields = Object.entries(schema)
    .filter(([, s]) => !s.hidden)
    .filter(([, s]) => {
      if (!s.dependsOn) return true;
      const depValue = String(config[s.dependsOn.field] ?? "");
      return s.dependsOn.values.includes(depValue);
    })
    .sort(([, a], [, b]) => a.order - b.order);

  const groups = new Map<string, [string, ConfigFieldSchemaDto][]>();
  for (const entry of visibleFields) {
    const group = entry[1].group ?? "General";
    if (!groups.has(group)) groups.set(group, []);
    groups.get(group)!.push(entry);
  }

  // Remaining keys not in schema
  const schemaKeys = new Set(Object.keys(schema));
  const remainingKeys = Object.keys(config).filter((k) => !schemaKeys.has(k));

  return (
    <>
      {[...groups.entries()].map(([group, fields]) => (
        <Section key={group} icon={<Settings2 size={12} />} title={group}>
          <div className="flex flex-col gap-2.5">
            {fields.map(([key, fieldSchema]) => (
              <GenericField
                key={key}
                fieldKey={key}
                schema={fieldSchema}
                value={config[key]}
                pluginName={pluginName}
                running={running}
                onSave={(v) => saveField(key, v)}
              />
            ))}
          </div>
        </Section>
      ))}
      {remainingKeys.length > 0 && (
        <Section icon={<Settings2 size={12} />} title="Other Settings">
          <div className="flex flex-col gap-2">
            {remainingKeys.map((key) => (
              <ConfigField
                key={key}
                fieldKey={key}
                value={config[key]}
                onSave={(v) => onConfigSave(pluginName, key, v)}
              />
            ))}
          </div>
        </Section>
      )}
    </>
  );
}

// ===========================================================================
// Generic Field (renders based on schema type + format)
// ===========================================================================

function GenericField({
  fieldKey,
  schema,
  value,
  pluginName,
  running,
  onSave,
}: {
  fieldKey: string;
  schema: ConfigFieldSchemaDto;
  value: unknown;
  pluginName: string;
  running: boolean;
  onSave: (value: unknown) => void;
}) {
  const label = schema.label ?? fieldKey;

  // string + valid_values → select
  if (schema.type === "string" && schema.validValues.length > 0) {
    const defaultStr =
      schema.default != null ? String(schema.default) : undefined;
    const effectiveValue =
      value != null && value !== "" ? String(value) : (defaultStr ?? "");
    return (
      <FieldRow label={label} description={schema.description}>
        <select
          value={effectiveValue}
          onChange={(e) => onSave(e.target.value)}
          className="w-full px-2.5 py-1.5 rounded-lg text-xs outline-none appearance-none"
          style={{
            background: "var(--bg-tertiary)",
            border: "1px solid var(--border)",
            color: "var(--text-primary)",
            backgroundImage: `url("data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' width='12' height='12' viewBox='0 0 24 24' fill='none' stroke='%2352525b' stroke-width='2'%3E%3Cpath d='m6 9 6 6 6-6'/%3E%3C/svg%3E")`,
            backgroundRepeat: "no-repeat",
            backgroundPosition: "right 8px center",
            paddingRight: "2rem",
          }}
        >
          <option value="">{defaultStr ? `— (${defaultStr})` : "—"}</option>
          {schema.validValues.map((v) => (
            <option key={v} value={v}>
              {v}
            </option>
          ))}
        </select>
      </FieldRow>
    );
  }

  // string[] + format: "phone" → phone list
  if (schema.type === "string[]" && schema.format === "phone") {
    return (
      <PhoneListField
        label={label}
        description={schema.description}
        value={value}
        onSave={onSave}
      />
    );
  }

  // string[] + options_command → remote select (fetch options from plugin)
  if (schema.type === "string[]" && schema.optionsCommand) {
    return (
      <RemoteOptionsField
        label={label}
        description={schema.description}
        value={value}
        pluginName={pluginName}
        command={schema.optionsCommand}
        running={running}
        onSave={onSave}
      />
    );
  }

  // string[] → tag list
  if (schema.type === "string[]") {
    return (
      <TagListField
        label={label}
        description={schema.description}
        value={value}
        onSave={onSave}
      />
    );
  }

  // bool → toggle
  if (schema.type === "bool") {
    const effective = value != null && value !== "" ? value : schema.default;
    const checked = effective === true || effective === "true";
    return (
      <FieldRow label={label} description={schema.description}>
        <button
          onClick={() => onSave(!checked)}
          className="relative w-7 h-[16px] rounded-full transition-colors"
          style={{
            background: checked ? "rgba(34,197,94,0.35)" : "rgba(63,63,70,0.4)",
            border: "none",
            cursor: "pointer",
            padding: 0,
          }}
        >
          <span
            className="absolute top-[2px] w-[12px] h-[12px] rounded-full transition-all"
            style={{
              background: checked ? "#22c55e" : "#52525b",
              left: checked ? 12 : 2,
            }}
          />
        </button>
      </FieldRow>
    );
  }

  // number → number input
  if (schema.type === "number") {
    const effectiveValue =
      value != null && value !== "" ? value : schema.default;
    return (
      <TextInputField
        label={label}
        description={schema.description}
        value={effectiveValue}
        type="number"
        sensitive={false}
        onSave={onSave}
        isDefault={value == null || value === ""}
      />
    );
  }

  // string (default) — with format hints
  const isSensitive =
    schema.format === "password" ||
    /token|key|secret|password|credential/i.test(fieldKey);
  const effectiveStrValue =
    value != null && value !== "" ? value : schema.default;
  return (
    <TextInputField
      label={label}
      description={schema.description}
      value={effectiveStrValue}
      type={schema.format === "url" ? "url" : "text"}
      sensitive={isSensitive}
      onSave={onSave}
      isDefault={value == null || value === ""}
    />
  );
}

function FieldRow({
  label,
  description,
  children,
}: {
  label: string;
  description?: string;
  children: React.ReactNode;
}) {
  return (
    <div>
      <div className="flex items-center justify-between gap-2 mb-1">
        <span
          className="text-[10px] font-medium"
          style={{ color: "var(--text-secondary)" }}
        >
          {label}
        </span>
        {children}
      </div>
      {description && (
        <p className="text-[9px]" style={{ color: "var(--text-muted)" }}>
          {description}
        </p>
      )}
    </div>
  );
}

function TextInputField({
  label,
  description,
  value,
  type,
  sensitive,
  onSave,
  isDefault,
}: {
  label: string;
  description?: string;
  value: unknown;
  type: string;
  sensitive: boolean;
  onSave: (v: unknown) => void;
  isDefault?: boolean;
}) {
  const strValue = value === null || value === undefined ? "" : String(value);
  const [local, setLocal] = useState(strValue);
  const [visible, setVisible] = useState(!sensitive);
  useEffect(() => {
    setLocal(value === null || value === undefined ? "" : String(value));
  }, [value]);

  const commit = () => {
    if (local !== strValue) {
      onSave(type === "number" ? (local === "" ? null : Number(local)) : local);
    }
  };

  return (
    <div>
      <span
        className="text-[10px] font-medium block mb-1"
        style={{ color: "var(--text-secondary)" }}
      >
        {label}
      </span>
      <div className="flex items-center gap-1">
        <input
          type={visible ? (type === "number" ? "number" : "text") : "password"}
          value={local}
          onChange={(e) => setLocal(e.target.value)}
          onBlur={commit}
          onKeyDown={(e) => {
            if (e.key === "Enter") commit();
          }}
          className="flex-1 px-2.5 py-1.5 rounded-lg text-xs outline-none"
          style={{
            background: "var(--bg-tertiary)",
            border: "1px solid var(--border)",
            color: isDefault ? "var(--text-muted)" : "var(--text-primary)",
          }}
        />
        {sensitive && (
          <button
            type="button"
            onClick={() => setVisible(!visible)}
            className="shrink-0 p-1 rounded cursor-pointer border-none bg-transparent"
            style={{ color: "var(--text-muted)" }}
          >
            {visible ? <EyeOff size={12} /> : <Eye size={12} />}
          </button>
        )}
      </div>
      {description && (
        <p className="text-[9px] mt-0.5" style={{ color: "var(--text-muted)" }}>
          {description}
        </p>
      )}
    </div>
  );
}

function TagListField({
  label,
  description,
  value,
  onSave,
}: {
  label: string;
  description?: string;
  value: unknown;
  onSave: (v: unknown) => void;
}) {
  const items: string[] = Array.isArray(value) ? (value as string[]) : [];
  const [adding, setAdding] = useState(false);
  const [newItem, setNewItem] = useState("");

  const handleAdd = () => {
    const trimmed = newItem.trim();
    if (!trimmed || items.includes(trimmed)) return;
    onSave([...items, trimmed]);
    setNewItem("");
    setAdding(false);
  };

  return (
    <div>
      <div className="flex items-center justify-between mb-1">
        <span
          className="text-[10px] font-medium"
          style={{ color: "var(--text-secondary)" }}
        >
          {label}
          {items.length > 0 ? ` (${items.length})` : ""}
        </span>
        {!adding && (
          <button
            onClick={() => setAdding(true)}
            className="inline-flex items-center gap-0.5 text-[9px] cursor-pointer border-none bg-transparent"
            style={{ color: "var(--text-muted)" }}
          >
            + Add
          </button>
        )}
      </div>
      {items.length > 0 && (
        <div className="flex flex-wrap gap-1 mb-1.5">
          {items.map((item) => (
            <span
              key={item}
              className="inline-flex items-center gap-1 px-2 py-0.5 rounded text-[10px] font-mono"
              style={{
                background: "var(--bg-tertiary)",
                border: "1px solid var(--border)",
                color: "var(--text-primary)",
              }}
            >
              {item}
              <button
                className="inline-flex border-none bg-transparent cursor-pointer p-0"
                style={{ color: "var(--text-muted)" }}
                onClick={() => onSave(items.filter((i) => i !== item))}
              >
                <X size={10} />
              </button>
            </span>
          ))}
        </div>
      )}
      {adding && (
        <div className="flex gap-1.5">
          <input
            value={newItem}
            onChange={(e) => setNewItem(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter") handleAdd();
              if (e.key === "Escape") setAdding(false);
            }}
            placeholder="Value"
            className="flex-1 px-2 py-1 rounded text-[10px] outline-none"
            style={{
              background: "var(--bg-tertiary)",
              border: "1px solid var(--border)",
              color: "var(--text-primary)",
            }}
            autoFocus
          />
          <button
            onClick={handleAdd}
            disabled={!newItem.trim()}
            className="px-2 py-1 rounded text-[10px] font-medium cursor-pointer border-none"
            style={{
              background: "#fafafa",
              color: "#09090b",
              opacity: !newItem.trim() ? 0.4 : 1,
            }}
          >
            Add
          </button>
          <button
            onClick={() => {
              setAdding(false);
              setNewItem("");
            }}
            className="px-2 py-1 rounded text-[10px] cursor-pointer border-none"
            style={{ background: "transparent", color: "var(--text-muted)" }}
          >
            Cancel
          </button>
        </div>
      )}
      {description && (
        <p className="text-[9px] mt-0.5" style={{ color: "var(--text-muted)" }}>
          {description}
        </p>
      )}
    </div>
  );
}

function PhoneListField({
  label,
  description,
  value,
  onSave,
}: {
  label: string;
  description?: string;
  value: unknown;
  onSave: (v: unknown) => void;
}) {
  const { toast } = useToast();
  const items: string[] = Array.isArray(value) ? (value as string[]) : [];
  const [showAdd, setShowAdd] = useState(false);
  const [phoneCountry, setPhoneCountry] = useState("54");
  const [phoneNumber, setPhoneNumber] = useState("");

  const handleAdd = () => {
    const digits = phoneNumber.replace(/\D/g, "");
    if (!digits) return;
    const full = phoneCountry + digits;
    if (items.includes(full)) {
      toast("info", "Already in list");
      return;
    }
    onSave([...items, full]);
    setPhoneNumber("");
    setShowAdd(false);
  };

  return (
    <div>
      <div className="flex items-center justify-between mb-1">
        <span
          className="text-[10px] font-medium"
          style={{ color: "var(--text-secondary)" }}
        >
          {label}
          {items.length > 0 ? ` (${items.length})` : ""}
        </span>
        {!showAdd && (
          <button
            onClick={() => setShowAdd(true)}
            className="inline-flex items-center gap-0.5 text-[9px] cursor-pointer border-none bg-transparent"
            style={{ color: "var(--text-muted)" }}
          >
            + Add
          </button>
        )}
      </div>
      {items.length > 0 && (
        <div className="flex flex-wrap gap-1 mb-1.5">
          {items.map((num) => (
            <span
              key={num}
              className="inline-flex items-center gap-1 px-2 py-0.5 rounded text-[10px] font-mono"
              style={{
                background: "var(--bg-tertiary)",
                border: "1px solid var(--border)",
                color: "var(--text-primary)",
              }}
            >
              {formatPhoneNumber(num)}
              <button
                className="inline-flex border-none bg-transparent cursor-pointer p-0"
                style={{ color: "var(--text-muted)" }}
                onClick={() => onSave(items.filter((n) => n !== num))}
              >
                <X size={10} />
              </button>
            </span>
          ))}
        </div>
      )}
      {items.length === 0 && !showAdd && (
        <p className="text-[10px]" style={{ color: "var(--text-muted)" }}>
          No numbers. All allowed.
        </p>
      )}
      {showAdd && (
        <div
          className="flex flex-wrap gap-1.5 p-2 rounded-lg"
          style={{
            background: "var(--bg-base)",
            border: "1px solid var(--border)",
          }}
        >
          <select
            value={phoneCountry}
            onChange={(e) => setPhoneCountry(e.target.value)}
            className="px-2 py-1 rounded text-[10px] outline-none appearance-none"
            style={{
              background: "var(--bg-tertiary)",
              border: "1px solid var(--border)",
              color: "var(--text-primary)",
              width: "100%",
              backgroundImage: `url("data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' width='12' height='12' viewBox='0 0 24 24' fill='none' stroke='%2352525b' stroke-width='2'%3E%3Cpath d='m6 9 6 6 6-6'/%3E%3C/svg%3E")`,
              backgroundRepeat: "no-repeat",
              backgroundPosition: "right 8px center",
              paddingRight: "2rem",
            }}
          >
            {COUNTRY_CODES.map((c) => (
              <option key={c.value} value={c.value}>
                {c.label}
              </option>
            ))}
          </select>
          <div className="flex gap-1.5 w-full">
            <input
              value={phoneNumber}
              placeholder="Phone number"
              onChange={(e) =>
                setPhoneNumber(e.target.value.replace(/\D/g, ""))
              }
              onKeyDown={(e) => {
                if (e.key === "Enter") handleAdd();
                if (e.key === "Escape") setShowAdd(false);
              }}
              className="flex-1 px-2 py-1 rounded text-[10px] outline-none"
              style={{
                background: "var(--bg-tertiary)",
                border: "1px solid var(--border)",
                color: "var(--text-primary)",
              }}
            />
            <button
              onClick={handleAdd}
              disabled={!phoneNumber.trim()}
              className="inline-flex items-center gap-0.5 px-2 py-1 rounded text-[10px] font-medium cursor-pointer border-none"
              style={{
                background: "#fafafa",
                color: "#09090b",
                opacity: !phoneNumber.trim() ? 0.4 : 1,
              }}
            >
              Add
            </button>
            <button
              onClick={() => {
                setShowAdd(false);
                setPhoneNumber("");
              }}
              className="px-2 py-1 rounded text-[10px] cursor-pointer border-none"
              style={{ background: "transparent", color: "var(--text-muted)" }}
            >
              Cancel
            </button>
          </div>
        </div>
      )}
      {description && (
        <p className="text-[9px] mt-0.5" style={{ color: "var(--text-muted)" }}>
          {description}
        </p>
      )}
    </div>
  );
}

function RemoteOptionsField({
  label,
  description,
  value,
  pluginName,
  command,
  running,
  onSave,
}: {
  label: string;
  description?: string;
  value: unknown;
  pluginName: string;
  command: string;
  running: boolean;
  onSave: (v: unknown) => void;
}) {
  const { toast } = useToast();
  const items: string[] = Array.isArray(value) ? (value as string[]) : [];
  const [options, setOptions] = useState<{ id: string; label?: string }[]>([]);
  const [loading, setLoading] = useState(false);
  const [fetched, setFetched] = useState(false);

  const fetchOptions = async () => {
    setLoading(true);
    try {
      const output = await runPluginCommand(pluginName, command);
      // Take only the first line (plugin may print duplicates)
      const firstLine = output.split("\n")[0]?.trim() ?? output;
      const parsed = JSON.parse(firstLine);
      // Support both [{id, label}] and [{id, subject}] (WhatsApp compat)
      setOptions(
        Array.isArray(parsed)
          ? parsed.map((o: Record<string, unknown>) => ({
              id: String(o.id ?? o.value ?? ""),
              label: String(o.label ?? o.subject ?? o.name ?? o.id ?? ""),
            }))
          : [],
      );
      setFetched(true);
    } catch (e) {
      toast("error", `Failed to fetch options: ${e}`);
    } finally {
      setLoading(false);
    }
  };

  const toggle = (id: string) => {
    const next = items.includes(id)
      ? items.filter((i) => i !== id)
      : [...items, id];
    onSave(next);
  };

  return (
    <div>
      <div className="flex items-center justify-between mb-1">
        <span
          className="text-[10px] font-medium"
          style={{ color: "var(--text-secondary)" }}
        >
          {label}
          {items.length > 0 ? ` (${items.length})` : ""}
        </span>
        <button
          onClick={fetchOptions}
          disabled={!running || loading}
          className="inline-flex items-center gap-1 text-[10px] cursor-pointer border-none bg-transparent"
          style={{
            color: "var(--text-muted)",
            opacity: !running || loading ? 0.4 : 1,
          }}
        >
          {loading ? (
            <Loader2 size={10} className="animate-spin" />
          ) : (
            <RefreshCw size={10} />
          )}
          Fetch
        </button>
      </div>
      {!running && !fetched && (
        <p className="text-[10px]" style={{ color: "var(--text-muted)" }}>
          Start plugin to fetch options.
        </p>
      )}
      {options.length > 0 && (
        <div
          className="flex flex-col gap-px rounded-lg overflow-auto"
          style={{
            maxHeight: 180,
            border: "1px solid var(--border)",
          }}
        >
          {options.map((opt) => (
            <label
              key={opt.id}
              className="flex items-center gap-2 px-2.5 py-1.5 cursor-pointer text-xs"
              style={{ background: "var(--bg-secondary)" }}
              onMouseEnter={(e) => {
                e.currentTarget.style.background = "var(--bg-hover)";
              }}
              onMouseLeave={(e) => {
                e.currentTarget.style.background = "var(--bg-secondary)";
              }}
            >
              <input
                type="checkbox"
                checked={items.includes(opt.id)}
                onChange={() => toggle(opt.id)}
                style={{ accentColor: "#a1a1aa" }}
              />
              <span
                className="truncate"
                style={{ color: "var(--text-primary)" }}
              >
                {opt.label ?? opt.id}
              </span>
            </label>
          ))}
        </div>
      )}
      {description && (
        <p className="text-[9px] mt-0.5" style={{ color: "var(--text-muted)" }}>
          {description}
        </p>
      )}
    </div>
  );
}

// ===========================================================================
// QR Modal
// ===========================================================================

function QrModal({
  qrDataUrl,
  qrLoading,
  qrLinked,
  onClose,
  onDone,
}: {
  qrDataUrl: string | null;
  qrLoading: boolean;
  qrLinked: boolean;
  onClose: () => void;
  onDone: () => void;
}) {
  useEffect(() => {
    const handler = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose();
    };
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
        style={{
          background: "rgba(20,20,20,0.98)",
          border: "1px solid var(--border)",
          maxWidth: 300,
        }}
        onClick={(e) => e.stopPropagation()}
      >
        {qrLinked ? (
          <>
            <div
              className="flex items-center justify-center"
              style={{
                width: 56,
                height: 56,
                borderRadius: "50%",
                background: "rgba(74,222,128,0.15)",
                border: "2px solid rgba(74,222,128,0.4)",
              }}
            >
              <Check size={28} style={{ color: "#4ade80" }} />
            </div>
            <span
              className="text-sm font-semibold"
              style={{ color: "var(--text-primary)" }}
            >
              Linked!
            </span>
            <p
              className="text-[11px] text-center"
              style={{ color: "var(--text-secondary)" }}
            >
              Connected successfully.
            </p>
            <button
              onClick={onDone}
              className="px-3 py-1.5 rounded-lg text-xs font-medium cursor-pointer border-none"
              style={{ background: "#fafafa", color: "#09090b" }}
            >
              Done
            </button>
          </>
        ) : (
          <>
            <div className="flex items-center gap-1.5">
              <QrCode size={14} style={{ color: "var(--text-secondary)" }} />
              <span
                className="text-xs font-semibold"
                style={{ color: "var(--text-primary)" }}
              >
                Scan QR Code
              </span>
            </div>
            <p
              className="text-[10px] text-center"
              style={{ color: "var(--text-muted)" }}
            >
              Scan with your device to link
            </p>
            {qrLoading ? (
              <div
                className="flex items-center justify-center"
                style={{ width: 220, height: 220 }}
              >
                <Loader2
                  size={24}
                  className="animate-spin"
                  style={{ color: "var(--text-muted)" }}
                />
              </div>
            ) : qrDataUrl ? (
              <img
                src={qrDataUrl}
                alt="QR"
                style={{
                  width: 220,
                  height: 220,
                  borderRadius: 8,
                  imageRendering: "pixelated",
                }}
              />
            ) : (
              <div
                className="flex items-center justify-center text-[10px] text-center"
                style={{
                  width: 220,
                  height: 140,
                  color: "var(--text-muted)",
                  background: "var(--bg-tertiary)",
                  borderRadius: 8,
                  border: "1px dashed var(--border)",
                  padding: 16,
                }}
              >
                No QR available. Is the plugin running?
              </div>
            )}
            <button
              onClick={onClose}
              className="px-3 py-1.5 rounded-lg text-xs cursor-pointer"
              style={{
                background: "var(--bg-tertiary)",
                color: "var(--text-primary)",
                border: "1px solid var(--border)",
              }}
            >
              Close
            </button>
          </>
        )}
      </div>
    </div>
  );
}

// ===========================================================================
// Add-ons View (inline, not modal)
// ===========================================================================

function AddonsView({
  remotePlugins,
  installedNames,
  loading,
  installing,
  search,
  onSearchChange,
  onInstall,
}: {
  remotePlugins: RemotePlugin[];
  installedNames: Set<string>;
  loading: boolean;
  installing: string | null;
  search: string;
  onSearchChange: (v: string) => void;
  onInstall: (name: string) => void;
}) {
  const filtered = search
    ? remotePlugins.filter(
        (p) =>
          p.name.toLowerCase().includes(search.toLowerCase()) ||
          p.description?.toLowerCase().includes(search.toLowerCase()),
      )
    : remotePlugins;

  const available = filtered.filter(
    (p) => !installedNames.has(p.name) && p.available,
  );
  const installed = filtered.filter((p) => installedNames.has(p.name));

  if (loading) {
    return (
      <div
        style={{
          display: "flex",
          alignItems: "center",
          justifyContent: "center",
          height: 200,
          gap: 8,
          color: "#52525b",
        }}
      >
        <Loader2 size={16} className="animate-spin" />
        <span style={{ fontSize: 13 }}>Loading add-ons...</span>
      </div>
    );
  }

  return (
    <div>
      {/* Search */}
      <div style={{ position: "relative", marginBottom: 14 }}>
        <Search
          size={13}
          style={{
            position: "absolute",
            left: 10,
            top: "50%",
            transform: "translateY(-50%)",
            color: "#52525b",
          }}
        />
        <input
          className="s-input"
          style={{ width: "100%", paddingLeft: 30 }}
          placeholder="Search add-ons..."
          value={search}
          onChange={(e) => onSearchChange(e.target.value)}
        />
      </div>

      {/* Available */}
      {available.length > 0 && (
        <div className="s-section">
          <div className="s-section-title">Available</div>
          <div className="s-card">
            {available.map((p) => (
              <div key={p.name} className="s-row">
                <div style={{ minWidth: 0, flex: 1 }}>
                  <div
                    style={{ display: "flex", alignItems: "center", gap: 6 }}
                  >
                    <span
                      style={{
                        fontSize: 12.5,
                        fontWeight: 600,
                        color: "var(--text-primary)",
                      }}
                    >
                      {p.name}
                    </span>
                    <span
                      style={{
                        fontSize: 10,
                        fontFamily: "var(--font-mono)",
                        color: "var(--text-muted)",
                      }}
                    >
                      v{p.version}
                    </span>
                    <span className="s-badge s-badge-gray">{p.pluginType}</span>
                  </div>
                  {p.description && (
                    <div
                      style={{
                        fontSize: 11,
                        color: "var(--text-muted)",
                        marginTop: 2,
                        overflow: "hidden",
                        textOverflow: "ellipsis",
                        whiteSpace: "nowrap",
                      }}
                    >
                      {p.description}
                    </div>
                  )}
                </div>
                <button
                  className="s-btn"
                  onClick={() => onInstall(p.name)}
                  disabled={installing !== null}
                  style={{
                    opacity: installing !== null ? 0.4 : 1,
                    flexShrink: 0,
                  }}
                >
                  {installing === p.name ? (
                    <Loader2 size={11} className="animate-spin" />
                  ) : (
                    <Download size={11} />
                  )}
                  Install
                </button>
              </div>
            ))}
          </div>
        </div>
      )}

      {/* Already installed */}
      {installed.length > 0 && (
        <div className="s-section">
          <div className="s-section-title">Already Installed</div>
          <div className="s-card">
            {installed.map((p) => (
              <div key={p.name} className="s-row">
                <div style={{ display: "flex", alignItems: "center", gap: 6 }}>
                  <span
                    style={{ fontSize: 12.5, color: "var(--text-primary)" }}
                  >
                    {p.name}
                  </span>
                  <span
                    style={{
                      fontSize: 10,
                      fontFamily: "var(--font-mono)",
                      color: "var(--text-muted)",
                    }}
                  >
                    v{p.version}
                  </span>
                </div>
                <span className="s-badge s-badge-green">Installed</span>
              </div>
            ))}
          </div>
        </div>
      )}

      {filtered.length === 0 && (
        <div
          style={{
            textAlign: "center",
            padding: "32px 0",
            color: "#52525b",
            fontSize: 12,
          }}
        >
          {search ? "No add-ons match your search." : "No add-ons available."}
        </div>
      )}
    </div>
  );
}
