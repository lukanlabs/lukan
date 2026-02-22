import { HelpCircle, Send } from "lucide-react";
import React, { useState } from "react";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
  DialogDescription,
} from "@/components/ui/dialog";

interface Question {
  header: string;
  question: string;
  options: Array<{ label: string; description?: string }>;
  multiSelect: boolean;
}

interface QuestionPickerProps {
  questions: Question[];
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
    <Dialog open>
      <DialogContent>
        <DialogHeader>
          <DialogTitle className="flex items-center gap-2">
            <HelpCircle className="h-5 w-5 text-blue-400" />
            {question.header}
          </DialogTitle>
          <DialogDescription>{question.question}</DialogDescription>
        </DialogHeader>

        <div className="space-y-1.5 my-2">
          {question.options.map((opt, i) => (
            <button
              key={i}
              onClick={() => selectOption(opt.label)}
              className="flex w-full flex-col items-start rounded-md border px-3 py-2.5 text-left transition-colors hover:border-blue-500/40 hover:bg-blue-500/5"
            >
              <span className="text-sm font-semibold text-foreground">{opt.label}</span>
              {opt.description && (
                <span className="text-xs text-muted-foreground mt-0.5">{opt.description}</span>
              )}
            </button>
          ))}
        </div>

        <div className="flex gap-2">
          <input
            type="text"
            value={customInput}
            onChange={(e) => setCustomInput(e.target.value)}
            placeholder="Or type a custom answer..."
            className="flex-1 rounded-md border bg-transparent px-3 py-2 text-sm text-foreground placeholder:text-muted-foreground focus:outline-none focus:ring-1 focus:ring-ring"
            onKeyDown={(e) => e.key === "Enter" && submitCustom()}
          />
          <Button onClick={submitCustom} disabled={!customInput.trim()} size="sm">
            <Send className="h-3.5 w-3.5" />
          </Button>
        </div>

        {questions.length > 1 && (
          <p className="text-center text-[11px] text-muted-foreground mt-2">
            Question {currentIdx + 1} of {questions.length}
          </p>
        )}
      </DialogContent>
    </Dialog>
  );
}
