import { useState } from "react";
import { History, RotateCcw, FileCode, X } from "lucide-react";
import type { CheckpointInfo } from "../../lib/types";

interface CheckpointMarkerProps {
  checkpoint: CheckpointInfo;
  /** All checkpoints from this one onward (used to show total affected files) */
  affectedSnapshots: CheckpointInfo["snapshots"];
  onRestore: (id: string, restoreCode: boolean) => void;
}

export function CheckpointMarker({
  checkpoint,
  affectedSnapshots,
  onRestore,
}: CheckpointMarkerProps) {
  const [expanded, setExpanded] = useState(false);
  const [confirming, setConfirming] = useState(false);

  // Unique files affected by restore (from this checkpoint onward)
  const affectedFileCount = new Set(affectedSnapshots.map((s) => s.path)).size;
  const time = new Date(checkpoint.createdAt).toLocaleTimeString([], {
    hour: "2-digit",
    minute: "2-digit",
  });

  return (
    <div className="my-2 mx-auto max-w-4xl">
      <div
        className="flex items-center gap-2 px-3 py-1.5 rounded-md bg-white/[0.02] border border-white/5 cursor-pointer hover:bg-white/[0.04] transition-colors"
        onClick={() => setExpanded(!expanded)}
      >
        <History size={12} className="text-zinc-600 shrink-0" />
        <span className="text-[11px] text-zinc-500 flex-1">
          Checkpoint — {affectedFileCount} file{affectedFileCount !== 1 ? "s" : ""} affected
        </span>
        <span className="text-[10px] text-zinc-600">{time}</span>
      </div>

      {expanded && (
        <div className="mt-1 rounded-md bg-white/[0.02] border border-white/5 overflow-hidden">
          {/* Files that will be reverted */}
          <div className="px-3 py-2 space-y-1">
            {/* Deduplicate affected files by path, keep last operation */}
            {(() => {
              const fileMap = new Map<string, typeof affectedSnapshots[0]>();
              for (const snap of affectedSnapshots) {
                fileMap.set(snap.path, snap);
              }
              return [...fileMap.values()].map((snap, i) => {
                const fileName = snap.path.split("/").pop() ?? snap.path;
                const opLabel =
                  snap.operation === "created"
                    ? "will delete"
                    : snap.operation === "deleted"
                      ? "will restore"
                      : "will revert";
                const opColor =
                  snap.operation === "created"
                    ? "text-red-400/70"
                    : snap.operation === "deleted"
                      ? "text-green-500/70"
                      : "text-yellow-500/70";
                return (
                  <div key={i} className="flex items-center gap-2">
                    <FileCode size={11} className="text-zinc-600 shrink-0" />
                    <span className="text-[11px] text-zinc-400 font-mono truncate flex-1">
                      {fileName}
                    </span>
                    <span className={`text-[10px] ${opColor}`}>{opLabel}</span>
                  </div>
                );
              });
            })()}
            {affectedSnapshots.length === 0 && (
              <span className="text-[11px] text-zinc-600">No files to revert</span>
            )}
          </div>

          {/* Restore actions */}
          <div className="border-t border-white/5 px-3 py-2 flex items-center gap-2">
            {!confirming ? (
              <button
                onClick={(e) => {
                  e.stopPropagation();
                  setConfirming(true);
                }}
                className="flex items-center gap-1.5 px-2.5 py-1 rounded-md text-[11px] font-medium text-zinc-400 hover:text-zinc-200 hover:bg-white/5 transition-colors"
              >
                <RotateCcw size={11} />
                Restore to here
              </button>
            ) : (
              <div className="flex items-center gap-2 flex-1">
                <span className="text-[11px] text-zinc-500">Restore:</span>
                <button
                  onClick={(e) => {
                    e.stopPropagation();
                    onRestore(checkpoint.id, false);
                    setConfirming(false);
                    setExpanded(false);
                  }}
                  className="flex items-center gap-1 px-2 py-1 rounded-md text-[11px] font-medium text-zinc-300 hover:text-zinc-100 bg-white/5 hover:bg-white/10 transition-colors"
                >
                  Chat only
                </button>
                <button
                  onClick={(e) => {
                    e.stopPropagation();
                    onRestore(checkpoint.id, true);
                    setConfirming(false);
                    setExpanded(false);
                  }}
                  className="flex items-center gap-1 px-2 py-1 rounded-md text-[11px] font-medium text-orange-300 hover:text-orange-100 bg-orange-500/10 hover:bg-orange-500/20 transition-colors"
                >
                  Chat + Code
                </button>
                <button
                  onClick={(e) => {
                    e.stopPropagation();
                    setConfirming(false);
                  }}
                  className="p-1 rounded-md text-zinc-500 hover:text-zinc-300 hover:bg-white/5 transition-colors"
                >
                  <X size={11} />
                </button>
              </div>
            )}
          </div>
        </div>
      )}
    </div>
  );
}
