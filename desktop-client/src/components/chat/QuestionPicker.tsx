import { HelpCircle, Send } from "lucide-react";
import { useState } from "react";
import type { PlannerQuestion } from "../../lib/types";

interface QuestionPickerProps {
  questions: PlannerQuestion[];
  onSubmit: (answer: string) => void;
}

export function QuestionPicker({ questions, onSubmit }: QuestionPickerProps) {
  const [currentIdx, setCurrentIdx] = useState(0);
  const [answers, setAnswers] = useState<string[]>([]);
  const [customInput, setCustomInput] = useState("");

  const question = questions[currentIdx];

  const selectOption = (label: string) => {
    const newAnswers = [...answers, label];
    if (currentIdx < questions.length - 1) {
      setCurrentIdx(currentIdx + 1);
      setAnswers(newAnswers);
    } else {
      onSubmit(newAnswers.join(", "));
    }
  };

  const submitCustom = () => {
    const trimmed = customInput.trim();
    if (trimmed) {
      selectOption(trimmed);
      setCustomInput("");
    }
  };

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center">
      <div className="absolute inset-0 bg-black/60 backdrop-blur-sm" />

      <div
        className="relative w-full max-w-md mx-4 rounded-xl border animate-scale-in"
        style={{
          background: "var(--surface-raised)",
          borderColor: "var(--border)",
          boxShadow: "var(--shadow-lg)",
        }}
      >
        {/* Header */}
        <div className="px-6 py-4 border-b" style={{ borderColor: "var(--border-subtle)" }}>
          <h2 className="flex items-center gap-2 text-base font-semibold text-zinc-100">
            <HelpCircle className="h-5 w-5 text-blue-400" />
            {question.header}
          </h2>
          <p className="text-sm text-zinc-500 mt-1">{question.question}</p>
        </div>

        {/* Options */}
        <div className="px-6 py-4 space-y-1.5">
          {question.options.map((opt, i) => (
            <button
              key={i}
              onClick={() => selectOption(opt.label)}
              className="flex w-full flex-col items-start rounded-lg border border-zinc-800 px-3 py-2.5 text-left transition-colors hover:border-blue-500/40 hover:bg-blue-500/5 cursor-pointer"
            >
              <span className="text-sm font-semibold text-zinc-100">{opt.label}</span>
              {opt.description && (
                <span className="text-xs text-zinc-500 mt-0.5">{opt.description}</span>
              )}
            </button>
          ))}
        </div>

        {/* Custom input */}
        <div className="px-6 pb-4 flex gap-2">
          <input
            type="text"
            value={customInput}
            onChange={(e) => setCustomInput(e.target.value)}
            placeholder="Or type a custom answer..."
            className="flex-1 rounded-lg border border-zinc-800 bg-zinc-900 px-3 py-2 text-sm text-zinc-100 placeholder:text-zinc-600 focus:outline-none focus:ring-1 focus:ring-zinc-600"
            onKeyDown={(e) => e.key === "Enter" && submitCustom()}
          />
          <button
            onClick={submitCustom}
            disabled={!customInput.trim()}
            className="px-3 py-2 rounded-lg bg-zinc-100 text-zinc-900 hover:bg-zinc-200 transition-colors disabled:opacity-50 disabled:cursor-not-allowed"
          >
            <Send className="h-3.5 w-3.5" />
          </button>
        </div>

        {questions.length > 1 && (
          <p className="text-center text-[11px] text-zinc-600 pb-4">
            Question {currentIdx + 1} of {questions.length}
          </p>
        )}
      </div>
    </div>
  );
}
