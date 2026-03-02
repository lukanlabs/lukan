import { HelpCircle, Send, Check, Square, CheckSquare, ChevronDown, ChevronRight } from "lucide-react";
import { useState } from "react";
import type { PlannerQuestion } from "../../lib/types";

interface QuestionPickerProps {
  questions: PlannerQuestion[];
  onSubmit: (answer: string) => void;
}

export function QuestionPicker({ questions, onSubmit }: QuestionPickerProps) {
  const [open, setOpen] = useState(true);
  const [currentIdx, setCurrentIdx] = useState(0);
  const [answers, setAnswers] = useState<string[]>([]);
  const [selected, setSelected] = useState<Set<string>>(new Set());
  const [customInput, setCustomInput] = useState("");
  const [answered, setAnswered] = useState(false);

  const question = questions[currentIdx];
  const isMulti = question.multiSelect;

  const advance = (answer: string) => {
    const newAnswers = [...answers, answer];
    if (currentIdx < questions.length - 1) {
      setCurrentIdx(currentIdx + 1);
      setAnswers(newAnswers);
      setSelected(new Set());
      setCustomInput("");
    } else {
      setAnswered(true);
      onSubmit(newAnswers.join("; "));
    }
  };

  const toggleOption = (label: string) => {
    setSelected((prev) => {
      const next = new Set(prev);
      if (next.has(label)) next.delete(label);
      else next.add(label);
      return next;
    });
  };

  const confirmMulti = () => {
    if (selected.size === 0) return;
    advance([...selected].join(", "));
  };

  const submitCustom = () => {
    const trimmed = customInput.trim();
    if (!trimmed) return;
    if (isMulti) {
      toggleOption(trimmed);
      setCustomInput("");
    } else {
      advance(trimmed);
    }
  };

  return (
    <div className="my-1 rounded-lg bg-white/[0.02] overflow-hidden">
      {/* Header */}
      <button
        onClick={() => setOpen(!open)}
        className="flex items-center gap-2 w-full text-left cursor-pointer rounded-md px-2 py-1.5 hover:bg-white/5 transition-colors"
      >
        <span className="text-zinc-600 shrink-0">
          {open ? <ChevronDown className="h-3 w-3" /> : <ChevronRight className="h-3 w-3" />}
        </span>
        <span className="shrink-0 text-blue-400/70">
          <HelpCircle className="h-3.5 w-3.5" />
        </span>
        <span className="text-xs font-medium text-blue-300/80">
          {question.header}
        </span>
        {isMulti && (
          <span className="text-[10px] text-zinc-600">multi-select</span>
        )}
        {questions.length > 1 && (
          <span className="text-[11px] text-zinc-600">
            {currentIdx + 1}/{questions.length}
          </span>
        )}
        <span className="shrink-0 ml-auto">
          {answered ? (
            <span className="h-1.5 w-1.5 rounded-full bg-green-500/50 inline-block" />
          ) : (
            <span className="h-1.5 w-1.5 rounded-full bg-blue-400/50 inline-block" />
          )}
        </span>
      </button>

      {/* Collapsible content */}
      {open && !answered && (
        <div className="mx-2 mb-2">
          {/* Question text */}
          <div className="rounded-md bg-white/[0.02] px-3 py-2 mb-2">
            <p className="text-xs text-zinc-400">{question.question}</p>
          </div>

          {/* Options */}
          <div className="space-y-1 mb-2">
            {question.options.map((opt, i) =>
              isMulti ? (
                <button
                  key={i}
                  onClick={() => toggleOption(opt.label)}
                  className={`flex w-full items-start gap-2 rounded-md px-3 py-2 text-left transition-colors cursor-pointer ${
                    selected.has(opt.label)
                      ? "bg-blue-500/10"
                      : "bg-white/[0.02] hover:bg-white/[0.03]"
                  }`}
                >
                  {selected.has(opt.label) ? (
                    <CheckSquare className="h-3.5 w-3.5 shrink-0 mt-0.5 text-blue-400" />
                  ) : (
                    <Square className="h-3.5 w-3.5 shrink-0 mt-0.5 text-zinc-600" />
                  )}
                  <div className="flex flex-col min-w-0">
                    <span className="text-xs font-medium text-blue-400/80">{opt.label}</span>
                    {opt.description && (
                      <span className="text-[11px] text-zinc-600 mt-0.5">{opt.description}</span>
                    )}
                  </div>
                </button>
              ) : (
                <button
                  key={i}
                  onClick={() => advance(opt.label)}
                  className="flex w-full flex-col items-start rounded-md bg-white/[0.02] px-3 py-2 text-left transition-colors hover:bg-white/[0.03] cursor-pointer"
                >
                  <span className="text-xs font-medium text-blue-400/80">{opt.label}</span>
                  {opt.description && (
                    <span className="text-[11px] text-zinc-600 mt-0.5">{opt.description}</span>
                  )}
                </button>
              ),
            )}
          </div>

          {/* Footer actions */}
          <div className="flex items-center gap-1 px-1">
            {isMulti ? (
              <button
                onClick={confirmMulti}
                disabled={selected.size === 0}
                className="flex items-center gap-1 px-2 py-1 rounded-md text-[11px] font-medium text-zinc-300 hover:text-zinc-100 hover:bg-white/5 transition-colors disabled:opacity-40 disabled:cursor-not-allowed"
              >
                <Check className="h-3 w-3" />
                Confirm ({selected.size})
              </button>
            ) : null}
            <div className="flex-1" />
            <div className="flex items-center gap-1.5">
              <input
                type="text"
                value={customInput}
                onChange={(e) => setCustomInput(e.target.value)}
                placeholder={isMulti ? "Custom option..." : "Custom answer..."}
                className="w-40 rounded-md border border-white/5 bg-white/[0.02] px-2 py-1 text-[11px] text-zinc-100 placeholder:text-zinc-600 focus:outline-none focus:ring-1 focus:ring-zinc-600"
                onKeyDown={(e) => e.key === "Enter" && submitCustom()}
              />
              <button
                onClick={submitCustom}
                disabled={!customInput.trim()}
                className="flex items-center px-1.5 py-1 rounded-md text-zinc-400 hover:text-zinc-200 hover:bg-white/5 transition-colors disabled:opacity-40 disabled:cursor-not-allowed"
              >
                <Send className="h-3 w-3" />
              </button>
            </div>
          </div>
        </div>
      )}

      {/* Answered state */}
      {answered && (
        <div className="flex items-center gap-2 mx-2 mb-2 px-2 py-1.5 text-xs text-green-400/70">
          <Check className="h-3 w-3" />
          Answered
        </div>
      )}
    </div>
  );
}
