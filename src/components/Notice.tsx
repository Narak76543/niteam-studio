import { CircleAlert, CircleCheck, Info, TriangleAlert } from "lucide-react";
import type { ReactNode } from "react";

type Tone = "neutral" | "warn" | "danger" | "ok";

const ICONS: Record<Tone, ReactNode> = {
  neutral: <Info size={16} strokeWidth={1.5} />,
  warn: <TriangleAlert size={16} strokeWidth={1.5} />,
  danger: <CircleAlert size={16} strokeWidth={1.5} />,
  ok: <CircleCheck size={16} strokeWidth={1.5} />,
};

interface Props {
  tone?: Tone;
  children: ReactNode;
  actions?: ReactNode;
}

export function Notice({ tone = "neutral", children, actions }: Props) {
  return (
    <div className={tone === "neutral" ? "notice" : `notice notice--${tone}`} role={tone === "danger" ? "alert" : "status"}>
      <span className="notice__icon">{ICONS[tone]}</span>
      <div className="notice__body">{children}</div>
      {actions && <div className="notice__actions">{actions}</div>}
    </div>
  );
}
