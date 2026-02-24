import { useEffect, useState, useCallback, createContext, useContext, type ReactNode } from "react";
import { CheckCircle, XCircle, AlertTriangle } from "lucide-react";

interface ToastMessage {
  id: number;
  type: "success" | "error" | "info";
  message: string;
}

interface ToastContextType {
  toast: (type: ToastMessage["type"], message: string) => void;
}

const ToastContext = createContext<ToastContextType>({ toast: () => {} });

export function useToast() {
  return useContext(ToastContext);
}

let nextId = 0;

export function ToastProvider({ children }: { children: ReactNode }) {
  const [toasts, setToasts] = useState<ToastMessage[]>([]);

  const toast = useCallback((type: ToastMessage["type"], message: string) => {
    const id = nextId++;
    setToasts((prev) => [...prev, { id, type, message }]);
  }, []);

  const remove = useCallback((id: number) => {
    setToasts((prev) => prev.filter((t) => t.id !== id));
  }, []);

  return (
    <ToastContext.Provider value={{ toast }}>
      {children}
      <div className="fixed bottom-5 right-5 flex flex-col gap-2.5 z-50">
        {toasts.map((t) => (
          <ToastItem key={t.id} toast={t} onDone={() => remove(t.id)} />
        ))}
      </div>
    </ToastContext.Provider>
  );
}

const accentColors: Record<string, string> = {
  success: "#4ade80",
  error: "#fb7185",
  info: "#fbbf24",
};

function ToastItem({ toast, onDone }: { toast: ToastMessage; onDone: () => void }) {
  useEffect(() => {
    const timer = setTimeout(onDone, 3000);
    return () => clearTimeout(timer);
  }, [onDone]);

  const icons = {
    success: <CheckCircle size={16} style={{ color: "#4ade80" }} />,
    error: <XCircle size={16} style={{ color: "#fb7185" }} />,
    info: <AlertTriangle size={16} style={{ color: "#fbbf24" }} />,
  };

  return (
    <div
      className="flex items-center gap-3 px-4 py-3 rounded-xl text-sm"
      style={{
        background: "rgba(20, 20, 20, 0.95)",
        backdropFilter: "blur(12px)",
        border: "1px solid var(--border)",
        borderLeft: `3px solid ${accentColors[toast.type]}`,
        color: "var(--text-primary)",
        minWidth: "300px",
        boxShadow: "var(--shadow-lg)",
        animation: "slideIn 0.3s cubic-bezier(0.4, 0, 0.2, 1)",
      }}
    >
      {icons[toast.type]}
      {toast.message}
    </div>
  );
}
