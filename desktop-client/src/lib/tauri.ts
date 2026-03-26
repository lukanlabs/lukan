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
  FileContent,
  WorkerSummary,
  WorkerDetail,
  WorkerDefinition,
  WorkerRun,
  WorkerCreateInput,
  WorkerUpdateInput,
  PipelineSummary,
  PipelineDetail,
  PipelineDefinition,
  PipelineRun,
  PipelineCreateInput,
  PipelineUpdateInput,
  ApprovalRequest,
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
export const installPlugin = async (path: string): Promise<string> => {
  const res = await getTransport().call<string | { message: string }>("install_plugin", { path });
  return typeof res === "object" && res !== null ? (res as { message: string }).message : (res as string);
};
export const installRemotePlugin = async (name: string): Promise<string> => {
  const res = await getTransport().call<string | { message: string }>("install_remote_plugin", { name });
  return typeof res === "object" && res !== null ? (res as { message: string }).message : (res as string);
};
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
export const startWebUi = (port: number, cwd?: string) =>
  getTransport().call<void>("start_web_ui", { port, cwd });
export const stopWebUi = () => getTransport().call<void>("stop_web_ui");

// Project
export interface RecentProject {
  path: string;
  name: string;
  lastOpened: string;
}
export const setProjectCwd = (path: string) =>
  getTransport().call<void>("set_project_cwd", { path });
export const getRecentProjects = () =>
  getTransport().call<RecentProject[]>("get_recent_projects");
export const addRecentProject = (path: string) =>
  getTransport().call<void>("add_recent_project", { path });
export const pickDirectory = () =>
  getTransport().call<string | null>("pick_directory");

// Chat — tab management
export const createAgentTab = (cwd?: string) =>
  getTransport().call<string>("create_agent_tab", cwd ? { cwd } : undefined);
export const destroyAgentTab = (sessionId: string) =>
  getTransport().call<void>("destroy_agent_tab", { sessionId });
export const renameAgentTab = (sessionId: string, label: string) =>
  getTransport().call<void>("rename_agent_tab", { sessionId, label });

export interface AgentTabState {
  id: string;
  label?: string;
  sessionId?: string;
}
export interface AgentTabsFile {
  tabs: AgentTabState[];
  activeTabId?: string;
}
export const loadAgentTabs = () =>
  getTransport().call<AgentTabsFile>("load_agent_tabs");
export const saveAgentTabs = (state: AgentTabsFile) =>
  getTransport().call<void>("save_agent_tabs", { state });

// Chat — global
export const initializeChat = () =>
  getTransport().call<InitResponse>("initialize_chat");
export const setPermissionMode = (mode: string) =>
  getTransport().call<void>("set_permission_mode", { mode });
export const listSessions = () =>
  getTransport().call<SessionSummary[]>("list_sessions");
export const deleteSession = (id: string) =>
  getTransport().call<boolean>("delete_session", { id });
export const deleteAllSessions = () =>
  getTransport().call<number>("delete_all_sessions");
export const listTasks = () =>
  getTransport().call<TaskInfo[]>("list_tasks");

// Chat — per-session (scoped by sessionId)
export const sendMessage = (sessionId: string, content: string) =>
  getTransport().call<void>("send_message", { sessionId, content });
export const cancelStream = (sessionId: string) =>
  getTransport().call<void>("cancel_stream", { sessionId });
export const approveTools = (sessionId: string, approvedIds: string[]) =>
  getTransport().call<void>("approve_tools", { sessionId, approvedIds });
export const alwaysAllowTools = (
  sessionId: string,
  approvedIds: string[],
  tools: ToolApprovalRequest[],
) =>
  getTransport().call<void>("always_allow_tools", { sessionId, approvedIds, tools });
export const denyAllTools = (sessionId: string) =>
  getTransport().call<void>("deny_all_tools", { sessionId });
export const acceptPlan = (
  sessionId: string,
  tasks?: Array<{ title: string; detail: string }>,
) => getTransport().call<void>("accept_plan", { sessionId, tasks });
export const rejectPlan = (sessionId: string, feedback: string) =>
  getTransport().call<void>("reject_plan", { sessionId, feedback });
export const answerQuestion = (sessionId: string, answer: string) =>
  getTransport().call<void>("answer_question", { sessionId, answer });
export const loadSession = (sessionId: string, id: string) =>
  getTransport().call<InitResponse>("load_session", { sessionId, id });
export const newSession = (sessionId: string) =>
  getTransport().call<InitResponse>("new_session", { sessionId });

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
export const terminalReconnect = (sessionId: string) =>
  getTransport().call<TerminalSessionInfo & { scrollback?: string }>(
    "terminal_reconnect",
    { sessionId },
  );
export const terminalRename = (sessionId: string, name: string) =>
  getTransport().call<void>("terminal_rename", { sessionId, name });
export const onTerminalOutput = (
  sessionId: string,
  cb: (event: TerminalOutputEvent) => void,
) =>
  getTransport().subscribe(
    `terminal-output-${sessionId}`,
    cb as (p: unknown) => void,
  );
export const onTerminalSessionsRecovered = (
  cb: (sessions: TerminalSessionInfo[]) => void,
) =>
  getTransport().subscribe(
    "terminal-sessions-recovered",
    cb as (p: unknown) => void,
  );

// Background processes
export const listBgProcesses = (sessionId?: string) =>
  getTransport().call<BgProcessInfo[]>("list_bg_processes", { sessionId });
export const getBgProcessLog = (pid: number, maxLines: number) =>
  getTransport().call<string | null>("get_bg_process_log", { pid, maxLines });
export const killBgProcess = (pid: number) =>
  getTransport().call<boolean>("kill_bg_process", { pid });
export const sendToBackground = (sessionId: string) =>
  getTransport().call<boolean>("send_to_background", { sessionId });

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
export const readFile = (path: string) =>
  getTransport().call<FileContent>("read_file", { path });
export const writeFile = (path: string, content: string) =>
  getTransport().call<void>("write_file", { path, content });
export const openInEditor = (path: string, editor?: string) =>
  getTransport().call<void>("open_in_editor", { path, editor });
export const getCwd = () => getTransport().call<string>("get_cwd");
export const setActiveTab = (tabId: string) =>
  getTransport().call<void>("set_active_tab", { tabId });
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

// Pipelines
export const listPipelines = () =>
  getTransport().call<PipelineSummary[]>("list_pipelines");
export const createPipeline = (input: PipelineCreateInput) =>
  getTransport().call<PipelineDefinition>("create_pipeline", { pipeline: input });
export const updatePipeline = (id: string, patch: PipelineUpdateInput) =>
  getTransport().call<PipelineDefinition>("update_pipeline", { id, patch });
export const deletePipeline = (id: string) =>
  getTransport().call<boolean>("delete_pipeline", { id });
export const togglePipeline = (id: string, enabled: boolean) =>
  getTransport().call<PipelineDefinition>("toggle_pipeline", { id, enabled });
export const getPipelineDetail = (id: string) =>
  getTransport().call<PipelineDetail>("get_pipeline_detail", { id });
export const triggerPipeline = (id: string, input?: string) =>
  getTransport().call<void>("trigger_pipeline", { id, input: input ?? null });
export const cancelPipeline = (id: string) =>
  getTransport().call<void>("cancel_pipeline", { id });
export const getPipelineRun = (pipelineId: string, runId: string) =>
  getTransport().call<PipelineRun>("get_pipeline_run", { pipelineId, runId });
export const onPipelineNotification = (cb: (data: unknown) => void) =>
  getTransport().subscribe("pipeline-notification", cb);

// Pipeline approvals
export const listPendingApprovals = () =>
  getTransport().call<ApprovalRequest[]>("list_pending_approvals");
export const approveApproval = (id: string, comment?: string) =>
  getTransport().call<ApprovalRequest>("approve_approval", { id, comment });
export const rejectApproval = (id: string, comment?: string) =>
  getTransport().call<ApprovalRequest>("reject_approval", { id, comment });

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

// Event listeners (session-scoped)
export const onStreamEvent = (sessionId: string, cb: (payload: string) => void) =>
  getTransport().subscribe(`stream-event-${sessionId}`, cb as (p: unknown) => void);
export const onTurnComplete = (sessionId: string, cb: (payload: string) => void) =>
  getTransport().subscribe(`turn-complete-${sessionId}`, cb as (p: unknown) => void);

// Event listeners (global)
export const onWorkerNotification = (cb: (payload: string) => void) =>
  getTransport().subscribe("worker-notification", cb as (p: unknown) => void);
