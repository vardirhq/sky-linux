import { useEffect } from "react";
import { useMutation, useQuery, useQueryClient, type QueryClient } from "@tanstack/react-query";
import { api, homeDir, onEvent, SkyError } from "./api";
import type { Transfer } from "./types";
import { useUi } from "@/store/ui";

export const keys = {
  status: ["status"] as const,
  accounts: ["accounts"] as const,
  usage: ["usage"] as const,
  list: (path: string) => ["list", path] as const,
  search: (q: string) => ["search", q] as const,
  transfers: ["transfers"] as const,
  backups: ["backups"] as const,
  sync: ["sync"] as const,
  history: ["history"] as const,
  setting: (k: string) => ["setting", k] as const,
};

export const useStatus = () => useQuery({ queryKey: keys.status, queryFn: api.status, refetchInterval: 15_000 });
export const useAccounts = () => useQuery({ queryKey: keys.accounts, queryFn: api.accounts });
export const useUsage = () => useQuery({ queryKey: keys.usage, queryFn: api.usage, refetchInterval: 60_000 });
export const useListing = (path: string) => useQuery({ queryKey: keys.list(path), queryFn: () => api.list(path) });
export const useSearch = (q: string) =>
  useQuery({ queryKey: keys.search(q), queryFn: () => api.search(q), enabled: q.trim().length >= 2 });
export const useTransfers = () => useQuery({ queryKey: keys.transfers, queryFn: () => api.transfers(200) });
export const useBackups = () => useQuery({ queryKey: keys.backups, queryFn: api.backups });
/** The user's home folder (for showing paths as ~/…). */
export const useHome = () => useQuery({ queryKey: ["home"], queryFn: homeDir, staleTime: Infinity });

export const useSyncPairs = () => useQuery({ queryKey: keys.sync, queryFn: api.syncPairs });
export const useHistory = () => useQuery({ queryKey: keys.history, queryFn: () => api.history(200) });
export const useSetting = <T,>(key: string) =>
  useQuery({ queryKey: keys.setting(key), queryFn: () => api.getSetting<T>(key) });

/** Wrap a mutation with user-friendly error toasts and cache invalidation. */
export function useAction<A, R>(fn: (a: A) => Promise<R>, invalidate: readonly (readonly unknown[])[], success?: string) {
  const qc = useQueryClient();
  const toast = useUi((s) => s.toast);
  return useMutation({
    mutationFn: fn,
    onSuccess: () => {
      invalidate.forEach((k) => qc.invalidateQueries({ queryKey: k }));
      if (success) toast({ kind: "success", title: success });
    },
    onError: (e) => {
      const info = e instanceof SkyError ? e.info : undefined;
      toast({ kind: "error", title: info?.title ?? "Something went wrong", message: info?.message ?? String(e), error: info });
    },
  });
}

function upsertTransfer(qc: QueryClient, t: Transfer) {
  qc.setQueryData<Transfer[]>(keys.transfers, (old) => {
    if (!old) return [t];
    const i = old.findIndex((x) => x.id === t.id);
    if (i < 0) return [t, ...old];
    const next = old.slice();
    next[i] = t;
    return next;
  });
}

/** Subscribe once to daemon events and fold them into the query cache. */
export function useDaemonEvents() {
  const qc = useQueryClient();
  useEffect(() => {
    const ui = useUi.getState();
    let unsub: (() => void) | undefined;
    let alive = true;
    onEvent((e) => {
      switch (e.type) {
        case "stats":
          ui.setStats(e.data);
          break;
        case "transferUpdated":
          upsertTransfer(qc, e.data);
          if (e.data.state === "completed" || e.data.state === "failed") {
            qc.invalidateQueries({ queryKey: keys.usage });
            qc.invalidateQueries({ queryKey: keys.history });
            qc.invalidateQueries({ queryKey: ["list"] });
          }
          if (e.data.state === "failed" && e.data.error) {
            ui.toast({ kind: "error", title: e.data.error.title, message: e.data.error.message, error: e.data.error });
          }
          break;
        case "rcloneStatus":
          ui.setRcloneRunning(e.data.running);
          break;
        case "authUrl":
          ui.setAuthUrl(e.data.url);
          break;
        case "authExpired":
          ui.setAuthExpired(true);
          break;
        case "accountConnected":
        case "accountRemoved":
          ui.setAuthUrl(null);
          ui.setAuthExpired(false);
          qc.invalidateQueries();
          break;
        case "backupStarted":
        case "backupFinished":
        case "backupJobChanged":
        case "backupJobRemoved":
          qc.invalidateQueries({ queryKey: keys.backups });
          qc.invalidateQueries({ queryKey: keys.history });
          break;
        case "syncStarted":
        case "syncFinished":
        case "syncPairChanged":
        case "syncPairRemoved":
          qc.invalidateQueries({ queryKey: keys.sync });
          if (e.type === "syncFinished") {
            qc.invalidateQueries({ queryKey: ["list"] });
            qc.invalidateQueries({ queryKey: keys.history });
          }
          break;
        case "error":
          ui.toast({ kind: "error", title: e.data.title, message: e.data.message, error: e.data });
          break;
      }
    }).then((u) => {
      if (alive) unsub = u;
      else u();
    });
    return () => {
      alive = false;
      unsub?.();
    };
  }, [qc]);
}
