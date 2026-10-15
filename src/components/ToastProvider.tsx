import { CircleAlert, CircleCheck, Info } from "lucide-react";
import { useCallback, useMemo, useRef, useState, type ReactNode } from "react";

import { ToastContext, type ToastApi, type ToastTone } from "./toast";

const AUTO_HIDE_MS = 4000;

interface Toast {
  id: number;
  message: string;
  tone: ToastTone;
}

const ICON = {
  ok: <CircleCheck size={16} strokeWidth={1.5} />,
  danger: <CircleAlert size={16} strokeWidth={1.5} />,
  neutral: <Info size={16} strokeWidth={1.5} />,
};

export function ToastProvider({ children }: { children: ReactNode }) {
  const [toasts, setToasts] = useState<Toast[]>([]);
  const nextId = useRef(1);

  const show = useCallback((message: string, tone: ToastTone = "neutral") => {
    const id = nextId.current++;
    setToasts((list) => [...list, { id, message, tone }]);
    window.setTimeout(() => setToasts((list) => list.filter((t) => t.id !== id)), AUTO_HIDE_MS);
  }, []);

  const api = useMemo<ToastApi>(() => ({ show }), [show]);

  return (
    <ToastContext.Provider value={api}>
      {children}
      <div className="toasts" aria-live="polite">
        {toasts.map((t) => (
          <div key={t.id} className={`toast toast--${t.tone}`} role={t.tone === "danger" ? "alert" : "status"}>
            <span className="toast__icon">{ICON[t.tone]}</span>
            <span>{t.message}</span>
          </div>
        ))}
      </div>
    </ToastContext.Provider>
  );
}
