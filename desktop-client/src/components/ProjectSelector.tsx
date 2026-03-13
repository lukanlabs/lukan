import { useState, useEffect } from "react";
import {
  pickDirectory,
  setProjectCwd,
  getRecentProjects,
  addRecentProject,
  type RecentProject,
} from "../lib/tauri";

interface Props {
  onSelect: () => void;
}

export default function ProjectSelector({ onSelect }: Props) {
  const [recents, setRecents] = useState<RecentProject[]>([]);
  const [loading, setLoading] = useState(false);

  useEffect(() => {
    getRecentProjects().then(setRecents).catch(() => {});
  }, []);

  const selectPath = async (path: string) => {
    setLoading(true);
    try {
      await setProjectCwd(path);
      await addRecentProject(path);
      onSelect();
    } catch (e) {
      console.error("Failed to set project cwd:", e);
      setLoading(false);
    }
  };

  const handlePick = async () => {
    const path = await pickDirectory();
    if (path) {
      await selectPath(path);
    }
  };

  const handleHomeDir = async () => {
    const home =
      (await import("../lib/tauri").then((m) => m.getCwd()).catch(() => null)) ||
      "/";
    await selectPath(home);
  };

  return (
    <div className="flex items-center justify-center h-screen bg-zinc-950 text-zinc-200">
      <div className="w-full max-w-lg px-6">
        <div className="text-center mb-8">
          <h1 className="text-2xl font-semibold mb-2">Lukan Desktop</h1>
          <p className="text-sm text-zinc-500">
            Select a project directory for the agent to work in
          </p>
        </div>

        <div className="space-y-3 mb-6">
          <button
            onClick={handlePick}
            disabled={loading}
            className="w-full flex items-center gap-3 px-4 py-3 bg-zinc-800 hover:bg-zinc-700 rounded-lg transition-colors cursor-pointer disabled:opacity-50"
          >
            <svg
              className="w-5 h-5 text-zinc-400 shrink-0"
              fill="none"
              viewBox="0 0 24 24"
              stroke="currentColor"
              strokeWidth={2}
            >
              <path
                strokeLinecap="round"
                strokeLinejoin="round"
                d="M3 7v10a2 2 0 002 2h14a2 2 0 002-2V9a2 2 0 00-2-2h-6l-2-2H5a2 2 0 00-2 2z"
              />
            </svg>
            <span className="text-sm font-medium">Open Folder...</span>
          </button>

          <button
            onClick={handleHomeDir}
            disabled={loading}
            className="w-full flex items-center gap-3 px-4 py-3 bg-zinc-900 hover:bg-zinc-800 rounded-lg transition-colors cursor-pointer text-zinc-400 disabled:opacity-50"
          >
            <svg
              className="w-5 h-5 shrink-0"
              fill="none"
              viewBox="0 0 24 24"
              stroke="currentColor"
              strokeWidth={2}
            >
              <path
                strokeLinecap="round"
                strokeLinejoin="round"
                d="M3 12l2-2m0 0l7-7 7 7M5 10v10a1 1 0 001 1h3m10-11l2 2m-2-2v10a1 1 0 01-1 1h-3m-4 0h4"
              />
            </svg>
            <span className="text-sm">Continue without project (home directory)</span>
          </button>
        </div>

        {recents.length > 0 && (
          <div>
            <p className="text-xs text-zinc-500 uppercase tracking-wider mb-2 px-1">
              Recent Projects
            </p>
            <div className="space-y-1">
              {recents.map((project) => (
                <button
                  key={project.path}
                  onClick={() => selectPath(project.path)}
                  disabled={loading}
                  className="w-full flex items-center gap-3 px-4 py-2.5 hover:bg-zinc-800/60 rounded-lg transition-colors cursor-pointer text-left disabled:opacity-50"
                >
                  <svg
                    className="w-4 h-4 text-zinc-500 shrink-0"
                    fill="none"
                    viewBox="0 0 24 24"
                    stroke="currentColor"
                    strokeWidth={2}
                  >
                    <path
                      strokeLinecap="round"
                      strokeLinejoin="round"
                      d="M3 7v10a2 2 0 002 2h14a2 2 0 002-2V9a2 2 0 00-2-2h-6l-2-2H5a2 2 0 00-2 2z"
                    />
                  </svg>
                  <div className="min-w-0 flex-1">
                    <p className="text-sm font-medium truncate">
                      {project.name}
                    </p>
                    <p className="text-xs text-zinc-500 truncate">
                      {project.path}
                    </p>
                  </div>
                </button>
              ))}
            </div>
          </div>
        )}
      </div>
    </div>
  );
}
