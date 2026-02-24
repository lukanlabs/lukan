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
  whatsapp?: Record<string, unknown>;
  email?: Record<string, unknown>;
  openaiCompatibleBaseUrl?: string;
  openaiCompatibleProviderName?: string;
  openaiCompatibleProviderOptions?: Record<string, unknown>;
  webPassword?: string;
  webTokenTtl?: number;
  plugins?: PluginsConfig;
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
  openaiCompatibleApiKey?: string;
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
}

export interface FetchedModel {
  id: string;
  name: string;
}

export interface PluginInfo {
  name: string;
  version: string;
  description: string;
  pluginType: string;
  running: boolean;
  alias?: string;
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

export interface WhatsAppGroup {
  id: string;
  subject: string;
  participants?: number;
}

export interface WebUiStatus {
  running: boolean;
  port: number;
}

export type TabId = "config" | "credentials" | "plugins" | "providers" | "memory";
