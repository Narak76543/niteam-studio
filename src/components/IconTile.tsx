import type { ReactNode } from "react";

/** 32×32 ink tile with a white line icon (card headers, empty states). */
export function IconTile({ children }: { children: ReactNode }) {
  return (
    <span className="icon-tile" aria-hidden="true">
      {children}
    </span>
  );
}
