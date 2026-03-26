import { useState, useCallback, useEffect, useRef } from "react";
import type { FileEntry } from "../lib/types";
import { listDirectory, openInEditor, getCwd } from "../lib/tauri";

/** Map of relative path → git status letter (M, A, D, U, R) */
export type GitStatusMap = Map<string, string>;

/** Flat tree entry for rendering */
export interface TreeEntry {
  name: string;
  path: string;
  isDir: boolean;
  size: number;
  depth: number;
  expanded: boolean;
}

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
      // Also mark parent directories
      const parts = path.split("/");
      for (let i = 1; i < parts.length; i++) {
        const dirPath = parts.slice(0, i).join("/");
        if (!map.has(dirPath)) map.set(dirPath, "M");
      }
    }
  } catch {
    // ignore
  }
  return map;
}

export function useFileExplorer() {
  const [tree, setTree] = useState<TreeEntry[]>([]);
  const [rootPath, setRootPath] = useState<string>("");
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [gitStatus, setGitStatus] = useState<GitStatusMap>(new Map());
  const expandedRef = useRef<Set<string>>(new Set());

  const loadGitStatus = useCallback(async (dirPath: string) => {
    const map = await fetchGitStatus(dirPath);
    setGitStatus(map);
  }, []);

  /** Load children of a directory and insert into tree */
  const loadChildren = useCallback(async (dirPath: string, depth: number, insertAfterIdx?: number) => {
    const result = await listDirectory(dirPath);
    // Sort: dirs first, then alphabetical
    const sorted = [...result.entries].sort((a, b) => {
      if (a.isDir !== b.isDir) return a.isDir ? -1 : 1;
      return a.name.localeCompare(b.name);
    });
    const children: TreeEntry[] = sorted.map(e => ({
      name: e.name,
      path: `${dirPath}/${e.name}`,
      isDir: e.isDir,
      size: e.size,
      depth,
      expanded: false,
    }));
    return children;
  }, []);

  /** Initialize tree from a root path */
  const initTree = useCallback(async (path?: string) => {
    setLoading(true);
    setError(null);
    try {
      const result = await listDirectory(path);
      const root = result.path;
      setRootPath(root);
      expandedRef.current = new Set([root]);

      const sorted = [...result.entries].sort((a, b) => {
        if (a.isDir !== b.isDir) return a.isDir ? -1 : 1;
        return a.name.localeCompare(b.name);
      });
      setTree(sorted.map(e => ({
        name: e.name,
        path: `${root}/${e.name}`,
        isDir: e.isDir,
        size: e.size,
        depth: 0,
        expanded: false,
      })));
      loadGitStatus(root);
    } catch (e) {
      setError(String(e));
    } finally {
      setLoading(false);
    }
  }, [loadGitStatus, loadChildren]);

  /** Toggle expand/collapse a directory */
  const toggleDir = useCallback(async (dirPath: string) => {
    const expanded = expandedRef.current;

    if (expanded.has(dirPath)) {
      // Collapse: remove all descendants
      expanded.delete(dirPath);
      setTree(prev => {
        const idx = prev.findIndex(e => e.path === dirPath);
        if (idx < 0) return prev;
        const depth = prev[idx].depth;
        let endIdx = idx + 1;
        while (endIdx < prev.length && prev[endIdx].depth > depth) endIdx++;
        const next = [...prev];
        next[idx] = { ...next[idx], expanded: false };
        next.splice(idx + 1, endIdx - idx - 1);
        // Also remove any nested expanded dirs
        for (let i = idx + 1; i < endIdx; i++) {
          if (prev[i].isDir) expanded.delete(prev[i].path);
        }
        return next;
      });
    } else {
      // Expand: load children and insert
      try {
        const idx = tree.findIndex(e => e.path === dirPath);
        if (idx < 0) return;
        const depth = tree[idx].depth;
        const children = await loadChildren(dirPath, depth + 1);
        expanded.add(dirPath);
        setTree(prev => {
          const i = prev.findIndex(e => e.path === dirPath);
          if (i < 0) return prev;
          const next = [...prev];
          next[i] = { ...next[i], expanded: true };
          next.splice(i + 1, 0, ...children);
          return next;
        });
      } catch (e) {
        setError(String(e));
      }
    }
  }, [tree, loadChildren]);

  const openFile = useCallback(async (path: string, editor?: string) => {
    try {
      await openInEditor(path, editor);
    } catch (e) {
      setError(String(e));
    }
  }, []);

  const refresh = useCallback(() => {
    initTree(rootPath || undefined);
  }, [initTree, rootPath]);

  // Load cwd on mount
  useEffect(() => {
    getCwd().then((cwd) => initTree(cwd));
  }, [initTree]);

  // Navigate to new cwd when agent tab changes
  useEffect(() => {
    const handler = () => {
      setTimeout(() => getCwd().then((cwd) => {
        if (cwd && cwd !== rootPath) initTree(cwd);
      }), 300);
    };
    window.addEventListener("active-tab-changed", handler);
    return () => window.removeEventListener("active-tab-changed", handler);
  }, [initTree, rootPath]);

  // Poll git status every 3s
  useEffect(() => {
    if (!rootPath) return;
    const interval = setInterval(() => loadGitStatus(rootPath), 3000);
    return () => clearInterval(interval);
  }, [rootPath, loadGitStatus]);

  /** Get git status for a path (relative to repo root) */
  const getGitStatus = useCallback((entryPath: string): string | undefined => {
    if (gitStatus.size === 0) return undefined;
    // Try matching the entry name or relative path
    const name = entryPath.split("/").pop() ?? "";
    for (const [gPath, status] of gitStatus) {
      if (gPath === name || entryPath.endsWith("/" + gPath) || gPath.endsWith("/" + name)) return status;
      if (gPath.split("/").pop() === name) return status;
    }
    return undefined;
  }, [gitStatus]);

  return { tree, rootPath, loading, error, toggleDir, openFile, refresh, getGitStatus, initTree };
}
