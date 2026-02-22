import { Check, MessageSquare, ListChecks } from "lucide-react";
import React, { useState } from "react";
import { renderMarkdown } from "../lib/markdown.ts";
import { Button } from "@/components/ui/button";
import { Card, CardContent } from "@/components/ui/card";
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
  DialogFooter,
} from "@/components/ui/dialog";
import { ScrollArea } from "@/components/ui/scroll-area";

interface PlanReviewerProps {
  title: string;
  plan: string;
  tasks: Array<{ title: string; detail: string }>;
  onAccept: (tasks?: Array<{ title: string; detail: string }>) => void;
  onReject: (feedback: string) => void;
}

export function PlanReviewer({ title, plan, tasks, onAccept, onReject }: PlanReviewerProps) {
  const [showFeedback, setShowFeedback] = useState(false);
  const [feedback, setFeedback] = useState("");

  return (
    <Dialog open>
      <DialogContent wide>
        <DialogHeader>
          <DialogTitle className="flex items-center gap-2">
            <ListChecks className="h-5 w-5 text-blue-400" />
            {title}
          </DialogTitle>
        </DialogHeader>

        {plan && (
          <ScrollArea className="max-h-60 rounded-md border bg-muted/30 p-3">
            <div
              className="prose-chat text-sm"
              dangerouslySetInnerHTML={{ __html: renderMarkdown(plan) }}
            />
          </ScrollArea>
        )}

        {tasks.length > 0 && (
          <div className="mt-3">
            <h3 className="text-sm font-semibold mb-2">Tasks</h3>
            <div className="space-y-2">
              {tasks.map((task, i) => (
                <Card key={i} className="bg-muted/30">
                  <CardContent className="p-3">
                    <strong className="text-sm text-blue-400">
                      {i + 1}. {task.title}
                    </strong>
                    <div
                      className="mt-1 text-xs text-muted-foreground prose-chat"
                      dangerouslySetInnerHTML={{ __html: renderMarkdown(task.detail) }}
                    />
                  </CardContent>
                </Card>
              ))}
            </div>
          </div>
        )}

        {showFeedback ? (
          <div className="mt-3">
            <textarea
              value={feedback}
              onChange={(e) => setFeedback(e.target.value)}
              placeholder="Describe what changes you'd like..."
              rows={4}
              className="w-full rounded-md border bg-transparent px-3 py-2 text-sm text-foreground placeholder:text-muted-foreground focus:outline-none focus:ring-1 focus:ring-ring resize-y"
            />
            <DialogFooter>
              <Button onClick={() => onReject(feedback)} disabled={!feedback.trim()}>
                <MessageSquare className="h-4 w-4" />
                Submit Feedback
              </Button>
              <Button variant="outline" onClick={() => setShowFeedback(false)}>
                Cancel
              </Button>
            </DialogFooter>
          </div>
        ) : (
          <DialogFooter>
            <Button onClick={() => onAccept(tasks)}>
              <Check className="h-4 w-4" />
              Accept Plan
            </Button>
            <Button variant="outline" onClick={() => setShowFeedback(true)}>
              <MessageSquare className="h-4 w-4" />
              Request Changes
            </Button>
          </DialogFooter>
        )}
      </DialogContent>
    </Dialog>
  );
}
