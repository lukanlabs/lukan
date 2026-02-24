import { useState, useEffect, useCallback, useRef } from "react";
import type { PluginInfo, RemotePlugin, WhatsAppGroup, PluginCommand } from "../../lib/types";
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
} from "../../lib/tauri";
import QRCode from "qrcode";
import { useToast } from "../ui/Toast";
import Button from "../ui/Button";
import Input from "../ui/Input";
import Card from "../ui/Card";
import Badge from "../ui/Badge";
import Select from "../ui/Select";
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
} from "lucide-react";

// ---------------------------------------------------------------------------
// Master-Detail Plugins Tab
// ---------------------------------------------------------------------------

export default function PluginsTab() {
  const { toast } = useToast();

  // --- shared state --------------------------------------------------------
  const [plugins, setPlugins] = useState<PluginInfo[]>([]);
  const [selectedPlugin, setSelectedPlugin] = useState<string | null>(null);
  const [showRegistry, setShowRegistry] = useState(false);

  // --- master view state ---------------------------------------------------
  const [localPath, setLocalPath] = useState("");
  const [installingLocal, setInstallingLocal] = useState(false);

  // --- detail view state ---------------------------------------------------
  const [actionLoading, setActionLoading] = useState<"start" | "stop" | "restart" | "remove" | null>(null);
  const [pluginConfig, setPluginConfig] = useState<Record<string, unknown>>({});
  const [pluginLogs, setPluginLogs] = useState("");
  const [logsRefreshing, setLogsRefreshing] = useState(false);
  const [pluginCommands, setPluginCommands] = useState<PluginCommand[]>([]);
  const [commandRunning, setCommandRunning] = useState<string | null>(null);
  const [commandOutput, setCommandOutput] = useState<string | null>(null);

  // --- WhatsApp auth state -------------------------------------------------
  const [whatsappAuthed, setWhatsappAuthed] = useState(false);

  // --- QR state ------------------------------------------------------------
  const [showQr, setShowQr] = useState(false);
  const [qrDataUrl, setQrDataUrl] = useState<string | null>(null);
  const [qrLoading, setQrLoading] = useState(false);
  const [qrLinked, setQrLinked] = useState(false);

  // Poll for auth completion while QR modal is showing
  useEffect(() => {
    if (!showQr || !qrDataUrl || qrLinked) return;
    let cancelled = false;
    const interval = setInterval(async () => {
      try {
        const qr = await getWhatsappQr();
        if (!cancelled && !qr) {
          // QR file gone = connector connected successfully
          setQrLinked(true);
          toast("success", "WhatsApp linked successfully!");
        }
      } catch {}
    }, 3000);
    return () => { cancelled = true; clearInterval(interval); };
  }, [showQr, qrDataUrl, qrLinked, toast]);

  // --- registry state ------------------------------------------------------
  const [remotePlugins, setRemotePlugins] = useState<RemotePlugin[]>([]);
  const [registryLoading, setRegistryLoading] = useState(false);
  const [installingRemote, setInstallingRemote] = useState<string | null>(null);

  // --- QR: no polling, just read the file ---------------------------------

  // -------------------------------------------------------------------------
  // Data fetching
  // -------------------------------------------------------------------------

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

  // When a plugin is selected, load its config, logs, and commands
  const loadPluginDetail = useCallback(
    async (name: string) => {
      try {
        const [cfg, logs, cmds] = await Promise.all([
          getPluginConfig(name),
          getPluginLogs(name, 200),
          getPluginCommands(name).catch(() => [] as PluginCommand[]),
        ]);
        setPluginConfig(cfg);
        setPluginLogs(logs);
        setPluginCommands(cmds);

        // Check WhatsApp auth status if this is the whatsapp plugin
        if (name === "whatsapp") {
          const authed = await checkWhatsappAuth().catch(() => false);
          setWhatsappAuthed(authed);
        }
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
    setWhatsappAuthed(false);
  }, []);

  // -------------------------------------------------------------------------
  // Handlers - master
  // -------------------------------------------------------------------------

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

  // -------------------------------------------------------------------------
  // Handlers - detail
  // -------------------------------------------------------------------------

  const handleStart = async (name: string) => {
    setActionLoading("start");
    try {
      await startPlugin(name);
      toast("success", `Plugin '${name}' started`);
      await refreshPlugins();
      // Auto-refresh logs after start
      await handleRefreshLogs(name);
    } catch (e) {
      toast("error", `Failed to start plugin: ${e}`);
    } finally {
      setActionLoading(null);
    }
  };

  const handleStop = async (name: string) => {
    setActionLoading("stop");
    try {
      await stopPlugin(name);
      toast("success", `Plugin '${name}' stopped`);
      await refreshPlugins();
      await handleRefreshLogs(name);
    } catch (e) {
      toast("error", `Failed to stop plugin: ${e}`);
    } finally {
      setActionLoading(null);
    }
  };

  const handleRestart = async (name: string) => {
    setActionLoading("restart");
    try {
      await restartPlugin(name);
      toast("success", `Plugin '${name}' restarted`);
      await refreshPlugins();
      await handleRefreshLogs(name);
    } catch (e) {
      toast("error", `Failed to restart plugin: ${e}`);
    } finally {
      setActionLoading(null);
    }
  };

  const handleShowQr = async () => {
    setShowQr(true);
    setQrLoading(true);
    setQrDataUrl(null);
    setQrLinked(false);
    try {
      const qrStr = await getWhatsappQr();
      if (qrStr) {
        const url = await QRCode.toDataURL(qrStr, {
          width: 320,
          margin: 2,
          color: { dark: "#000000", light: "#ffffff" },
        });
        setQrDataUrl(url);
      }
    } catch (e) {
      toast("error", `Failed to get QR: ${e}`);
    } finally {
      setQrLoading(false);
    }
  };

  const handleRemove = async (name: string) => {
    setActionLoading("remove");
    try {
      await removePlugin(name);
      toast("success", `Plugin '${name}' removed`);
      await refreshPlugins();
      goBack();
    } catch (e) {
      toast("error", `Failed to remove plugin: ${e}`);
    } finally {
      setActionLoading(null);
    }
  };

  const handleConfigSave = async (pluginName: string, key: string, value: string) => {
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
      if (output && output.trim()) {
        setCommandOutput(output.trim());
      }
      toast("success", `Command '${command}' completed`);
      // Refresh config in case auth saved tokens
      const cfg = await getPluginConfig(pluginName);
      setPluginConfig(cfg);
    } catch (e) {
      toast("error", `Command '${command}' failed: ${e}`);
    } finally {
      setCommandRunning(null);
    }
  };

  // -------------------------------------------------------------------------
  // Handlers - registry
  // -------------------------------------------------------------------------

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
      toast("success", `Plugin '${installed}' installed from registry`);
      await refreshPlugins();
      const remotes = await listRemotePlugins();
      setRemotePlugins(remotes);
    } catch (e) {
      toast("error", `${e}`);
    } finally {
      setInstallingRemote(null);
    }
  };

  // -------------------------------------------------------------------------
  // Resolve selected plugin from the list
  // -------------------------------------------------------------------------

  const activePlugin = selectedPlugin
    ? plugins.find((p) => p.name === selectedPlugin) ?? null
    : null;

  // -------------------------------------------------------------------------
  // Render
  // -------------------------------------------------------------------------

  return (
    <div className={selectedPlugin && activePlugin ? "max-w-5xl" : "max-w-3xl"} style={{ animation: "fadeIn 0.3s ease-out" }}>
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
          onStart={handleStart}
          onStop={handleStop}
          onRestart={handleRestart}
          onRemove={handleRemove}
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

      {/* QR modal overlay */}
      {showQr && (
        <div
          className="fixed inset-0 flex items-center justify-center z-50"
          style={{
            background: "rgba(0,0,0,0.75)",
            backdropFilter: "blur(8px)",
            animation: "fadeIn 0.2s ease-out",
          }}
          onClick={() => setShowQr(false)}
        >
          <div
            className="rounded-2xl p-8 flex flex-col items-center gap-5"
            style={{
              background: "rgba(20, 20, 20, 0.98)",
              border: "1px solid var(--border)",
              boxShadow: "var(--shadow-lg)",
              animation: "scaleIn 0.25s cubic-bezier(0.4, 0, 0.2, 1)",
            }}
            onClick={(e) => e.stopPropagation()}
          >
            {qrLinked ? (
              <>
                <div
                  className="flex items-center justify-center"
                  style={{
                    width: 80,
                    height: 80,
                    borderRadius: "50%",
                    background: "rgba(74, 222, 128, 0.15)",
                    border: "2px solid rgba(74, 222, 128, 0.4)",
                  }}
                >
                  <Check size={40} style={{ color: "#4ade80" }} />
                </div>
                <h3 className="text-lg font-bold" style={{ color: "var(--text-primary)" }}>
                  WhatsApp Linked!
                </h3>
                <p className="text-sm text-center" style={{ color: "var(--text-secondary)", maxWidth: 280 }}>
                  Your WhatsApp account has been successfully connected.
                </p>
                <Button onClick={() => { setShowQr(false); refreshPlugins(); }}>
                  Done
                </Button>
              </>
            ) : (
              <>
                <div className="flex items-center gap-2">
                  <QrCode size={18} style={{ color: "var(--text-secondary)" }} />
                  <h3 className="text-base font-bold" style={{ color: "var(--text-primary)" }}>
                    Scan QR Code
                  </h3>
                </div>
                <p className="text-xs text-center" style={{ color: "var(--text-muted)", maxWidth: 280 }}>
                  Open WhatsApp on your phone, go to Settings &gt; Linked Devices, and scan this code.
                </p>
                {qrLoading ? (
                  <div className="flex items-center justify-center" style={{ width: 320, height: 320 }}>
                    <Loader2 size={32} className="animate-spin" style={{ color: "var(--text-muted)" }} />
                  </div>
                ) : qrDataUrl ? (
                  <img
                    src={qrDataUrl}
                    alt="WhatsApp QR Code"
                    style={{ width: 320, height: 320, borderRadius: 12, imageRendering: "pixelated" }}
                  />
                ) : (
                  <div
                    className="flex items-center justify-center text-sm text-center"
                    style={{
                      width: 320,
                      height: 200,
                      color: "var(--text-muted)",
                      background: "var(--bg-tertiary)",
                      borderRadius: 12,
                      border: "1px dashed var(--border)",
                      padding: 24,
                    }}
                  >
                    No QR code available. Make sure the plugin is running and waiting for authentication.
                  </div>
                )}
                <Button variant="secondary" onClick={() => setShowQr(false)}>
                  <X size={14} />
                  Close
                </Button>
              </>
            )}
          </div>
        </div>
      )}

      {/* Registry modal overlay */}
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
  onLocalPathChange: (value: string) => void;
  onInstallLocal: () => void;
  onSelectPlugin: (name: string) => void;
  onOpenRegistry: () => void;
}

function MasterView({
  plugins,
  localPath,
  installingLocal,
  onLocalPathChange,
  onInstallLocal,
  onSelectPlugin,
  onOpenRegistry,
}: MasterViewProps) {
  return (
    <>
      {/* Header */}
      <div className="flex items-center justify-between mb-10">
        <div className="flex items-center gap-3">
          <div>
            <h2
              className="text-xl font-bold tracking-tight"
              style={{ color: "var(--text-primary)" }}
            >
              Plugins
            </h2>
            <p className="text-sm mt-1.5" style={{ color: "var(--text-muted)" }}>
              Install, configure, and manage agent plugins.
            </p>
          </div>
          <Badge variant="neutral">{plugins.length} installed</Badge>
        </div>
        <Button variant="secondary" onClick={onOpenRegistry}>
          <Download size={14} />
          Install from Registry
        </Button>
      </div>

      {/* Install from local path */}
      <Card className="mb-6">
        <h4
          className="text-[11px] font-bold uppercase tracking-[0.1em] mb-3"
          style={{ color: "var(--text-muted)" }}
        >
          Install from local path
        </h4>
        <div className="flex items-end gap-3">
          <div className="flex-1">
            <Input
              value={localPath}
              placeholder="/path/to/plugin"
              onChange={(e) => onLocalPathChange(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === "Enter") onInstallLocal();
              }}
            />
          </div>
          <Button onClick={onInstallLocal} disabled={installingLocal || !localPath.trim()}>
            {installingLocal ? (
              <Loader2 size={14} className="animate-spin" />
            ) : (
              <FolderOpen size={14} />
            )}
            {installingLocal ? "Installing..." : "Install"}
          </Button>
        </div>
      </Card>

      {/* Plugin list */}
      {plugins.length === 0 ? (
        <div
          className="text-center py-20 rounded-2xl"
          style={{
            color: "var(--text-muted)",
            background: "var(--surface-glass)",
            border: "1px dashed var(--border)",
          }}
        >
          <Package size={36} style={{ margin: "0 auto 12px", opacity: 0.4 }} />
          <p className="text-sm font-medium" style={{ color: "var(--text-secondary)" }}>
            No plugins installed
          </p>
          <p className="text-xs mt-1" style={{ color: "var(--text-muted)" }}>
            Browse the registry to get started.
          </p>
        </div>
      ) : (
        <div className="flex flex-col gap-3">
          {plugins.map((plugin) => (
            <PluginCard
              key={plugin.name}
              plugin={plugin}
              onClick={() => onSelectPlugin(plugin.name)}
            />
          ))}
        </div>
      )}
    </>
  );
}

// ===========================================================================
// Plugin Card (master list item)
// ===========================================================================

interface PluginCardProps {
  plugin: PluginInfo;
  onClick: () => void;
}

function PluginCard({ plugin, onClick }: PluginCardProps) {
  return (
    <div
      className="rounded-2xl p-5 cursor-pointer transition-all"
      style={{
        background:
          "linear-gradient(135deg, rgba(255,255,255,0.04) 0%, rgba(255,255,255,0.01) 100%)",
        border: "1px solid var(--border)",
        boxShadow: "var(--shadow-sm), inset 0 1px 0 rgba(255,255,255,0.03)",
        transitionDuration: "180ms",
      }}
      onClick={onClick}
      onMouseEnter={(e) => {
        e.currentTarget.style.borderColor = "var(--border-hover)";
        e.currentTarget.style.boxShadow =
          "var(--shadow-md), inset 0 1px 0 rgba(255,255,255,0.05)";
        e.currentTarget.style.transform = "translateY(-1px)";
      }}
      onMouseLeave={(e) => {
        e.currentTarget.style.borderColor = "var(--border)";
        e.currentTarget.style.boxShadow =
          "var(--shadow-sm), inset 0 1px 0 rgba(255,255,255,0.03)";
        e.currentTarget.style.transform = "";
      }}
    >
      <div className="flex items-center justify-between">
        <div>
          <div className="flex items-center gap-2 flex-wrap">
            <span
              className="font-semibold text-sm"
              style={{ color: "var(--text-primary)" }}
            >
              {plugin.name}
            </span>
            <span className="text-xs font-mono" style={{ color: "var(--text-muted)" }}>
              v{plugin.version}
            </span>
            {plugin.alias && <Badge variant="neutral">{plugin.alias}</Badge>}
          </div>
          {plugin.description && (
            <p
              className="text-xs mt-1.5 leading-relaxed"
              style={{ color: "var(--text-secondary)" }}
            >
              {plugin.description}
            </p>
          )}
        </div>
        {plugin.pluginType === "channel" && (
          <Badge variant={plugin.running ? "success" : "neutral"}>
            {plugin.running ? "Running" : "Stopped"}
          </Badge>
        )}
      </div>
    </div>
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
  onStart: (name: string) => void;
  onStop: (name: string) => void;
  onRestart: (name: string) => void;
  onRemove: (name: string) => void;
  onConfigSave: (pluginName: string, key: string, value: string) => void;
  onRefreshLogs: (name: string) => void;
  onShowQr: () => void;
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
  whatsappAuthed,
  onBack,
  onStart,
  onStop,
  onRestart,
  onRemove,
  onConfigSave,
  onRefreshLogs,
  onShowQr,
  onRunCommand,
}: DetailViewProps) {
  const logsEndRef = useRef<HTMLPreElement>(null);

  // Auto-scroll logs to bottom on refresh
  useEffect(() => {
    if (logsEndRef.current) {
      logsEndRef.current.scrollTop = logsEndRef.current.scrollHeight;
    }
  }, [logs]);

  const configKeys = Object.keys(config);
  const hasConfig = configKeys.length > 0;

  return (
    <div style={{ animation: "fadeIn 0.2s ease-out" }}>
      {/* Back button */}
      <button
        className="inline-flex items-center gap-1.5 px-0 py-1 mb-6 text-sm font-medium cursor-pointer border-none bg-transparent transition-all"
        style={{ color: "var(--text-muted)", transitionDuration: "150ms" }}
        onClick={onBack}
        onMouseEnter={(e) => {
          e.currentTarget.style.color = "var(--text-primary)";
        }}
        onMouseLeave={(e) => {
          e.currentTarget.style.color = "var(--text-muted)";
        }}
      >
        <ArrowLeft size={16} />
        Plugins
      </button>

      {/* Plugin header */}
      <div className="mb-8">
        <div className="flex items-start justify-between gap-4">
          <div>
            <div className="flex items-center gap-2.5 flex-wrap mb-2">
              <h2
                className="text-xl font-bold tracking-tight"
                style={{ color: "var(--text-primary)" }}
              >
                {plugin.name}
              </h2>
              <span className="text-sm font-mono" style={{ color: "var(--text-muted)" }}>
                v{plugin.version}
              </span>
              <Badge variant="neutral">{plugin.pluginType}</Badge>
              {plugin.alias && <Badge variant="neutral">{plugin.alias}</Badge>}
              {plugin.pluginType === "channel" && (
                <Badge variant={plugin.running ? "success" : "neutral"}>
                  {plugin.running ? "Running" : "Stopped"}
                </Badge>
              )}
            </div>
            {plugin.description && (
              <p
                className="text-sm leading-relaxed"
                style={{ color: "var(--text-secondary)" }}
              >
                {plugin.description}
              </p>
            )}
          </div>
          <div className="flex items-center gap-2 shrink-0">
            {plugin.pluginType === "channel" && (
              plugin.running ? (
                <>
                  <Button
                    size="sm"
                    onClick={() => onStop(plugin.name)}
                    disabled={actionLoading !== null}
                  >
                    {actionLoading === "stop" ? (
                      <Loader2 size={12} className="animate-spin" />
                    ) : (
                      <Square size={12} />
                    )}
                    {actionLoading === "stop" ? "Stopping..." : "Stop"}
                  </Button>
                  <Button
                    variant="secondary"
                    size="sm"
                    onClick={() => onRestart(plugin.name)}
                    disabled={actionLoading !== null}
                  >
                    {actionLoading === "restart" ? (
                      <Loader2 size={12} className="animate-spin" />
                    ) : (
                      <RotateCw size={12} />
                    )}
                    {actionLoading === "restart" ? "Restarting..." : "Restart"}
                  </Button>
                </>
              ) : (
                <Button
                  size="sm"
                  onClick={() => onStart(plugin.name)}
                  disabled={actionLoading !== null}
                >
                  {actionLoading === "start" ? (
                    <Loader2 size={12} className="animate-spin" />
                  ) : (
                    <Play size={12} />
                  )}
                  {actionLoading === "start" ? "Starting..." : "Start"}
                </Button>
              )
            )}
            <Button
              variant="danger"
              size="sm"
              onClick={() => onRemove(plugin.name)}
              disabled={actionLoading !== null}
            >
              {actionLoading === "remove" ? (
                <Loader2 size={12} className="animate-spin" />
              ) : (
                <Trash2 size={12} />
              )}
              {actionLoading === "remove" ? "Uninstalling..." : "Uninstall"}
            </Button>
          </div>
        </div>
      </div>

      {/* Plugin commands (auth, etc.) */}
      {commands.length > 0 && (
        <Card className="mb-6">
          <div className="flex items-center gap-2 mb-4">
            <Settings2 size={14} style={{ color: "var(--text-muted)" }} />
            <h3
              className="text-[11px] font-bold uppercase tracking-[0.1em]"
              style={{ color: "var(--text-muted)" }}
            >
              Commands
            </h3>
          </div>
          <div className="flex flex-col gap-3">
            {commands.map((cmd) => {
              const isAuth = cmd.name === "auth";
              const isAuthenticated = isAuth && (
                plugin.name === "whatsapp"
                  ? whatsappAuthed
                  : !!config.accessToken
              );
              return (
                <div key={cmd.name} className="flex items-center justify-between">
                  <div className="flex items-center gap-2">
                    <div>
                      <span
                        className="text-sm font-medium"
                        style={{ color: "var(--text-primary)" }}
                      >
                        {cmd.name}
                      </span>
                      {cmd.description && (
                        <p className="text-xs mt-0.5" style={{ color: "var(--text-muted)" }}>
                          {cmd.description}
                        </p>
                      )}
                    </div>
                    {isAuth && (
                      <Badge variant={isAuthenticated ? "success" : "neutral"}>
                        {isAuthenticated ? "Authenticated" : "Not authenticated"}
                      </Badge>
                    )}
                  </div>
                  <Button
                    size="sm"
                    variant={isAuth && isAuthenticated ? "secondary" : "primary"}
                    onClick={() => onRunCommand(plugin.name, cmd.name)}
                    disabled={commandRunning !== null}
                  >
                    {commandRunning === cmd.name ? (
                      <Loader2 size={12} className="animate-spin" />
                    ) : (
                      <Play size={12} />
                    )}
                    {commandRunning === cmd.name
                      ? "Running..."
                      : isAuth && isAuthenticated
                        ? "Re-authenticate"
                        : "Run"}
                  </Button>
                </div>
              );
            })}
          </div>
          {commandOutput && (
            <pre
              className="text-xs mt-4 p-3 rounded-xl overflow-auto"
              style={{
                background: "var(--bg-tertiary)",
                color: "var(--text-secondary)",
                border: "1px solid var(--border)",
                maxHeight: 200,
                whiteSpace: "pre-wrap",
                wordBreak: "break-word",
              }}
            >
              {commandOutput}
            </pre>
          )}
        </Card>
      )}

      {/* WhatsApp QR section */}
      {plugin.name.includes("whatsapp") && (
        <Card className="mb-6">
          <div className="flex items-center justify-between">
            <div className="flex items-center gap-2">
              <QrCode size={14} style={{ color: "var(--text-secondary)" }} />
              <h3
                className="text-[11px] font-bold uppercase tracking-[0.1em]"
                style={{ color: "var(--text-muted)" }}
              >
                WhatsApp Authentication
              </h3>
            </div>
            <Button onClick={onShowQr}>
              <QrCode size={14} />
              View QR Code
            </Button>
          </div>
          <p className="text-xs mt-2" style={{ color: "var(--text-secondary)" }}>
            Start the plugin, then click "View QR Code" to scan with your phone.
          </p>
        </Card>
      )}

      {/* Configuration section */}
      {hasConfig && (
        plugin.name.includes("whatsapp") ? (
          <WhatsAppConfigSection
            pluginName={plugin.name}
            config={config}
            running={plugin.running}
            onConfigSave={onConfigSave}
          />
        ) : (
          <Card className="mb-6">
            <div className="flex items-center gap-2 mb-4">
              <Settings2 size={14} style={{ color: "var(--text-muted)" }} />
              <h3
                className="text-[11px] font-bold uppercase tracking-[0.1em]"
                style={{ color: "var(--text-muted)" }}
              >
                Configuration
              </h3>
            </div>
            <div className="flex flex-col gap-3">
              {configKeys.map((key) => (
                <ConfigField
                  key={key}
                  fieldKey={key}
                  value={config[key]}
                  onSave={(value) => onConfigSave(plugin.name, key, value)}
                />
              ))}
            </div>
          </Card>
        )
      )}

      {/* Logs section (only for channel plugins that can run) */}
      {plugin.pluginType === "channel" && (
        <Card>
          <div className="flex items-center justify-between mb-4">
            <div className="flex items-center gap-2">
              <ScrollText size={14} style={{ color: "var(--text-muted)" }} />
              <h3
                className="text-[11px] font-bold uppercase tracking-[0.1em]"
                style={{ color: "var(--text-muted)" }}
              >
                Logs
              </h3>
            </div>
            <Button
              variant="ghost"
              size="sm"
              onClick={() => onRefreshLogs(plugin.name)}
              disabled={logsRefreshing}
            >
              <RefreshCw
                size={12}
                className={logsRefreshing ? "animate-spin" : ""}
              />
              Refresh
            </Button>
          </div>
          <pre
            ref={logsEndRef}
            className="text-xs p-4 rounded-xl overflow-auto"
            style={{
              fontFamily:
                "'JetBrains Mono', 'Fira Code', 'SF Mono', 'Cascadia Code', monospace",
              minHeight: "300px",
              maxHeight: "500px",
              width: "100%",
              background: "var(--bg-base)",
              color: "var(--text-secondary)",
              border: "1px solid var(--border)",
              boxShadow: "inset 0 2px 4px rgba(0,0,0,0.2)",
              lineHeight: "1.6",
              whiteSpace: "pre",
              margin: 0,
            }}
          >
            {logs || "No logs available"}
          </pre>
        </Card>
      )}
    </div>
  );
}

// ===========================================================================
// WhatsApp Config Section
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

// Sorted by longest prefix first for correct matching
const COUNTRY_CODE_PREFIXES = COUNTRY_CODES
  .map((c) => ({ prefix: c.value, display: c.code }))
  .sort((a, b) => b.prefix.length - a.prefix.length);

function formatPhoneNumber(num: string): string {
  for (const { prefix, display } of COUNTRY_CODE_PREFIXES) {
    if (num.startsWith(prefix)) {
      return `${display} ${num.slice(prefix.length)}`;
    }
  }
  return `+${num}`;
}

interface WhatsAppConfigSectionProps {
  pluginName: string;
  config: Record<string, unknown>;
  running: boolean;
  onConfigSave: (pluginName: string, key: string, value: string) => void;
}

function WhatsAppConfigSection({
  pluginName,
  config,
  running,
  onConfigSave,
}: WhatsAppConfigSectionProps) {
  const { toast } = useToast();

  // --- Bridge URL state ---
  const bridgeUrl = (config.bridgeUrl as string) ?? "";
  const [localBridgeUrl, setLocalBridgeUrl] = useState(bridgeUrl);
  useEffect(() => setLocalBridgeUrl((config.bridgeUrl as string) ?? ""), [config.bridgeUrl]);

  // --- Prefix state ---
  const prefixValue = config.prefix;
  const prefixStr = prefixValue === null || prefixValue === undefined ? "" : String(prefixValue);
  const [localPrefix, setLocalPrefix] = useState(prefixStr);
  useEffect(() => {
    const v = config.prefix;
    setLocalPrefix(v === null || v === undefined ? "" : String(v));
  }, [config.prefix]);

  // --- Groups state ---
  const [groups, setGroups] = useState<WhatsAppGroup[]>([]);
  const [groupsLoading, setGroupsLoading] = useState(false);
  const [groupsFetched, setGroupsFetched] = useState(false);
  const allowedGroups: string[] = Array.isArray(config.allowedGroups)
    ? (config.allowedGroups as string[])
    : [];

  // --- Whitelist state ---
  const whitelist: string[] = Array.isArray(config.whitelist)
    ? (config.whitelist as string[])
    : [];
  const [showAddPhone, setShowAddPhone] = useState(false);
  const [phoneCountry, setPhoneCountry] = useState("54");
  const [phoneNumber, setPhoneNumber] = useState("");

  // --- Transcription state ---
  const transcriptionBackend = (config.transcriptionBackend as string) ?? "openai";
  const whisperUrl = (config.whisperUrl as string) ?? "http://localhost:8787";
  const [localWhisperUrl, setLocalWhisperUrl] = useState(whisperUrl);
  useEffect(() => setLocalWhisperUrl((config.whisperUrl as string) ?? "http://localhost:8787"), [config.whisperUrl]);

  // --- Helpers ---
  const saveField = (key: string, value: unknown) => {
    onConfigSave(pluginName, key, JSON.stringify(value));
  };

  const handleFetchGroups = async () => {
    setGroupsLoading(true);
    try {
      const result = await fetchWhatsappGroups(pluginName);
      setGroups(result);
      setGroupsFetched(true);
      if (result.length === 0) {
        toast("info", "No groups found. Is the connector running?");
      }
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
    if (whitelist.includes(full)) {
      toast("info", "This number is already in the whitelist");
      return;
    }
    saveField("whitelist", [...whitelist, full]);
    setPhoneNumber("");
    setShowAddPhone(false);
  };

  const handleRemovePhone = (num: string) => {
    saveField("whitelist", whitelist.filter((n) => n !== num));
  };

  // Remaining config keys (not managed by custom UI)
  const remainingKeys = Object.keys(config).filter(
    (k) => !WHATSAPP_MANAGED_KEYS.includes(k),
  );

  return (
    <>
      {/* Bridge URL */}
      <Card className="mb-6">
        <div className="flex items-center gap-2 mb-4">
          <Settings2 size={14} style={{ color: "var(--text-muted)" }} />
          <h3
            className="text-[11px] font-bold uppercase tracking-[0.1em]"
            style={{ color: "var(--text-muted)" }}
          >
            Bridge URL
          </h3>
        </div>
        <Input
          value={localBridgeUrl}
          placeholder="http://localhost:3000"
          onChange={(e) => setLocalBridgeUrl(e.target.value)}
          onBlur={() => {
            if (localBridgeUrl !== bridgeUrl) {
              saveField("bridgeUrl", localBridgeUrl);
            }
          }}
          onKeyDown={(e) => {
            if (e.key === "Enter" && localBridgeUrl !== bridgeUrl) {
              saveField("bridgeUrl", localBridgeUrl);
            }
          }}
        />
      </Card>

      {/* Prefix */}
      <Card className="mb-6">
        <div className="flex items-center gap-2 mb-3">
          <Settings2 size={14} style={{ color: "var(--text-muted)" }} />
          <h3
            className="text-[11px] font-bold uppercase tracking-[0.1em]"
            style={{ color: "var(--text-muted)" }}
          >
            Prefix
          </h3>
        </div>
        <Input
          value={localPrefix}
          placeholder="No prefix (all messages processed)"
          onChange={(e) => setLocalPrefix(e.target.value)}
          onBlur={() => {
            const next = localPrefix.trim() || null;
            const prev = config.prefix === undefined ? null : config.prefix;
            if (next !== prev) {
              saveField("prefix", next);
            }
          }}
          onKeyDown={(e) => {
            if (e.key === "Enter") {
              const next = localPrefix.trim() || null;
              const prev = config.prefix === undefined ? null : config.prefix;
              if (next !== prev) {
                saveField("prefix", next);
              }
            }
          }}
        />
        <p className="text-xs mt-2" style={{ color: "var(--text-muted)" }}>
          When set, only messages starting with this prefix will be processed.
        </p>
      </Card>

      {/* Transcription Backend */}
      <Card className="mb-6">
        <div className="flex items-center gap-2 mb-3">
          <Settings2 size={14} style={{ color: "var(--text-muted)" }} />
          <h3
            className="text-[11px] font-bold uppercase tracking-[0.1em]"
            style={{ color: "var(--text-muted)" }}
          >
            Audio Transcription
          </h3>
        </div>
        <Select
          label="Backend"
          value={transcriptionBackend}
          onChange={(e) => saveField("transcriptionBackend", e.target.value)}
          options={[
            { value: "openai", label: "OpenAI API" },
            { value: "local", label: "Local (whisper.cpp)" },
          ]}
        />
        {transcriptionBackend === "local" && (
          <div className="mt-3">
            <Input
              label="Whisper Server URL"
              value={localWhisperUrl}
              placeholder="http://localhost:8787"
              onChange={(e) => setLocalWhisperUrl(e.target.value)}
              onBlur={() => {
                if (localWhisperUrl !== whisperUrl) {
                  saveField("whisperUrl", localWhisperUrl);
                }
              }}
              onKeyDown={(e) => {
                if (e.key === "Enter" && localWhisperUrl !== whisperUrl) {
                  saveField("whisperUrl", localWhisperUrl);
                }
              }}
            />
            <p className="text-xs mt-2" style={{ color: "var(--text-muted)" }}>
              URL of the local whisper.cpp server. The whisper plugin auto-starts if installed.
            </p>
          </div>
        )}
        {transcriptionBackend === "openai" && (
          <p className="text-xs mt-2" style={{ color: "var(--text-muted)" }}>
            Uses OpenAI Whisper API. Requires OPENAI_API_KEY in credentials.
          </p>
        )}
      </Card>

      {/* Allowed Groups */}
      <Card className="mb-6">
        <div className="flex items-center justify-between mb-4">
          <div className="flex items-center gap-2">
            <Users size={14} style={{ color: "var(--text-muted)" }} />
            <h3
              className="text-[11px] font-bold uppercase tracking-[0.1em]"
              style={{ color: "var(--text-muted)" }}
            >
              Allowed Groups
            </h3>
            {allowedGroups.length > 0 && (
              <Badge variant="neutral">{allowedGroups.length} selected</Badge>
            )}
          </div>
          <Button
            variant="secondary"
            size="sm"
            onClick={handleFetchGroups}
            disabled={!running || groupsLoading}
          >
            {groupsLoading ? (
              <Loader2 size={12} className="animate-spin" />
            ) : (
              <RefreshCw size={12} />
            )}
            {groupsLoading ? "Fetching..." : "Fetch Groups"}
          </Button>
        </div>
        {!running && !groupsFetched && (
          <p className="text-xs" style={{ color: "var(--text-muted)" }}>
            Start the plugin to fetch available groups.
          </p>
        )}
        {groupsFetched && groups.length === 0 && (
          <p className="text-xs" style={{ color: "var(--text-muted)" }}>
            No groups found. Make sure the WhatsApp connector is running and authenticated.
          </p>
        )}
        {groups.length > 0 && (
          <div
            className="flex flex-col gap-1 rounded-xl overflow-auto"
            style={{
              maxHeight: 280,
              background: "var(--bg-base)",
              border: "1px solid var(--border)",
              padding: 4,
            }}
          >
            {groups.map((group) => (
              <label
                key={group.id}
                className="flex items-center gap-3 px-3 py-2 rounded-lg cursor-pointer transition-all"
                style={{ transitionDuration: "120ms" }}
                onMouseEnter={(e) => {
                  e.currentTarget.style.background = "var(--bg-hover)";
                }}
                onMouseLeave={(e) => {
                  e.currentTarget.style.background = "transparent";
                }}
              >
                <input
                  type="checkbox"
                  checked={allowedGroups.includes(group.id)}
                  onChange={() => handleToggleGroup(group.id)}
                  style={{ accentColor: "#a1a1aa" }}
                />
                <div className="flex-1 min-w-0">
                  <span
                    className="text-sm font-medium block truncate"
                    style={{ color: "var(--text-primary)" }}
                  >
                    {group.subject}
                  </span>
                  <span className="text-xs" style={{ color: "var(--text-muted)" }}>
                    {group.participants != null ? `${group.participants} members` : "group"}
                  </span>
                </div>
                <span
                  className="text-xs font-mono shrink-0"
                  style={{ color: "var(--text-muted)", opacity: 0.6 }}
                >
                  {group.id.length > 24 ? group.id.slice(0, 24) + "..." : group.id}
                </span>
              </label>
            ))}
          </div>
        )}
      </Card>

      {/* Whitelist (Phone Numbers) */}
      <Card className="mb-6">
        <div className="flex items-center justify-between mb-4">
          <div className="flex items-center gap-2">
            <Phone size={14} style={{ color: "var(--text-muted)" }} />
            <h3
              className="text-[11px] font-bold uppercase tracking-[0.1em]"
              style={{ color: "var(--text-muted)" }}
            >
              Whitelist
            </h3>
            {whitelist.length > 0 && (
              <Badge variant="neutral">{whitelist.length}</Badge>
            )}
          </div>
          {!showAddPhone && (
            <Button
              variant="secondary"
              size="sm"
              onClick={() => setShowAddPhone(true)}
            >
              <Plus size={12} />
              Add Phone Number
            </Button>
          )}
        </div>

        {/* Phone chips */}
        {whitelist.length > 0 && (
          <div className="flex flex-wrap gap-2 mb-3">
            {whitelist.map((num) => (
              <span
                key={num}
                className="inline-flex items-center gap-1.5 px-3 py-1.5 rounded-lg text-xs font-mono"
                style={{
                  background: "var(--bg-tertiary)",
                  border: "1px solid var(--border)",
                  color: "var(--text-primary)",
                }}
              >
                {formatPhoneNumber(num)}
                <button
                  className="inline-flex items-center justify-center rounded cursor-pointer border-none p-0 transition-all"
                  style={{
                    background: "transparent",
                    color: "var(--text-muted)",
                    width: 16,
                    height: 16,
                    transitionDuration: "120ms",
                  }}
                  onClick={() => handleRemovePhone(num)}
                  onMouseEnter={(e) => {
                    e.currentTarget.style.color = "var(--danger)";
                  }}
                  onMouseLeave={(e) => {
                    e.currentTarget.style.color = "var(--text-muted)";
                  }}
                >
                  <X size={12} />
                </button>
              </span>
            ))}
          </div>
        )}

        {whitelist.length === 0 && !showAddPhone && (
          <p className="text-xs" style={{ color: "var(--text-muted)" }}>
            No phone numbers in the whitelist. All numbers are allowed.
          </p>
        )}

        {/* Add phone form */}
        {showAddPhone && (
          <div
            className="flex items-end gap-2 p-3 rounded-xl"
            style={{
              background: "var(--bg-base)",
              border: "1px solid var(--border)",
            }}
          >
            <div style={{ width: 180 }}>
              <Select
                options={COUNTRY_CODES.map((c) => ({
                  value: c.value,
                  label: c.label,
                }))}
                value={phoneCountry}
                onChange={(e) => setPhoneCountry(e.target.value)}
              />
            </div>
            <div className="flex-1">
              <Input
                value={phoneNumber}
                placeholder="Phone number"
                onChange={(e) => setPhoneNumber(e.target.value.replace(/\D/g, ""))}
                onKeyDown={(e) => {
                  if (e.key === "Enter") handleAddPhone();
                  if (e.key === "Escape") setShowAddPhone(false);
                }}
              />
            </div>
            <Button size="sm" onClick={handleAddPhone} disabled={!phoneNumber.trim()}>
              <Plus size={12} />
              Add
            </Button>
            <Button
              variant="ghost"
              size="sm"
              onClick={() => {
                setShowAddPhone(false);
                setPhoneNumber("");
              }}
            >
              Cancel
            </Button>
          </div>
        )}
      </Card>

      {/* Remaining config fields */}
      {remainingKeys.length > 0 && (
        <Card className="mb-6">
          <div className="flex items-center gap-2 mb-4">
            <Settings2 size={14} style={{ color: "var(--text-muted)" }} />
            <h3
              className="text-[11px] font-bold uppercase tracking-[0.1em]"
              style={{ color: "var(--text-muted)" }}
            >
              Other Settings
            </h3>
          </div>
          <div className="flex flex-col gap-3">
            {remainingKeys.map((key) => (
              <ConfigField
                key={key}
                fieldKey={key}
                value={config[key]}
                onSave={(value) => onConfigSave(pluginName, key, value)}
              />
            ))}
          </div>
        </Card>
      )}
    </>
  );
}

// ===========================================================================
// Config Field (editable key-value row)
// ===========================================================================

interface ConfigFieldProps {
  fieldKey: string;
  value: unknown;
  onSave: (value: string) => void;
}

function ConfigField({ fieldKey, value, onSave }: ConfigFieldProps) {
  const displayValue = typeof value === "string" ? value : JSON.stringify(value);
  const [localValue, setLocalValue] = useState(displayValue);
  const [dirty, setDirty] = useState(false);

  // Sync from parent when config refreshes after a save
  useEffect(() => {
    const next = typeof value === "string" ? value : JSON.stringify(value);
    setLocalValue(next);
    setDirty(false);
  }, [value]);

  const handleChange = (e: React.ChangeEvent<HTMLInputElement>) => {
    setLocalValue(e.target.value);
    setDirty(e.target.value !== displayValue);
  };

  const handleBlur = () => {
    if (dirty) {
      onSave(localValue);
    }
  };

  const handleKeyDown = (e: React.KeyboardEvent) => {
    if (e.key === "Enter" && dirty) {
      onSave(localValue);
    }
  };

  return (
    <div className="flex items-center gap-3">
      <span
        className="text-xs font-mono w-40 shrink-0 px-2.5 py-1.5 rounded-lg truncate"
        style={{
          color: "var(--text-secondary)",
          background: "var(--bg-tertiary)",
        }}
        title={fieldKey}
      >
        {fieldKey}
      </span>
      <div className="flex-1">
        <Input
          value={localValue}
          onChange={handleChange}
          onBlur={handleBlur}
          onKeyDown={handleKeyDown}
        />
      </div>
    </div>
  );
}

// ===========================================================================
// Registry Modal
// ===========================================================================

interface RegistryModalProps {
  remotePlugins: RemotePlugin[];
  loading: boolean;
  installingRemote: string | null;
  onInstall: (name: string) => void;
  onClose: () => void;
}

function RegistryModal({
  remotePlugins,
  loading,
  installingRemote,
  onInstall,
  onClose,
}: RegistryModalProps) {
  // Close on Escape
  useEffect(() => {
    const handler = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose();
    };
    window.addEventListener("keydown", handler);
    return () => window.removeEventListener("keydown", handler);
  }, [onClose]);

  return (
    <div
      className="fixed inset-0 flex items-center justify-center z-40"
      style={{
        background: "rgba(0,0,0,0.7)",
        backdropFilter: "blur(8px)",
        animation: "fadeIn 0.2s ease-out",
      }}
      onClick={onClose}
    >
      <div
        className="rounded-2xl p-6 w-[640px] max-h-[75vh] flex flex-col"
        style={{
          background: "rgba(20, 20, 20, 0.98)",
          border: "1px solid var(--border)",
          boxShadow: "var(--shadow-lg)",
          animation: "scaleIn 0.25s cubic-bezier(0.4, 0, 0.2, 1)",
        }}
        onClick={(e) => e.stopPropagation()}
      >
        {/* Modal header */}
        <div className="flex items-center justify-between mb-5 shrink-0">
          <div className="flex items-center gap-2.5">
            <h3
              className="text-base font-bold"
              style={{ color: "var(--text-primary)" }}
            >
              Plugin Registry
            </h3>
            {!loading && (
              <Badge variant="neutral">{remotePlugins.length} available</Badge>
            )}
          </div>
          <button
            className="p-1.5 rounded-xl cursor-pointer border-none transition-all"
            style={{
              background: "transparent",
              color: "var(--text-muted)",
              transitionDuration: "150ms",
            }}
            onClick={onClose}
            onMouseEnter={(e) => {
              e.currentTarget.style.background = "var(--bg-hover)";
              e.currentTarget.style.color = "var(--text-primary)";
            }}
            onMouseLeave={(e) => {
              e.currentTarget.style.background = "transparent";
              e.currentTarget.style.color = "var(--text-muted)";
            }}
          >
            <X size={18} />
          </button>
        </div>

        {/* Modal body */}
        <div className="overflow-y-auto flex-1">
          {loading ? (
            <div
              className="flex items-center justify-center gap-2 py-12"
              style={{ color: "var(--text-muted)" }}
            >
              <Loader2 size={18} className="animate-spin" />
              <span className="text-sm">Loading registry...</span>
            </div>
          ) : remotePlugins.length === 0 ? (
            <div
              className="py-12 text-center text-sm"
              style={{ color: "var(--text-muted)" }}
            >
              No plugins available in the registry.
            </div>
          ) : (
            <div className="flex flex-col gap-3">
              {remotePlugins.map((rp) => (
                <RegistryPluginRow
                  key={rp.name}
                  plugin={rp}
                  installing={installingRemote === rp.name}
                  anyInstalling={installingRemote !== null}
                  onInstall={onInstall}
                />
              ))}
            </div>
          )}
        </div>
      </div>
    </div>
  );
}

// ===========================================================================
// Registry Plugin Row
// ===========================================================================

interface RegistryPluginRowProps {
  plugin: RemotePlugin;
  installing: boolean;
  anyInstalling: boolean;
  onInstall: (name: string) => void;
}

function RegistryPluginRow({
  plugin,
  installing,
  anyInstalling,
  onInstall,
}: RegistryPluginRowProps) {
  return (
    <div
      className="flex items-center justify-between p-4 rounded-xl transition-all"
      style={{
        background: "var(--surface-glass)",
        border: "1px solid var(--border)",
        transitionDuration: "150ms",
      }}
      onMouseEnter={(e) => {
        e.currentTarget.style.borderColor = "var(--border-hover)";
        e.currentTarget.style.background = "rgba(255,255,255,0.05)";
      }}
      onMouseLeave={(e) => {
        e.currentTarget.style.borderColor = "var(--border)";
        e.currentTarget.style.background = "var(--surface-glass)";
      }}
    >
      <div className="min-w-0 flex-1 mr-3">
        <div className="flex items-center gap-2 flex-wrap">
          <span
            className="font-semibold text-sm"
            style={{ color: "var(--text-primary)" }}
          >
            {plugin.name}
          </span>
          <span className="text-xs font-mono" style={{ color: "var(--text-muted)" }}>
            v{plugin.version}
          </span>
          {plugin.installed && <Badge variant="success">Installed</Badge>}
          {!plugin.available && <Badge variant="warning">Unavailable</Badge>}
        </div>
        {plugin.description && (
          <p
            className="text-xs mt-1 leading-relaxed"
            style={{ color: "var(--text-secondary)" }}
          >
            {plugin.description}
          </p>
        )}
      </div>
      {!plugin.installed && plugin.available && (
        <Button
          size="sm"
          onClick={() => onInstall(plugin.name)}
          disabled={anyInstalling}
        >
          {installing ? (
            <Loader2 size={12} className="animate-spin" />
          ) : (
            <Download size={12} />
          )}
          {installing ? "Installing..." : "Install"}
        </Button>
      )}
    </div>
  );
}
