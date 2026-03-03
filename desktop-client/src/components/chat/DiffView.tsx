interface DiffViewProps {
  diff: string;
}

export function DiffView({ diff }: DiffViewProps) {
  const lines = diff.split("\n");

  return (
    <div className="my-1.5 mx-2 max-h-72 rounded-md overflow-auto bg-white/[0.02]">
      <pre className="text-xs font-mono">
        {lines.map((line, i) => {
          let cls = "px-2 whitespace-pre";
          if (line.startsWith("+")) cls += " diff-add";
          else if (line.startsWith("-")) cls += " diff-remove";
          else if (line.startsWith("@@")) cls += " diff-hunk";
          else if (line.startsWith("diff") || line.startsWith("index")) cls += " diff-meta";

          return (
            <div key={i} className={cls}>
              {line}
            </div>
          );
        })}
      </pre>
    </div>
  );
}
