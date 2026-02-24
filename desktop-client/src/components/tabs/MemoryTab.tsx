import { useState, useEffect } from "react";
import { Globe, FolderOpen, Save } from "lucide-react";
import {
  getGlobalMemory,
  saveGlobalMemory,
  getProjectMemory,
  saveProjectMemory,
  isProjectMemoryActive,
  toggleProjectMemory,
} from "../../lib/tauri";
import { useToast } from "../ui/Toast";
import Button from "../ui/Button";
import Input from "../ui/Input";
import Textarea from "../ui/Textarea";
import Card from "../ui/Card";
import Badge from "../ui/Badge";
import Toggle from "../ui/Toggle";

type SubTab = "global" | "project";

export default function MemoryTab() {
  const { toast } = useToast();
  const [activeTab, setActiveTab] = useState<SubTab>("global");
  const [loading, setLoading] = useState(true);

  // Global memory state
  const [globalContent, setGlobalContent] = useState("");
  const [savingGlobal, setSavingGlobal] = useState(false);

  // Project memory state
  const [projectPath, setProjectPath] = useState("");
  const [projectContent, setProjectContent] = useState("");
  const [projectActive, setProjectActive] = useState(false);
  const [projectLoaded, setProjectLoaded] = useState(false);
  const [savingProject, setSavingProject] = useState(false);
  const [loadingProject, setLoadingProject] = useState(false);

  useEffect(() => {
    getGlobalMemory()
      .then(setGlobalContent)
      .catch((e) => toast("error", `${e}`))
      .finally(() => setLoading(false));
  }, []);

  const handleSaveGlobal = async () => {
    setSavingGlobal(true);
    try {
      await saveGlobalMemory(globalContent);
      toast("success", "Global memory saved");
    } catch (e) {
      toast("error", `${e}`);
    } finally {
      setSavingGlobal(false);
    }
  };

  const handleLoadProject = async () => {
    if (!projectPath.trim()) return;
    setLoadingProject(true);
    try {
      const [content, active] = await Promise.all([
        getProjectMemory(projectPath),
        isProjectMemoryActive(projectPath),
      ]);
      setProjectContent(content);
      setProjectActive(active);
      setProjectLoaded(true);
    } catch (e) {
      toast("error", `${e}`);
    } finally {
      setLoadingProject(false);
    }
  };

  const handleSaveProject = async () => {
    if (!projectPath.trim()) return;
    setSavingProject(true);
    try {
      await saveProjectMemory(projectPath, projectContent);
      toast("success", "Project memory saved");
    } catch (e) {
      toast("error", `${e}`);
    } finally {
      setSavingProject(false);
    }
  };

  const handleToggleProject = async (active: boolean) => {
    if (!projectPath.trim()) return;
    try {
      await toggleProjectMemory(projectPath, active);
      setProjectActive(active);
      toast(
        "success",
        active ? "Project memory activated" : "Project memory deactivated",
      );
    } catch (e) {
      toast("error", `${e}`);
    }
  };

  const pillBase =
    "inline-flex items-center gap-1.5 px-4 py-1.5 rounded-full text-sm font-medium cursor-pointer border-none select-none transition-all";

  return (
    <div className="max-w-3xl" style={{ animation: "fadeIn 0.3s ease-out" }}>
      {/* Header */}
      <div className="mb-8">
        <h2
          className="text-xl font-bold tracking-tight"
          style={{ color: "var(--text-primary)" }}
        >
          Memory
        </h2>
        <p className="text-sm mt-1.5" style={{ color: "var(--text-muted)" }}>
          Persistent context injected into every agent conversation.
        </p>
      </div>

      {/* Sub-navigation pills */}
      <div
        className="inline-flex items-center gap-1 p-1 rounded-full mb-6"
        style={{
          background: "var(--bg-tertiary)",
          border: "1px solid var(--border)",
        }}
      >
        <button
          className={pillBase}
          style={{
            background:
              activeTab === "global"
                ? "#fafafa"
                : "transparent",
            color: activeTab === "global" ? "#09090b" : "var(--text-secondary)",
            boxShadow:
              activeTab === "global"
                ? "0 1px 4px rgba(0,0,0,0.1)"
                : "none",
            transitionDuration: "200ms",
          }}
          onClick={() => setActiveTab("global")}
        >
          <Globe size={14} />
          Global
        </button>
        <button
          className={pillBase}
          style={{
            background:
              activeTab === "project"
                ? "#fafafa"
                : "transparent",
            color: activeTab === "project" ? "#09090b" : "var(--text-secondary)",
            boxShadow:
              activeTab === "project"
                ? "0 1px 4px rgba(0,0,0,0.1)"
                : "none",
            transitionDuration: "200ms",
          }}
          onClick={() => setActiveTab("project")}
        >
          <FolderOpen size={14} />
          Project
        </button>
      </div>

      {/* Loading state */}
      {loading && (
        <div
          className="text-sm py-12 text-center"
          style={{ color: "var(--text-muted)" }}
        >
          Loading...
        </div>
      )}

      {/* Global Memory view */}
      {!loading && activeTab === "global" && (
        <div style={{ animation: "fadeIn 0.2s ease-out" }}>
          <Card
            title="Global Memory"
            description="Stored at ~/.config/lukan/MEMORY.md — shared across all projects."
          >
            <Textarea
              value={globalContent}
              onChange={(e) => setGlobalContent(e.target.value)}
              placeholder="Write global memory content in markdown..."
              className="min-h-[350px]"
            />
            <div className="mt-3 flex items-end justify-between">
              <div />
              <div className="flex items-center gap-4">
                <span
                  className="text-xs tabular-nums"
                  style={{ color: "var(--text-muted)" }}
                >
                  {globalContent.length.toLocaleString()} characters
                </span>
                <Button onClick={handleSaveGlobal} disabled={savingGlobal}>
                  <Save size={14} />
                  {savingGlobal ? "Saving..." : "Save"}
                </Button>
              </div>
            </div>
          </Card>
        </div>
      )}

      {/* Project Memory view */}
      {!loading && activeTab === "project" && (
        <div style={{ animation: "fadeIn 0.2s ease-out" }}>
          <Card
            title="Project Memory"
            description="Scoped to a specific project directory."
          >
            {/* Project path input + Load button */}
            <div className="flex items-end gap-3 mb-5">
              <div className="flex-1">
                <Input
                  label="Project Path"
                  value={projectPath}
                  placeholder="/path/to/project"
                  onChange={(e) => {
                    setProjectPath(e.target.value);
                    setProjectLoaded(false);
                  }}
                  onKeyDown={(e) => {
                    if (e.key === "Enter") handleLoadProject();
                  }}
                />
              </div>
              <Button
                variant="secondary"
                onClick={handleLoadProject}
                disabled={!projectPath.trim() || loadingProject}
              >
                <FolderOpen size={14} />
                {loadingProject ? "Loading..." : "Load"}
              </Button>
            </div>

            {/* Loaded project content */}
            {projectLoaded && (
              <div style={{ animation: "slideUp 0.2s ease-out" }}>
                {/* Active toggle row */}
                <div
                  className="mb-4 py-3 px-4 rounded-xl flex items-center justify-between"
                  style={{
                    background: "var(--bg-tertiary)",
                    border: "1px solid var(--border)",
                  }}
                >
                  <Toggle
                    label="Project memory active"
                    checked={projectActive}
                    onChange={handleToggleProject}
                  />
                  <Badge variant={projectActive ? "success" : "neutral"}>
                    {projectActive ? "Active" : "Inactive"}
                  </Badge>
                </div>

                <Textarea
                  value={projectContent}
                  onChange={(e) => setProjectContent(e.target.value)}
                  placeholder="Write project memory content in markdown..."
                  className="min-h-[350px]"
                />
                <div className="mt-3 flex items-end justify-between">
                  <div />
                  <div className="flex items-center gap-4">
                    <span
                      className="text-xs tabular-nums"
                      style={{ color: "var(--text-muted)" }}
                    >
                      {projectContent.length.toLocaleString()} characters
                    </span>
                    <Button
                      onClick={handleSaveProject}
                      disabled={savingProject}
                    >
                      <Save size={14} />
                      {savingProject ? "Saving..." : "Save"}
                    </Button>
                  </div>
                </div>
              </div>
            )}

            {/* Placeholder when no project loaded */}
            {!projectLoaded && !loadingProject && (
              <div
                className="py-16 text-center rounded-xl"
                style={{
                  background: "var(--bg-tertiary)",
                  border: "1px dashed var(--border)",
                  color: "var(--text-muted)",
                }}
              >
                <FolderOpen
                  size={32}
                  style={{ margin: "0 auto 12px", opacity: 0.4 }}
                />
                <p className="text-sm">
                  Enter a project path and click Load.
                </p>
              </div>
            )}
          </Card>
        </div>
      )}
    </div>
  );
}
