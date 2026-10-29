import { useCallback, useEffect, useRef, useState } from "react";

import { errorMessage, updateCheck, updateInstall, type Update } from "./api";

/** In-app update state (docs/12-updates.md). */
export type UpdateState =
  | { kind: "idle" }
  | { kind: "checking" }
  | { kind: "upToDate" }
  | { kind: "available"; update: Update }
  | { kind: "downloading"; update: Update; done: number; total: number | null }
  | { kind: "installing"; update: Update }
  | { kind: "checkFailed" }
  | { kind: "installFailed"; update: Update; message: string };

export interface Updater {
  state: UpdateState;
  /** The newer version, while one is known (available, downloading, installing, failed install). */
  update: Update | null;
  /** `silent`: a failed check stays quiet (start-up check). */
  check: (silent?: boolean) => Promise<void>;
  install: () => Promise<void>;
  dialogOpen: boolean;
  openDialog: () => void;
  closeDialog: () => void;
  /** Hide the sidebar notice for this version until the next app start. */
  dismissed: boolean;
  dismiss: () => void;
}

/** Wait after start-up so the first check never competes with loading the app. */
const STARTUP_DELAY_MS = 4_000;

export function useUpdater(): Updater {
  const [state, setState] = useState<UpdateState>({ kind: "idle" });
  const [dialogOpen, setDialogOpen] = useState(false);
  const [dismissed, setDismissed] = useState(false);
  const busy = useRef(false);

  const check = useCallback(async (silent = false) => {
    if (busy.current) return;
    busy.current = true;
    if (!silent) setState({ kind: "checking" });
    try {
      const found = await updateCheck();
      setState(found ? { kind: "available", update: found } : { kind: "upToDate" });
    } catch {
      // Offline, GitHub down, or no latest.json published yet.
      setState(silent ? { kind: "idle" } : { kind: "checkFailed" });
    } finally {
      busy.current = false;
    }
  }, []);

  const install = useCallback(async () => {
    const update =
      state.kind === "available" || state.kind === "installFailed" ? state.update : null;
    if (!update || busy.current) return;
    busy.current = true;
    let done = 0;
    let total: number | null = null;
    setState({ kind: "downloading", update, done, total });
    try {
      await updateInstall(update, (e) => {
        if (e.event === "Started") total = e.data.contentLength ?? null;
        else if (e.event === "Progress") done += e.data.chunkLength;
        if (e.event === "Finished") setState({ kind: "installing", update });
        else setState({ kind: "downloading", update, done, total });
      });
    } catch (e) {
      setState({ kind: "installFailed", update, message: errorMessage(e) });
    } finally {
      busy.current = false;
    }
  }, [state]);

  useEffect(() => {
    const id = window.setTimeout(() => void check(true), STARTUP_DELAY_MS);
    return () => window.clearTimeout(id);
  }, [check]);

  const update =
    state.kind === "available" ||
    state.kind === "downloading" ||
    state.kind === "installing" ||
    state.kind === "installFailed"
      ? state.update
      : null;

  return {
    state,
    update,
    check,
    install,
    dialogOpen,
    openDialog: useCallback(() => setDialogOpen(true), []),
    closeDialog: useCallback(() => setDialogOpen(false), []),
    dismissed,
    dismiss: useCallback(() => setDismissed(true), []),
  };
}
