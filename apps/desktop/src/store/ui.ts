import { create } from "zustand";
import type { LiveStats, UserError } from "@/lib/types";

export type View =
  | "dashboard"
  | "files"
  | "backups"
  | "sync"
  | "transfers"
  | "history"
  | "devices"
  | "settings";

interface Toast {
  id: number;
  kind: "info" | "success" | "error";
  title: string;
  message?: string;
  error?: UserError;
}

interface UiState {
  view: View;
  cwd: string;
  backStack: string[];
  forwardStack: string[];
  viewMode: "list" | "grid";
  search: string;
  selection: Set<string>;
  stats: LiveStats | null;
  rcloneRunning: boolean;
  authUrl: string | null;
  authExpired: boolean;
  toasts: Toast[];

  setView(v: View): void;
  navigate(path: string): void;
  back(): void;
  forward(): void;
  setViewMode(m: "list" | "grid"): void;
  setSearch(s: string): void;
  toggleSelected(path: string): void;
  setSelection(paths: string[]): void;
  setStats(s: LiveStats): void;
  setRcloneRunning(r: boolean): void;
  setAuthUrl(u: string | null): void;
  setAuthExpired(v: boolean): void;
  toast(t: Omit<Toast, "id">): void;
  dismiss(id: number): void;
}

let toastId = 0;

const savedViewMode = (() => {
  try {
    return (localStorage.getItem("sky.viewMode") as "list" | "grid") || "list";
  } catch {
    return "list";
  }
})();

export const useUi = create<UiState>((set, get) => ({
  view: "dashboard",
  cwd: "",
  backStack: [],
  forwardStack: [],
  viewMode: savedViewMode,
  search: "",
  selection: new Set(),
  stats: null,
  rcloneRunning: true,
  authUrl: null,
  authExpired: false,
  toasts: [],

  setView: (view) => set({ view }),
  navigate: (path) => {
    const { cwd, backStack } = get();
    if (path === cwd) return;
    set({ cwd: path, backStack: [...backStack, cwd], forwardStack: [], selection: new Set() });
  },
  back: () => {
    const { backStack, cwd, forwardStack } = get();
    if (!backStack.length) return;
    set({
      cwd: backStack[backStack.length - 1],
      backStack: backStack.slice(0, -1),
      forwardStack: [cwd, ...forwardStack],
      selection: new Set(),
    });
  },
  forward: () => {
    const { forwardStack, cwd, backStack } = get();
    if (!forwardStack.length) return;
    set({
      cwd: forwardStack[0],
      forwardStack: forwardStack.slice(1),
      backStack: [...backStack, cwd],
      selection: new Set(),
    });
  },
  setViewMode: (viewMode) => {
    try {
      localStorage.setItem("sky.viewMode", viewMode);
    } catch {
      /* private mode */
    }
    set({ viewMode });
  },
  setSearch: (search) => set({ search }),
  toggleSelected: (path) => {
    const s = new Set(get().selection);
    if (s.has(path)) s.delete(path);
    else s.add(path);
    set({ selection: s });
  },
  setSelection: (paths) => set({ selection: new Set(paths) }),
  setStats: (stats) => set({ stats }),
  setRcloneRunning: (rcloneRunning) => set({ rcloneRunning }),
  setAuthUrl: (authUrl) => set({ authUrl }),
  setAuthExpired: (authExpired) => set({ authExpired }),
  toast: (t) => {
    const id = ++toastId;
    set({ toasts: [...get().toasts, { ...t, id }] });
    setTimeout(() => get().dismiss(id), t.kind === "error" ? 9000 : 4000);
  },
  dismiss: (id) => set({ toasts: get().toasts.filter((t) => t.id !== id) }),
}));
