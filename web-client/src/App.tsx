import React, { useState, useMemo } from "react";
import logoUrl from "./assets/logo.png";
import { ChatView } from "./components/ChatView.tsx";
import { Header } from "./components/Header.tsx";
import { LoginPage } from "./components/LoginPage.tsx";
import { PlanReviewer } from "./components/PlanReviewer.tsx";
import { QuestionPicker } from "./components/QuestionPicker.tsx";
import { Sidebar } from "./components/Sidebar.tsx";
import { SubAgentViewer } from "./components/SubAgentViewer.tsx";
import { ToolApprovalModal } from "./components/ToolApprovalModal.tsx";
import { Sheet } from "./components/ui/sheet.tsx";
import { WorkersPanel } from "./components/WorkersPanel.tsx";
import { useAgent } from "./hooks/useAgent.ts";

export function App() {
  const agent = useAgent();
  const [mobileSidebar, setMobileSidebar] = useState(false);
  const [subAgentViewerOpen, setSubAgentViewerOpen] = useState(false);
  const [workersPanelOpen, setWorkersPanelOpen] = useState(false);

  const runningSubAgentCount = useMemo(
    () => agent.subAgents.filter((a) => a.status === "running").length,
    [agent.subAgents],
  );

  const enabledWorkerCount = useMemo(
    () => agent.workers.filter((w) => w.enabled).length,
    [agent.workers],
  );

  if (!agent.connected) {
    return (
      <div className="flex h-screen flex-col items-center justify-center gap-4 bg-zinc-950">
        <div className="relative">
          <div className="flex h-14 w-14 items-center justify-center rounded-xl bg-zinc-800 border border-zinc-700 animate-pulse-subtle">
            <img src={logoUrl} alt="lukan" className="h-8 w-8" />
          </div>
        </div>
        <p className="text-sm text-zinc-500">Connecting to lukan...</p>
      </div>
    );
  }

  if (agent.authState === "required") {
    return <LoginPage onLogin={agent.login} error={agent.authError} />;
  }

  const sidebarContent = (
    <Sidebar
      sessions={agent.sessionList}
      currentSessionId={agent.sessionId}
      configValues={agent.configValues}
      onSelectSession={(id) => {
        agent.loadSession(id);
        setMobileSidebar(false);
      }}
      onNewSession={(name) => {
        agent.newSession(name);
        setMobileSidebar(false);
      }}
      onListSessions={agent.listSessions}
      onDeleteSession={agent.deleteSession}
      onRequestConfig={agent.getConfig}
      onSaveConfig={agent.setConfig}
    />
  );

  return (
    <div className="flex h-screen overflow-hidden bg-zinc-950">
      {/* Desktop sidebar */}
      <div className="hidden md:flex">{sidebarContent}</div>

      {/* Mobile sidebar (Sheet) */}
      <Sheet open={mobileSidebar} onOpenChange={setMobileSidebar}>
        {sidebarContent}
      </Sheet>

      {/* Main content */}
      <div className="flex flex-1 flex-col min-w-0">
        <Header
          providerName={agent.providerName}
          modelName={agent.modelName}
          tokenUsage={agent.tokenUsage}
          contextSize={agent.contextSize}
          isProcessing={agent.isProcessing}
          availableModels={agent.availableModels}
          subAgentCount={agent.subAgents.length}
          runningSubAgentCount={runningSubAgentCount}
          workerCount={agent.workers.length}
          enabledWorkerCount={enabledWorkerCount}
          onToggleSidebar={() => setMobileSidebar(true)}
          onListModels={agent.listModels}
          onSetModel={agent.setModel}
          onOpenSubAgentViewer={() => setSubAgentViewerOpen(true)}
          onOpenWorkersPanel={() => {
            agent.listWorkers();
            setWorkersPanelOpen(true);
          }}
        />

        <ChatView
          messages={agent.messages}
          streamingBlocks={agent.streamingBlocks}
          isProcessing={agent.isProcessing}
          error={agent.error}
          permissionMode={agent.permissionMode}
          toolImages={agent.toolImages}
          browserScreenshots={agent.browserScreenshots}
          onDismissError={agent.dismissError}
          onSend={agent.sendMessage}
          onAbort={agent.abort}
          onSetPermissionMode={agent.setPermissionMode}
          onSetScreenshots={agent.setScreenshots}
        />
      </div>

      {/* Modals */}
      {agent.pendingApproval && (
        <ToolApprovalModal
          tools={agent.pendingApproval.tools}
          onApprove={agent.approveTools}
          onDenyAll={agent.denyAllTools}
        />
      )}

      {agent.pendingQuestion && (
        <QuestionPicker
          questions={agent.pendingQuestion.questions}
          onSubmit={agent.answerQuestion}
        />
      )}

      {agent.pendingPlanReview && (
        <PlanReviewer
          title={agent.pendingPlanReview.title}
          plan={agent.pendingPlanReview.plan}
          tasks={agent.pendingPlanReview.tasks}
          onAccept={agent.acceptPlan}
          onReject={agent.rejectPlan}
        />
      )}

      <SubAgentViewer
        open={subAgentViewerOpen}
        agents={agent.subAgents}
        detail={agent.subAgentDetail}
        onViewDetail={agent.getSubAgentDetail}
        onAbort={agent.abortSubAgent}
        onDismissDetail={agent.dismissSubAgentDetail}
        onClose={() => {
          setSubAgentViewerOpen(false);
          agent.dismissSubAgentDetail();
        }}
      />

      <WorkersPanel
        open={workersPanelOpen}
        workers={agent.workers}
        detail={agent.workerDetail}
        runDetail={agent.workerRunDetail}
        onViewDetail={agent.getWorkerDetail}
        onViewRunDetail={agent.getWorkerRunDetail}
        onToggle={agent.toggleWorker}
        onCreate={agent.createWorker}
        onDelete={agent.deleteWorker}
        onDismissDetail={agent.dismissWorkerDetail}
        onClose={() => {
          setWorkersPanelOpen(false);
          agent.dismissWorkerDetail();
        }}
      />

      {/* Worker notification toast */}
      {agent.workerNotification && (
        <div
          className={`fixed bottom-4 right-4 z-50 max-w-sm rounded-lg border px-4 py-3 shadow-lg backdrop-blur-sm animate-fade-in ${
            agent.workerNotification.status === "success"
              ? "border-green-500/30 bg-green-950/90 text-green-200"
              : "border-red-500/30 bg-red-950/90 text-red-200"
          }`}
        >
          <div className="flex items-center gap-2 mb-1">
            <span className="text-xs font-medium">
              {agent.workerNotification.status === "success" ? "Worker completed" : "Worker error"}
            </span>
            <span className="text-[10px] opacity-70">{agent.workerNotification.workerName}</span>
          </div>
          <p className="text-xs opacity-80 line-clamp-2">{agent.workerNotification.summary}</p>
        </div>
      )}
    </div>
  );
}
