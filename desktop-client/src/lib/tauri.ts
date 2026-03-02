export { IS_TAURI, initTransport } from "./transport";
import { getTransport } from "./transport";
import type {
  AppConfig,
  Credentials,
  ProviderStatus,
  ProviderInfo,
  FetchedModel,
  PluginInfo,
  PluginViewEnvelope,
  RemotePlugin,
  PluginCommand,
  PluginToolsInfo,
  ConfigFieldSchemaDto,
  AuthDeclarationDto,
  WebUiStatus,
  InitResponse,
  SessionSummary,
  SystemEvent,
  ToolApprovalRequest,
  TaskInfo,
  TerminalSessionInfo,
  TerminalOutputEvent,
  BgProcessInfo,
  BrowserStatus,
  BrowserTab,
  DirectoryListing,
  WorkerSummary,
  WorkerDetail,
  WorkerDefinition,
  WorkerRun,
  WorkerCreateInput,
  WorkerUpdateInput,
  TranscriptionStatus,
} from "./types";

// Config
export const getConfig = () => getTransport().call<AppConfig>("get_config");
export const saveConfig = (config: AppConfig) =>
  getTransport().call<void>("save_config", { config });
export const getConfigValue = (key: string) =>
  getTransport().call<unknown | null>("get_config_value", { key });
export const setConfigValue = (key: string, value: unknown) =>
  getTransport().call<void>("set_config_value", { key, value });
export const listTools = () =>
  getTransport().call<{ name: string; source: string | null }[]>("list_tools");

// Credentials
export const getCredentials = () =>
  getTransport().call<Credentials>("get_credentials");
export const saveCredentials = (credentials: Credentials) =>
  getTransport().call<void>("save_credentials", { credentials });
export const getProviderStatus = () =>
  getTransport().call<ProviderStatus[]>("get_provider_status");
export const testProvider = (provider: string) =>
  getTransport().call<string>("test_provider", { provider });

// Plugins
export const listPlugins = () =>
  getTransport().call<PluginInfo[]>("list_plugins");
export const installPlugin = (path: string) =>
  getTransport().call<string>("install_plugin", { path });
export const installRemotePlugin = (name: string) =>
  getTransport().call<string>("install_remote_plugin", { name });
export const removePlugin = (name: string) =>
  getTransport().call<void>("remove_plugin", { name });
export const startPlugin = (name: string) =>
  getTransport().call<void>("start_plugin", { name });
export const stopPlugin = (name: string) =>
  getTransport().call<void>("stop_plugin", { name });
export const restartPlugin = (name: string) =>
  getTransport().call<void>("restart_plugin", { name });
export const getPluginConfig = (name: string) =>
  getTransport().call<Record<string, unknown>>("get_plugin_config", { name });
export const setPluginConfigField = (
  name: string,
  key: string,
  value: unknown,
) =>
  getTransport().call<void>("set_plugin_config_field", { name, key, value });
export const getPluginLogs = (name: string, lines: number) =>
  getTransport().call<string>("get_plugin_logs", { name, lines });
export const listRemotePlugins = () =>
  getTransport().call<RemotePlugin[]>("list_remote_plugins");
export const getPluginManifestInfo = (name: string) =>
  getTransport().call<{ config: Record<string, ConfigFieldSchemaDto>; auth: AuthDeclarationDto | null }>("get_plugin_manifest_info", { name });
export const getPluginAuthQr = (name: string) =>
  getTransport().call<string | null>("get_plugin_auth_qr", { name });
export const checkPluginAuth = (name: string) =>
  getTransport().call<boolean>("check_plugin_auth", { name });
export const getPluginCommands = (name: string) =>
  getTransport().call<PluginCommand[]>("get_plugin_commands", { name });
export const runPluginCommand = (name: string, command: string) =>
  getTransport().call<string>("run_plugin_command", { name, command });
export const getPluginManifestTools = (name: string) =>
  getTransport().call<PluginToolsInfo>("get_plugin_manifest_tools", { name });
export const getPluginViewData = (pluginName: string, viewId: string) =>
  getTransport().call<PluginViewEnvelope | null>("get_plugin_view_data", {
    pluginName,
    viewId,
  });

// Providers
export const listProviders = () =>
  getTransport().call<ProviderInfo[]>("list_providers");
export const getModels = () => getTransport().call<string[]>("get_models");
export const fetchProviderModels = (provider: string) =>
  getTransport().call<FetchedModel[]>("fetch_provider_models", { provider });
export const setActiveProvider = (provider: string, model?: string) =>
  getTransport().call<void>("set_active_provider", { provider, model });
export const addModel = (entry: string) =>
  getTransport().call<void>("add_model", { entry });
export const setProviderModels = (
  provider: string,
  entries: string[],
  visionIds: string[],
) =>
  getTransport().call<void>("set_provider_models", {
    provider,
    entries,
    visionIds,
  });

// Memory
export const getGlobalMemory = () =>
  getTransport().call<string>("get_global_memory");
export const saveGlobalMemory = (content: string) =>
  getTransport().call<void>("save_global_memory", { content });
export const getProjectMemory = (path: string) =>
  getTransport().call<string>("get_project_memory", { path });
export const saveProjectMemory = (path: string, content: string) =>
  getTransport().call<void>("save_project_memory", { path, content });
export const isProjectMemoryActive = (path: string) =>
  getTransport().call<boolean>("is_project_memory_active", { path });
export const toggleProjectMemory = (path: string, active: boolean) =>
  getTransport().call<void>("toggle_project_memory", { path, active });

// Web UI
export const getWebUiStatus = () =>
  getTransport().call<WebUiStatus>("get_web_ui_status");
export const startWebUi = (port: number) =>
  getTransport().call<void>("start_web_ui", { port });
export const stopWebUi = () => getTransport().call<void>("stop_web_ui");

// Chat
export const initializeChat = () =>
  getTransport().call<InitResponse>("initialize_chat");
export const sendMessage = (content: string) =>
  getTransport().call<void>("send_message", { content });
export const cancelStream = () =>
  getTransport().call<void>("cancel_stream");
export const approveTools = (approvedIds: string[]) =>
  getTransport().call<void>("approve_tools", { approvedIds });
export const alwaysAllowTools = (
  approvedIds: string[],
  tools: ToolApprovalRequest[],
) =>
  getTransport().call<void>("always_allow_tools", { approvedIds, tools });
export const denyAllTools = () =>
  getTransport().call<void>("deny_all_tools");
export const acceptPlan = (
  tasks?: Array<{ title: string; detail: string }>,
) => getTransport().call<void>("accept_plan", { tasks });
export const rejectPlan = (feedback: string) =>
  getTransport().call<void>("reject_plan", { feedback });
export const answerQuestion = (answer: string) =>
  getTransport().call<void>("answer_question", { answer });
export const listSessions = () =>
  getTransport().call<SessionSummary[]>("list_sessions");
export const loadSession = (id: string) =>
  getTransport().call<InitResponse>("load_session", { id });
export const newSession = () =>
  getTransport().call<InitResponse>("new_session");
export const setPermissionMode = (mode: string) =>
  getTransport().call<void>("set_permission_mode", { mode });
export const listTasks = () =>
  getTransport().call<TaskInfo[]>("list_tasks");

// Terminal
export const terminalCreate = (cwd?: string, cols?: number, rows?: number) =>
  getTransport().call<TerminalSessionInfo>("terminal_create", {
    cwd,
    cols,
    rows,
  });
export const terminalInput = (sessionId: string, data: string) =>
  getTransport().call<void>("terminal_input", { sessionId, data });
export const terminalResize = (
  sessionId: string,
  cols: number,
  rows: number,
) => getTransport().call<void>("terminal_resize", { sessionId, cols, rows });
export const terminalDestroy = (sessionId: string) =>
  getTransport().call<void>("terminal_destroy", { sessionId });
export const terminalList = () =>
  getTransport().call<TerminalSessionInfo[]>("terminal_list");
export const onTerminalOutput = (
  sessionId: string,
  cb: (event: TerminalOutputEvent) => void,
) =>
  getTransport().subscribe(
    `terminal-output-${sessionId}`,
    cb as (p: unknown) => void,
  );

// Background processes
export const listBgProcesses = (sessionId?: string) =>
  getTransport().call<BgProcessInfo[]>("list_bg_processes", { sessionId });
export const getBgProcessLog = (pid: number, maxLines: number) =>
  getTransport().call<string | null>("get_bg_process_log", { pid, maxLines });
export const killBgProcess = (pid: number) =>
  getTransport().call<boolean>("kill_bg_process", { pid });
export const sendToBackground = () =>
  getTransport().call<boolean>("send_to_background");

// Browser
export const browserLaunch = (
  visible?: boolean,
  profile?: string,
  port?: number,
) =>
  getTransport().call<BrowserStatus>("browser_launch", {
    visible,
    profile,
    port,
  });
export const browserStatus = () =>
  getTransport().call<BrowserStatus>("browser_status");
export const browserNavigate = (url: string) =>
  getTransport().call<string>("browser_navigate", { url });
export const browserScreenshot = () =>
  getTransport().call<string>("browser_screenshot");
export const browserTabs = () =>
  getTransport().call<BrowserTab[]>("browser_tabs");
export const browserClose = () =>
  getTransport().call<void>("browser_close");

// Files
export const listDirectory = (path?: string) =>
  getTransport().call<DirectoryListing>("list_directory", { path });
export const openInEditor = (path: string, editor?: string) =>
  getTransport().call<void>("open_in_editor", { path, editor });
export const getCwd = () => getTransport().call<string>("get_cwd");
export const openUrl = (url: string) =>
  getTransport().call<void>("open_url", { url });

// Workers
export const listWorkers = () =>
  getTransport().call<WorkerSummary[]>("list_workers");
export const createWorker = (input: WorkerCreateInput) =>
  getTransport().call<WorkerDefinition>("create_worker", { input });
export const updateWorker = (id: string, patch: WorkerUpdateInput) =>
  getTransport().call<WorkerDefinition>("update_worker", { id, patch });
export const deleteWorker = (id: string) =>
  getTransport().call<boolean>("delete_worker", { id });
export const toggleWorker = (id: string, enabled: boolean) =>
  getTransport().call<WorkerDefinition>("toggle_worker", { id, enabled });
export const getWorkerDetail = (id: string) =>
  getTransport().call<WorkerDetail>("get_worker_detail", { id });
export const getWorkerRun = (workerId: string, runId: string) =>
  getTransport().call<WorkerRun>("get_worker_run", { workerId, runId });

// Events
export const consumePendingEvents = () =>
  getTransport().call<SystemEvent[]>("consume_pending_events");
export const getEventHistory = (count: number) =>
  getTransport().call<SystemEvent[]>("get_event_history", { count });
export const clearEventHistory = (source?: string) =>
  getTransport().call<boolean>("clear_event_history", {
    source: source ?? null,
  });

// Audio transcription
export const checkTranscriptionStatus = () =>
  getTransport().call<TranscriptionStatus>("check_transcription_status");
export const transcribeAudio = (audio: number[]) =>
  getTransport().call<string>("transcribe_audio", { audio });

// Audio recording (system-level via cpal in Tauri, MediaRecorder in web)
export const startRecording = () =>
  getTransport().call<void>("start_recording");
export const stopRecording = () =>
  getTransport().call<number[]>("stop_recording");
export const cancelRecording = () =>
  getTransport().call<void>("cancel_recording");
export const isRecording = () =>
  getTransport().call<boolean>("is_recording");
export const listAudioDevices = () =>
  getTransport().call<string[]>("list_audio_devices");

// Event listeners
export const onStreamEvent = (cb: (payload: string) => void) =>
  getTransport().subscribe("stream-event", cb as (p: unknown) => void);
export const onTurnComplete = (cb: (payload: string) => void) =>
  getTransport().subscribe("turn-complete", cb as (p: unknown) => void);
export const onWorkerNotification = (cb: (payload: string) => void) =>
  getTransport().subscribe("worker-notification", cb as (p: unknown) => void);
