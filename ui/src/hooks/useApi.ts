/**
 * Data hooks.
 *
 * A run is started, then polled while it judges — the server reports how many
 * typed calls it has made, so the wait shows progress rather than a spinner
 * with nothing behind it.
 */

import { useCallback, useEffect, useRef, useState } from "react";
import { ApiError, getInfo, getRun, listRuns } from "../api/client";
import type { RunSummary, RunView, ServerInfo } from "../api/types";

/** How often a running run is re-read. */
const POLL_MS = 700;

/** Whatever the API said went wrong, as a sentence. */
export function errorMessage(error: unknown): string {
  if (error instanceof ApiError) return error.message;
  return error instanceof Error ? error.message : String(error);
}

/** What the server can do, read once. */
export function useServerInfo(): { info: ServerInfo | null; error: string | null } {
  const [info, setInfo] = useState<ServerInfo | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let live = true;
    getInfo()
      .then((value) => live && setInfo(value))
      .catch((cause) => live && setError(errorMessage(cause)));
    return () => {
      live = false;
    };
  }, []);

  return { info, error };
}

/** The run list, refreshed on demand and while anything is still judging. */
export function useRunList(): {
  runs: RunSummary[];
  refresh: () => void;
} {
  const [runs, setRuns] = useState<RunSummary[]>([]);

  const refresh = useCallback(() => {
    listRuns()
      .then(setRuns)
      .catch(() => {
        /* the panel that owns the run reports the failure; a stale list is harmless */
      });
  }, []);

  useEffect(refresh, [refresh]);

  const anyRunning = runs.some((run) => run.status === "running");
  useEffect(() => {
    if (!anyRunning) return;
    const timer = window.setInterval(refresh, POLL_MS);
    return () => window.clearInterval(timer);
  }, [anyRunning, refresh]);

  return { runs, refresh };
}

/** One run, polled until it stops running. */
export function useRun(id: string | null): {
  run: RunView | null;
  error: string | null;
  reload: () => void;
} {
  const [run, setRun] = useState<RunView | null>(null);
  const [error, setError] = useState<string | null>(null);
  // Keeps the poll from writing a stale run over a newer selection.
  const wanted = useRef<string | null>(id);
  wanted.current = id;

  const load = useCallback(() => {
    if (!id) {
      setRun(null);
      setError(null);
      return;
    }
    getRun(id)
      .then((value) => {
        if (wanted.current === id) {
          setRun(value);
          setError(null);
        }
      })
      .catch((cause) => {
        if (wanted.current === id) setError(errorMessage(cause));
      });
  }, [id]);

  useEffect(() => {
    setRun(null);
    load();
  }, [load]);

  const running = run?.status === "running";
  useEffect(() => {
    if (!running) return;
    const timer = window.setInterval(load, POLL_MS);
    return () => window.clearInterval(timer);
  }, [running, load]);

  return { run, error, reload: load };
}

/** Fetch once, whenever `key` changes. Used by the detail drawers. */
export function useFetch<T>(
  fetcher: () => Promise<T>,
  key: string | null,
): { data: T | null; error: string | null; loading: boolean } {
  const [data, setData] = useState<T | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);
  const run = useRef(fetcher);
  run.current = fetcher;

  useEffect(() => {
    if (key === null) {
      setData(null);
      return;
    }
    let live = true;
    setLoading(true);
    setError(null);
    run
      .current()
      .then((value) => live && setData(value))
      .catch((cause) => live && setError(errorMessage(cause)))
      .finally(() => live && setLoading(false));
    return () => {
      live = false;
    };
  }, [key]);

  return { data, error, loading };
}
