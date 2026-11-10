import type { ReactNode } from "react";

import { IconTile } from "./IconTile";

interface Props {
  icon: ReactNode;
  title: string;
  body: string;
  action?: ReactNode;
  /** Extra content between the text and the action (e.g. a short how-it-works list). */
  extra?: ReactNode;
}

export function EmptyState({ icon, title, body, action, extra }: Props) {
  return (
    <div className="empty">
      <IconTile>{icon}</IconTile>
      <h3 className="t-h3">{title}</h3>
      <p className="t-small">{body}</p>
      {extra}
      {action}
    </div>
  );
}
