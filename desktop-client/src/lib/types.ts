// Mirrors Rust AppConfig
export interface AppConfig {
  provider: string;
  model?: string;
  maxTokens: number;
  temperature?: number;
  timezone?: string;
  syntaxTheme?: string;
  models?: string[];
  visionModel?: string;
  visionModels?: string[];
  openaiCompatibleBaseUrl?: string;
  openaiCompatibleProviderName?: string;
  openaiCompatibleProviderOptions?: Record<string, unknown>;
  webPassword?: string;
  webTokenTtl?: number;
  plugins?: PluginsConfig;
  browserCdpUrl?: string;
  disabledTools?: string[];
  mcpServers?: Record<string, McpServerConfig>;
}

export interface McpServerConfig {
  command: string;
  args?: string[];
  env?: Record<string, string>;
}

export interface PluginsConfig {
  enabled: string[];
  overrides: Record<string, PluginOverrides>;
}

export interface PluginOverrides {
  provider?: string;
  model?: string;
  tools?: string[];
  maxResponseLen?: number;
  autoRestart?: boolean;
}

// Mirrors Rust Credentials
export interface Credentials {
  nebiusApiKey?: string;
  anthropicApiKey?: string;
  fireworksApiKey?: string;
  githubToken?: string;
  copilotToken?: string;
  copilotClientId?: string;
  braveApiKey?: string;
  tavilyApiKey?: string;
  openaiApiKey?: string;
  codexAccessToken?: string;
  codexRefreshToken?: string;
  codexTokenExpiry?: number;
  zaiApiKey?: string;
  ollamaCloudApiKey?: string;
  openaiCompatibleApiKey?: string;
  lukanCloudApiKey?: string;
  geminiApiKey?: string;
}

export interface ProviderStatus {
  name: string;
  configured: boolean;
  defaultModel: string;
}

export interface ProviderInfo {
  name: string;
  defaultModel: string;
  active: boolean;
  currentModel?: string;
}

export interface FetchedModel {
  id: string;
  name: string;
}

export interface PluginActivityBar {
  icon: string;
  label: string;
}

export interface ViewDeclaration {
  id: string;
  viewType: string;
  label: string;
}

export interface PluginInfo {
  name: string;
  version: string;
  description: string;
  pluginType: string;
  running: boolean;
  alias?: string;
  activityBar?: PluginActivityBar;
  views: ViewDeclaration[];
}

export interface StatusViewItem {
  label: string;
  value: string;
  status?: "ok" | "warn" | "error" | "info";
}

export interface StatusViewData {
  items: StatusViewItem[];
}

export interface PluginViewEnvelope {
  updatedAt: string;
  data: StatusViewData;
}

export interface RemotePlugin {
  name: string;
  description: string;
  version: string;
  pluginType: string;
  source: string;
  available: boolean;
  installed: boolean;
}

export interface PluginCommand {
  name: string;
  description: string;
}

export interface PluginToolsInfo {
  defaultTools: string[];
  allCoreTools: string[];
}

export interface DependsOnDto {
  field: string;
  values: string[];
}

export interface ConfigFieldSchemaDto {
  type: string;
  description: string;
  validValues: string[];
  label?: string;
  format?: string;
  group?: string;
  dependsOn?: DependsOnDto;
  optionsCommand?: string;
  hidden: boolean;
  default?: unknown;
  order: number;
}

export type AuthDeclarationDto =
  | { type: "qr"; qrFile: string; statusFile: string }
  | { type: "token"; checkField: string }
  | { type: "command" };

export interface WebUiStatus {
  running: boolean;
  port: number;
}

export interface TranscriptionStatus {
  installed: boolean;
  running: boolean;
  port: number;
}

export type TabId = "chat" | "terminal" | "config" | "credentials" | "plugins" | "providers" | "memory";

// ── Workspace types ──────────────────────────────────────────────────

export type WorkspaceMode = "agent" | "terminal";

export type SidePanelId = "files" | "workers" | "sessions" | "browser" | "processes" | "events" | "plugin";

export interface SystemEvent {
  ts: string;
  source: string;
  level: string;
  detail: string;
}

export interface BgProcessInfo {
  pid: number;
  command: string;
  startedAt: string;
  exitedAt: string | null;
  status: "running" | "completed" | "killed";
  label?: string;
  sessionId?: string;
  tabId?: string;
}

export interface BrowserStatus {
  running: boolean;
  cdpUrl?: string;
  currentUrl?: string;
}

export interface BrowserTab {
  id: string;
  title: string;
  url: string;
  wsUrl: string;
}

export interface FileEntry {
  name: string;
  isDir: boolean;
  size: number;
  modified?: string;
}

export interface DirectoryListing {
  path: string;
  entries: FileEntry[];
}

export interface FileContent {
  path: string;
  name: string;
  content: string;
  encoding: "utf8" | "base64";
  size: number;
  language?: string;
  mimeType?: string;
}

// ── Worker types ──────────────────────────────────────────────────────

export interface WorkerDefinition {
  id: string;
  name: string;
  schedule: string;
  prompt: string;
  tools?: string[];
  provider?: string;
  model?: string;
  enabled: boolean;
  notify?: string[];
  createdAt: string;
  lastRunAt?: string;
  lastRunStatus?: string;
}

export interface WorkerRun {
  id: string;
  workerId: string;
  startedAt: string;
  completedAt?: string;
  status: string;
  output: string;
  error?: string;
  tokenUsage: WorkerTokenUsage;
  turns: number;
}

export interface WorkerTokenUsage {
  input: number;
  output: number;
  cacheCreation: number;
  cacheRead: number;
}

export interface WorkerSummary extends WorkerDefinition {
  recentRunStatus?: string;
}

export interface WorkerDetail extends WorkerSummary {
  recentRuns: WorkerRun[];
}

export interface WorkerCreateInput {
  name: string;
  schedule: string;
  prompt: string;
  tools?: string[];
  provider?: string;
  model?: string;
  enabled?: boolean;
  notify?: string[];
}

export interface WorkerUpdateInput {
  name?: string;
  schedule?: string;
  prompt?: string;
  tools?: string[];
  provider?: string;
  model?: string;
  enabled?: boolean;
  notify?: string[];
}

// ── Terminal types ────────────────────────────────────────────────────

export interface TerminalSessionInfo {
  id: string;
  cols: number;
  rows: number;
}

export interface TerminalOutputEvent {
  type: "data" | "exited";
  data?: string; // base64 for "data" type
}

// ── Chat types ───────────────────────────────────────────────────────

export type PermissionMode = "manual" | "auto" | "skip" | "planner";

// Message types
export interface TextBlock {
  type: "text";
  text: string;
}

export interface ThinkingBlock {
  type: "thinking";
  text: string;
}

export interface ToolUseBlock {
  type: "tool_use";
  id: string;
  name: string;
  input: Record<string, unknown>;
}

export interface ToolResultBlock {
  type: "tool_result";
  toolUseId: string;
  content: string;
  isError?: boolean;
  diff?: string;
  image?: string;
}

export interface ImageBlock {
  type: "image";
  source: "base64" | "url";
  data: string;
  mediaType?: string;
}

export type ContentBlock =
  | TextBlock
  | ThinkingBlock
  | ToolUseBlock
  | ToolResultBlock
  | ImageBlock;

export interface Message {
  role: "user" | "assistant" | "tool";
  content: string | ContentBlock[];
  toolCallId?: string;
  name?: string;
}

// Stream events (mirrors Rust StreamEvent serde output)
export interface ToolApprovalRequest {
  id: string;
  name: string;
  input: Record<string, unknown>;
}

export type StopReason = "end_turn" | "tool_use" | "max_tokens" | "error";

export type StreamEvent =
  | { type: "message_start" }
  | { type: "text_delta"; text: string }
  | { type: "thinking_delta"; text: string }
  | { type: "tool_use_start"; id: string; name: string }
  | { type: "tool_use_delta"; input: string }
  | { type: "tool_use_end"; id: string; name: string; input: Record<string, unknown> }
  | { type: "tool_progress"; id: string; name: string; content: string }
  | { type: "explore_progress"; id: string; task: string; toolCalls: number; tokens: number; elapsedSecs: number; activity: string }
  | { type: "tool_result"; id: string; name: string; content: string; isError?: boolean; diff?: string; image?: string }
  | { type: "approval_required"; tools: ToolApprovalRequest[] }
  | { type: "planner_question"; id: string; questions: PlannerQuestion[] }
  | { type: "plan_review"; id: string; title: string; plan: string; tasks: PlanTask[] }
  | { type: "usage"; inputTokens: number; outputTokens: number; cacheCreationTokens?: number; cacheReadTokens?: number }
  | { type: "message_end"; stopReason: StopReason }
  | { type: "mode_changed"; mode: string }
  | { type: "tasks_update"; tasks: TaskInfo[] }
  | { type: "error"; error: string };

export interface PlannerQuestion {
  header: string;
  question: string;
  options: Array<{ label: string; description?: string }>;
  multiSelect: boolean;
}

export interface PlanTask {
  title: string;
  detail: string;
}

export interface TaskInfo {
  id: number;
  title: string;
  status: "pending" | "in_progress" | "done";
}

// Session types
export interface SessionSummary {
  id: string;
  name: string;
  createdAt: string;
  updatedAt: string;
  messageCount: number;
  firstUserMessage: string;
  lastUserMessage: string;
  model: string;
}

// Init response from backend
export interface InitResponse {
  sessionId: string;
  messages: Message[];
  providerName: string;
  modelName: string;
  permissionMode: string;
  tokenUsage: TokenUsage;
  contextSize: number;
}

export interface TokenUsage {
  input: number;
  output: number;
  cacheCreation: number | null;
  cacheRead: number | null;
}

export interface TurnComplete {
  sessionId: string;
  messages: Message[];
  contextSize: number;
  tokenUsage: TokenUsage;
}
