import { Save, Loader2, Check, Cpu, Monitor, Globe, ChevronDown } from "lucide-react";
import React, { useState, useEffect, useCallback } from "react";
import { Button } from "@/components/ui/button";
import { ScrollArea } from "@/components/ui/scroll-area";
import { Separator } from "@/components/ui/separator";

const TIMEZONES = [
  { value: "America/New_York", label: "US Eastern (New York)" },
  { value: "America/Chicago", label: "US Central (Chicago)" },
  { value: "America/Denver", label: "US Mountain (Denver)" },
  { value: "America/Los_Angeles", label: "US Pacific (Los Angeles)" },
  { value: "America/Mexico_City", label: "Mexico City" },
  { value: "America/Bogota", label: "Colombia (Bogota)" },
  { value: "America/Lima", label: "Peru (Lima)" },
  { value: "America/Santiago", label: "Chile (Santiago)" },
  { value: "America/Argentina/Buenos_Aires", label: "Argentina (Buenos Aires)" },
  { value: "America/Sao_Paulo", label: "Brasil (Sao Paulo)" },
  { value: "Europe/London", label: "UK (London)" },
  { value: "Europe/Madrid", label: "Spain (Madrid)" },
  { value: "Europe/Paris", label: "France (Paris)" },
  { value: "Europe/Berlin", label: "Germany (Berlin)" },
  { value: "Europe/Rome", label: "Italy (Rome)" },
  { value: "Europe/Moscow", label: "Russia (Moscow)" },
  { value: "Asia/Tokyo", label: "Japan (Tokyo)" },
  { value: "Asia/Shanghai", label: "China (Shanghai)" },
  { value: "Asia/Kolkata", label: "India (Kolkata)" },
  { value: "Asia/Dubai", label: "UAE (Dubai)" },
  { value: "Australia/Sydney", label: "Australia (Sydney)" },
  { value: "Pacific/Auckland", label: "New Zealand (Auckland)" },
];

const SYNTAX_THEMES = [
  "andromeeda",
  "ayu-dark",
  "catppuccin-mocha",
  "catppuccin-macchiato",
  "dark-plus",
  "dracula",
  "dracula-soft",
  "everforest-dark",
  "github-dark",
  "github-dark-dimmed",
  "gruvbox-dark-medium",
  "houston",
  "kanagawa-wave",
  "material-theme-ocean",
  "min-dark",
  "monokai",
  "night-owl",
  "nord",
  "one-dark-pro",
  "poimandres",
  "rose-pine",
  "rose-pine-moon",
  "synthwave-84",
  "tokyo-night",
  "vesper",
  "vitesse-black",
  "vitesse-dark",
];

interface SettingsPanelProps {
  configValues: Record<string, unknown> | null;
  onRequestConfig: () => void;
  onSaveConfig: (config: Record<string, unknown>) => void;
}

type LocalConfig = {
  maxTokens: number;
  temperature: number;
  timezone: string;
  syntaxTheme: string;
  browserScreenshots: boolean;
};

const DEFAULTS: LocalConfig = {
  maxTokens: 8192,
  temperature: 0.7,
  timezone: "",
  syntaxTheme: "",
  browserScreenshots: true,
};

function toLocal(remote: Record<string, unknown>): LocalConfig {
  return {
    maxTokens: typeof remote.maxTokens === "number" ? remote.maxTokens : DEFAULTS.maxTokens,
    temperature: typeof remote.temperature === "number" ? remote.temperature : DEFAULTS.temperature,
    timezone: typeof remote.timezone === "string" ? remote.timezone : DEFAULTS.timezone,
    syntaxTheme: typeof remote.syntaxTheme === "string" ? remote.syntaxTheme : DEFAULTS.syntaxTheme,
    browserScreenshots:
      typeof remote.browserScreenshots === "boolean"
        ? remote.browserScreenshots
        : DEFAULTS.browserScreenshots,
  };
}

function ToggleSwitch({ checked, onChange }: { checked: boolean; onChange: (v: boolean) => void }) {
  return (
    <button
      type="button"
      onClick={() => onChange(!checked)}
      className={`relative inline-flex h-5 w-9 items-center rounded-full transition-colors ${
        checked ? "bg-purple-500" : "bg-zinc-700"
      }`}
    >
      <span
        className={`inline-block h-3.5 w-3.5 rounded-full bg-white transition-transform ${
          checked ? "translate-x-[18px]" : "translate-x-[3px]"
        }`}
      />
    </button>
  );
}

function FieldLabel({ children }: { children: React.ReactNode }) {
  return <label className="text-xs text-zinc-400 font-medium">{children}</label>;
}

function SectionHeader({
  icon: Icon,
  label,
}: {
  icon: React.FC<{ className?: string }>;
  label: string;
}) {
  return (
    <div className="flex items-center gap-2 mb-3">
      <Icon className="h-3.5 w-3.5 text-zinc-500" />
      <span className="text-[10px] font-semibold uppercase tracking-wider text-zinc-500">
        {label}
      </span>
    </div>
  );
}

export function SettingsPanel({ configValues, onRequestConfig, onSaveConfig }: SettingsPanelProps) {
  const [local, setLocal] = useState<LocalConfig>(DEFAULTS);
  const [saved, setSaved] = useState(false);

  // Fetch config on mount
  useEffect(() => {
    onRequestConfig();
  }, [onRequestConfig]);

  // Sync remote → local when configValues arrives
  useEffect(() => {
    if (configValues) {
      setLocal(toLocal(configValues));
      setSaved(false);
    }
  }, [configValues]);

  const isDirty = useCallback(() => {
    if (!configValues) return false;
    const remote = toLocal(configValues);
    return JSON.stringify(local) !== JSON.stringify(remote);
  }, [local, configValues]);

  const handleSave = () => {
    const diff: Record<string, unknown> = {};
    if (!configValues) return;
    const remote = toLocal(configValues);
    for (const key of Object.keys(local) as Array<keyof LocalConfig>) {
      if (local[key] !== remote[key]) {
        diff[key] = local[key];
      }
    }
    if (Object.keys(diff).length > 0) {
      onSaveConfig(diff);
      setSaved(true);
      setTimeout(() => setSaved(false), 2000);
    }
  };

  const update = <K extends keyof LocalConfig>(key: K, value: LocalConfig[K]) => {
    setLocal((prev) => ({ ...prev, [key]: value }));
    setSaved(false);
  };

  if (!configValues) {
    return (
      <div className="flex-1 flex items-center justify-center min-h-0">
        <Loader2 className="h-4 w-4 animate-spin text-zinc-500" />
      </div>
    );
  }

  return (
    <ScrollArea className="flex-1 min-h-0 px-3">
      <div className="space-y-5 py-3">
        {/* ── Generation ── */}
        <div>
          <SectionHeader icon={Cpu} label="Generation" />
          <div className="space-y-3 pl-1">
            <div className="space-y-1.5">
              <div className="flex items-center justify-between">
                <FieldLabel>Max Tokens</FieldLabel>
                <span className="text-[10px] text-zinc-600 font-mono">{local.maxTokens}</span>
              </div>
              <input
                type="range"
                min={512}
                max={32768}
                step={512}
                value={local.maxTokens}
                onChange={(e) => update("maxTokens", Number(e.target.value))}
                className="w-full h-1.5 bg-zinc-800 rounded-lg appearance-none cursor-pointer accent-purple-500"
              />
              <div className="flex justify-between text-[9px] text-zinc-600">
                <span>512</span>
                <span>32768</span>
              </div>
            </div>

            <div className="space-y-1.5">
              <div className="flex items-center justify-between">
                <FieldLabel>Temperature</FieldLabel>
                <span className="text-[10px] text-zinc-600 font-mono">
                  {local.temperature.toFixed(1)}
                </span>
              </div>
              <input
                type="range"
                min={0}
                max={2}
                step={0.1}
                value={local.temperature}
                onChange={(e) => update("temperature", Number(e.target.value))}
                className="w-full h-1.5 bg-zinc-800 rounded-lg appearance-none cursor-pointer accent-purple-500"
              />
              <div className="flex justify-between text-[9px] text-zinc-600">
                <span>0 (precise)</span>
                <span>2 (creative)</span>
              </div>
            </div>
          </div>
        </div>

        <Separator className="bg-zinc-800" />

        {/* ── Display ── */}
        <div>
          <SectionHeader icon={Monitor} label="Display" />
          <div className="space-y-3 pl-1">
            <div className="space-y-1.5">
              <FieldLabel>Timezone</FieldLabel>
              <div className="relative">
                <select
                  value={local.timezone || "__browser__"}
                  onChange={(e) =>
                    update("timezone", e.target.value === "__browser__" ? "" : e.target.value)
                  }
                  className="w-full appearance-none rounded-md bg-zinc-900 border border-zinc-800 px-2.5 py-1.5 pr-7 text-xs text-zinc-200 focus:outline-none focus:border-zinc-600 cursor-pointer"
                >
                  <option value="__browser__" className="bg-zinc-900">
                    {Intl.DateTimeFormat().resolvedOptions().timeZone} (auto)
                  </option>
                  {TIMEZONES.map((tz) => (
                    <option key={tz.value} value={tz.value} className="bg-zinc-900">
                      {tz.label}
                    </option>
                  ))}
                  {local.timezone && !TIMEZONES.some((t) => t.value === local.timezone) && (
                    <option value={local.timezone} className="bg-zinc-900">
                      {local.timezone}
                    </option>
                  )}
                </select>
                <ChevronDown className="absolute right-2 top-1/2 -translate-y-1/2 h-3 w-3 text-zinc-500 pointer-events-none" />
              </div>
            </div>

            <div className="space-y-1.5">
              <FieldLabel>Syntax Theme</FieldLabel>
              <div className="relative">
                <select
                  value={local.syntaxTheme}
                  onChange={(e) => update("syntaxTheme", e.target.value)}
                  className="w-full appearance-none rounded-md bg-zinc-900 border border-zinc-800 px-2.5 py-1.5 pr-7 text-xs text-zinc-200 focus:outline-none focus:border-zinc-600 cursor-pointer"
                >
                  <option value="" className="bg-zinc-900">
                    vitesse-dark (default)
                  </option>
                  {SYNTAX_THEMES.map((theme) => (
                    <option key={theme} value={theme} className="bg-zinc-900">
                      {theme}
                    </option>
                  ))}
                </select>
                <ChevronDown className="absolute right-2 top-1/2 -translate-y-1/2 h-3 w-3 text-zinc-500 pointer-events-none" />
              </div>
            </div>
          </div>
        </div>

        <Separator className="bg-zinc-800" />

        {/* ── Browser ── */}
        <div>
          <SectionHeader icon={Globe} label="Browser" />
          <div className="space-y-3 pl-1">
            <div className="flex items-center justify-between">
              <FieldLabel>Auto Screenshots</FieldLabel>
              <ToggleSwitch
                checked={local.browserScreenshots}
                onChange={(v) => update("browserScreenshots", v)}
              />
            </div>
          </div>
        </div>

        {/* ── Save ── */}
        <div className="pt-2 pb-1">
          <Button
            size="sm"
            onClick={handleSave}
            disabled={!isDirty() && !saved}
            className={`w-full gap-2 transition-all ${
              saved
                ? "bg-green-500/20 text-green-400 border border-green-500/30 hover:bg-green-500/20"
                : "bg-zinc-100 text-zinc-900 hover:bg-zinc-200 border-0"
            } disabled:bg-zinc-800 disabled:text-zinc-600`}
          >
            {saved ? (
              <>
                <Check className="h-3.5 w-3.5" />
                Saved
              </>
            ) : (
              <>
                <Save className="h-3.5 w-3.5" />
                Save Changes
              </>
            )}
          </Button>
        </div>
      </div>
    </ScrollArea>
  );
}
