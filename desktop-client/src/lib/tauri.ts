import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type {
  AppConfig,
  Credentials,
  ProviderStatus,
  ProviderInfo,
  FetchedModel,
  PluginInfo,
  RemotePlugin,
  WhatsAppGroup,
  PluginCommand,
  WebUiStatus,
  InitResponse,
  SessionSummary,
  ToolApprovalRequest,
  TerminalSessionInfo,
  TerminalOutputEvent,
} from "./types";

// Config
export const getConfig = () => invoke<AppConfig>("get_config");
export const saveConfig = (config: AppConfig) => invoke<void>("save_config", { config });
export const getConfigValue = (key: string) => invoke<unknown | null>("get_config_value", { key });
export const setConfigValue = (key: string, value: unknown) =>
  invoke<void>("set_config_value", { key, value });

// Credentials
export const getCredentials = () => invoke<Credentials>("get_credentials");
export const saveCredentials = (credentials: Credentials) =>
  invoke<void>("save_credentials", { credentials });
export const getProviderStatus = () => invoke<ProviderStatus[]>("get_provider_status");
export const testProvider = (provider: string) => invoke<string>("test_provider", { provider });

// Plugins
export const listPlugins = () => invoke<PluginInfo[]>("list_plugins");
export const installPlugin = (path: string) => invoke<string>("install_plugin", { path });
export const installRemotePlugin = (name: string) =>
  invoke<string>("install_remote_plugin", { name });
export const removePlugin = (name: string) => invoke<void>("remove_plugin", { name });
export const startPlugin = (name: string) => invoke<void>("start_plugin", { name });
export const stopPlugin = (name: string) => invoke<void>("stop_plugin", { name });
export const restartPlugin = (name: string) => invoke<void>("restart_plugin", { name });
export const getPluginConfig = (name: string) =>
  invoke<Record<string, unknown>>("get_plugin_config", { name });
export const setPluginConfigField = (name: string, key: string, value: unknown) =>
  invoke<void>("set_plugin_config_field", { name, key, value });
export const getPluginLogs = (name: string, lines: number) =>
  invoke<string>("get_plugin_logs", { name, lines });
export const listRemotePlugins = () => invoke<RemotePlugin[]>("list_remote_plugins");
export const getWhatsappQr = () => invoke<string | null>("get_whatsapp_qr");
export const checkWhatsappAuth = () => invoke<boolean>("check_whatsapp_auth");
export const fetchWhatsappGroups = (name: string) =>
  invoke<WhatsAppGroup[]>("fetch_whatsapp_groups", { name });
export const getPluginCommands = (name: string) =>
  invoke<PluginCommand[]>("get_plugin_commands", { name });
export const runPluginCommand = (name: string, command: string) =>
  invoke<string>("run_plugin_command", { name, command });

// Providers
export const listProviders = () => invoke<ProviderInfo[]>("list_providers");
export const getModels = () => invoke<string[]>("get_models");
export const fetchProviderModels = (provider: string) =>
  invoke<FetchedModel[]>("fetch_provider_models", { provider });
export const setActiveProvider = (provider: string, model?: string) =>
  invoke<void>("set_active_provider", { provider, model });
export const addModel = (entry: string) => invoke<void>("add_model", { entry });
export const setProviderModels = (provider: string, entries: string[], visionIds: string[]) =>
  invoke<void>("set_provider_models", { provider, entries, visionIds });

// Memory
export const getGlobalMemory = () => invoke<string>("get_global_memory");
export const saveGlobalMemory = (content: string) =>
  invoke<void>("save_global_memory", { content });
export const getProjectMemory = (path: string) => invoke<string>("get_project_memory", { path });
export const saveProjectMemory = (path: string, content: string) =>
  invoke<void>("save_project_memory", { path, content });
export const isProjectMemoryActive = (path: string) =>
  invoke<boolean>("is_project_memory_active", { path });
export const toggleProjectMemory = (path: string, active: boolean) =>
  invoke<void>("toggle_project_memory", { path, active });

// Web UI
export const getWebUiStatus = () => invoke<WebUiStatus>("get_web_ui_status");
export const startWebUi = (port: number) => invoke<void>("start_web_ui", { port });
export const stopWebUi = () => invoke<void>("stop_web_ui");

// Chat
export const initializeChat = () => invoke<InitResponse>("initialize_chat");
export const sendMessage = (content: string) => invoke<void>("send_message", { content });
export const cancelStream = () => invoke<void>("cancel_stream");
export const approveTools = (approvedIds: string[]) =>
  invoke<void>("approve_tools", { approvedIds });
export const alwaysAllowTools = (approvedIds: string[], tools: ToolApprovalRequest[]) =>
  invoke<void>("always_allow_tools", { approvedIds, tools });
export const denyAllTools = () => invoke<void>("deny_all_tools");
export const acceptPlan = (tasks?: Array<{ title: string; detail: string }>) =>
  invoke<void>("accept_plan", { tasks });
export const rejectPlan = (feedback: string) => invoke<void>("reject_plan", { feedback });
export const answerQuestion = (answer: string) => invoke<void>("answer_question", { answer });
export const listSessions = () => invoke<SessionSummary[]>("list_sessions");
export const loadSession = (id: string) => invoke<InitResponse>("load_session", { id });
export const newSession = () => invoke<InitResponse>("new_session");
export const setPermissionMode = (mode: string) =>
  invoke<void>("set_permission_mode", { mode });

// Terminal
export const terminalCreate = (cwd?: string, cols?: number, rows?: number) =>
  invoke<TerminalSessionInfo>("terminal_create", { cwd, cols, rows });
export const terminalInput = (sessionId: string, data: string) =>
  invoke<void>("terminal_input", { sessionId, data });
export const terminalResize = (sessionId: string, cols: number, rows: number) =>
  invoke<void>("terminal_resize", { sessionId, cols, rows });
export const terminalDestroy = (sessionId: string) =>
  invoke<void>("terminal_destroy", { sessionId });
export const terminalList = () => invoke<TerminalSessionInfo[]>("terminal_list");
export const onTerminalOutput = (
  sessionId: string,
  cb: (event: TerminalOutputEvent) => void,
): Promise<UnlistenFn> =>
  listen<TerminalOutputEvent>(`terminal-output-${sessionId}`, (e) => cb(e.payload));

// Event listeners
export const onStreamEvent = (cb: (payload: string) => void): Promise<UnlistenFn> =>
  listen<string>("stream-event", (e) => cb(e.payload));
export const onTurnComplete = (cb: (payload: string) => void): Promise<UnlistenFn> =>
  listen<string>("turn-complete", (e) => cb(e.payload));
