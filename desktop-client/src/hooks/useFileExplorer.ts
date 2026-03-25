import { useState, useCallback, useEffect, useRef } from "react";
import type { FileEntry } from "../lib/types";
import { listDirectory, openInEditor, getCwd } from "../lib/tauri";

/** Map of relative path → git status letter (M, A, D, U, R) */
export type GitStatusMap = Map<string, string>;

async function fetchGitStatus(dir: string): Promise<GitStatusMap> {
  const map: GitStatusMap = new Map();
  try {
    const port = (window as any).__DAEMON_PORT__ || window.location.port || "3000";
    const base = `${window.location.protocol}//${window.location.hostname}:${port}`;
    const r = await fetch(`${base}/api/git?cmd=status&dir=${encodeURIComponent(dir)}`);
    if (!r.ok) return map;
    const data = await r.json();
    if (!data.ok || !data.stdout) return map;

    for (const line of data.stdout.trim().split("\n")) {
      if (line.length < 4) continue;
      const idx = line[0], wt = line[1];
      let path = line.substring(2).trimStart();
      if (path.startsWith('"') && path.endsWith('"')) path = path.slice(1, -1);
      const arrow = path.indexOf(" -> ");
      if (arrow >= 0) path = path.slice(arrow + 4).replace(/^"|"$/g, "");

      let status = "M";
      if (idx === "?" && wt === "?") status = "U";
      else if (idx === "A" || wt === "A") status = "A";
      else if (idx === "D" || wt === "D") status = "D";
      else if (idx === "R" || wt === "R") status = "R";

      map.set(path, status);
      // Also mark parent directories as modified
      const parts = path.split("/");
      for (let i = 1; i < parts.length; i++) {
        const dirPath = parts.slice(0, i).join("/");
        if (!map.has(dirPath)) map.set(dirPath, "M");
      }
    }
  } catch {
    // ignore — not a git repo or API unavailable
  }
  return map;
}

export function useFileExplorer() {
  const [entries, setEntries] = useState<FileEntry[]>([]);
  const [currentPath, setCurrentPath] = useState<string>("");
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [gitStatus, setGitStatus] = useState<GitStatusMap>(new Map());
  const gitRootRef = useRef<string>("");

  const loadGitStatus = useCallback(async (dirPath: string) => {
    // Find git root by checking the dir itself (git status works from any subdir)
    const map = await fetchGitStatus(dirPath);
    setGitStatus(map);
    gitRootRef.current = dirPath;
  }, []);

  const navigate = useCallback(async (path?: string) => {
    setLoading(true);
    setError(null);
    try {
      const result = await listDirectory(path);
      setEntries(result.entries);
      setCurrentPath(result.path);
      loadGitStatus(result.path);
    } catch (e) {
      setError(String(e));
    } finally {
      setLoading(false);
    }
  }, [loadGitStatus]);

  const openFile = useCallback(async (path: string, editor?: string) => {
    try {
      await openInEditor(path, editor);
    } catch (e) {
      setError(String(e));
    }
  }, []);

  const refresh = useCallback(() => {
    navigate(currentPath || undefined);
  }, [navigate, currentPath]);

  // Load cwd on mount
  useEffect(() => {
    getCwd().then((cwd) => navigate(cwd));
  }, [navigate]);

  // Poll git status every 3s
  useEffect(() => {
    if (!currentPath) return;
    const interval = setInterval(() => loadGitStatus(currentPath), 3000);
    return () => clearInterval(interval);
  }, [currentPath, loadGitStatus]);

  /** Get git status for a file/dir name relative to currentPath */
  const getGitStatus = useCallback((name: string): string | undefined => {
    if (gitStatus.size === 0) return undefined;
    // git status paths are relative to repo root, but we might be in a subdir
    // Check both the name directly and full relative paths
    for (const [path, status] of gitStatus) {
      const pathName = path.split("/").pop();
      if (path === name || pathName === name) return status;
      // Check if this is a path ending with /name
      if (path.endsWith("/" + name)) return status;
    }
    return undefined;
  }, [gitStatus]);

  return { entries, currentPath, loading, error, navigate, openFile, refresh, getGitStatus };
}
