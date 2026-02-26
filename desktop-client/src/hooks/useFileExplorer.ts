import { useState, useCallback, useEffect } from "react";
import type { FileEntry } from "../lib/types";
import { listDirectory, openInEditor, getCwd } from "../lib/tauri";

export function useFileExplorer() {
  const [entries, setEntries] = useState<FileEntry[]>([]);
  const [currentPath, setCurrentPath] = useState<string>("");
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const navigate = useCallback(async (path?: string) => {
    setLoading(true);
    setError(null);
    try {
      const result = await listDirectory(path);
      setEntries(result.entries);
      setCurrentPath(result.path);
    } catch (e) {
      setError(String(e));
    } finally {
      setLoading(false);
    }
  }, []);

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

  return { entries, currentPath, loading, error, navigate, openFile, refresh };
}
