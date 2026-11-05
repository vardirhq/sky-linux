// The UI's only way to reach Sky Core. Inside Tauri, requests go to the Rust
// side (`daemon_request` command), which forwards them to sky-daemon over the
// local unix socket. Outside Tauri (plain `npm run dev`, screenshots, UI
// tests) a self-contained demo backend is used instead.

import type {
  Account,
  BackupJob,
  CloudEntry,
  DaemonStatus,
  Diagnostics,
  HistoryEntry,
  NewBackupJob,
  NewSyncPair,
  ReviewAction,
  SyncPair,
  ProviderInfo,
  Provider,
  SkyEvent,
  StorageUsage,
  Transfer,
  UserError,
} from "./types";

export interface Transport {
  request<T>(method: string, params?: unknown): Promise<T>;
  onEvent(cb: (e: SkyEvent) => void): () => void;
}

export class SkyError extends Error {
  constructor(public readonly info: UserError) {
    super(info.message);
  }
}

export const isTauri = () =>
  typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;

function toUserError(e: unknown): UserError {
  if (e && typeof e === "object" && "code" in e && "message" in e) return e as UserError;
  return {
    code: "unexpected",
    title: "Something went wrong",
    message: String(e),
    action: "retry",
    technical: String(e),
  };
}

class TauriTransport implements Transport {
  async request<T>(method: string, params?: unknown): Promise<T> {
    const { invoke } = await import("@tauri-apps/api/core");
    try {
      return await invoke<T>("daemon_request", { method, params: params ?? null });
    } catch (e) {
      throw new SkyError(toUserError(e));
    }
  }
  onEvent(cb: (e: SkyEvent) => void) {
    let unlisten: (() => void) | undefined;
    let cancelled = false;
    import("@tauri-apps/api/event").then(({ listen }) =>
      listen<SkyEvent>("sky://event", (msg) => cb(msg.payload)).then((u) => {
        if (cancelled) u();
        else unlisten = u;
      }),
    );
    return () => {
      cancelled = true;
      unlisten?.();
    };
  }
}

// Cache the promise, not the result: concurrent first calls must share one
// transport (otherwise the demo backend would be created several times).
let transport: Promise<Transport> | null = null;

export function getTransport(): Promise<Transport> {
  transport ??= isTauri()
    ? Promise.resolve(new TauriTransport())
    : import("./demo").then(({ DemoTransport }) => new DemoTransport());
  return transport;
}

/** For tests. */
export function setTransport(t: Transport) {
  transport = Promise.resolve(t);
}

async function call<T>(method: string, params?: unknown): Promise<T> {
  return (await getTransport()).request<T>(method, params);
}

export const api = {
  status: () => call<DaemonStatus>("daemon.status"),
  diagnostics: () => call<Diagnostics>("daemon.diagnostics"),
  providers: () => call<ProviderInfo[]>("accounts.providers"),
  accounts: () => call<Account[]>("accounts.list"),
  connect: (provider: Provider) => call<Account>("accounts.connect", { provider }),
  disconnect: (id: string) => call<void>("accounts.disconnect", { id }),

  list: (path: string, refresh = false) => call<CloudEntry[]>("files.list", { path, refresh }),
  search: (query: string) => call<CloudEntry[]>("files.search", { query }),
  usage: () => call<StorageUsage>("files.usage", {}),
  mkdir: (path: string) => call<void>("files.mkdir", { path }),
  move: (from: string, to: string, isDir: boolean) => call<void>("files.move", { from, to, isDir }),
  deleteFile: (path: string) => call<void>("files.delete", { path }),

  upload: (localPaths: string[], remoteDir: string) =>
    call<Transfer[]>("transfers.upload", { localPaths, remoteDir }),
  download: (entries: CloudEntry[], localDir: string) =>
    call<Transfer[]>("transfers.download", { entries, localDir }),
  transfers: (limit = 100) => call<Transfer[]>("transfers.list", { limit }),
  cancelTransfer: (id: string) => call<void>("transfers.cancel", { id }),

  backups: () => call<BackupJob[]>("backups.list"),
  createBackup: (job: NewBackupJob) => call<BackupJob>("backups.create", job),
  setBackupEnabled: (id: string, enabled: boolean) =>
    call<BackupJob>("backups.setEnabled", { id, enabled }),
  deleteBackup: (id: string) => call<void>("backups.delete", { id }),
  runBackup: (id: string) => call<BackupJob>("backups.runNow", { id }),

  syncPairs: () => call<SyncPair[]>("sync.list"),
  createSync: (pair: NewSyncPair) => call<SyncPair>("sync.create", pair),
  setSyncEnabled: (id: string, enabled: boolean) => call<SyncPair>("sync.setEnabled", { id, enabled }),
  deleteSync: (id: string) => call<void>("sync.delete", { id }),
  runSync: (id: string) => call<SyncPair>("sync.runNow", { id }),
  resolveSync: (id: string, action: ReviewAction) => call<SyncPair>("sync.resolve", { id, action }),

  history: (limit = 100) => call<HistoryEntry[]>("history.list", { limit }),
  getSetting: <T>(key: string) => call<T | null>("settings.get", { key }),
  setSetting: (key: string, value: unknown) => call<void>("settings.set", { key, value }),
};

export async function onEvent(cb: (e: SkyEvent) => void): Promise<() => void> {
  return (await getTransport()).onEvent(cb);
}

// ---- native helpers (no-ops / browser fallbacks outside Tauri) -------------

export async function pickFolders(title: string): Promise<string[]> {
  if (!isTauri()) {
    const p = window.prompt(`${title}\n(demo mode: type a folder path)`, "/home/ola/Pictures");
    return p ? [p] : [];
  }
  const { open } = await import("@tauri-apps/plugin-dialog");
  const r = await open({ directory: true, multiple: true, title });
  return r == null ? [] : Array.isArray(r) ? r : [r];
}

export async function pickFiles(title: string): Promise<string[]> {
  if (!isTauri()) {
    const p = window.prompt(`${title}\n(demo mode: type a file path)`, "/home/ola/Desktop/report.pdf");
    return p ? [p] : [];
  }
  const { open } = await import("@tauri-apps/plugin-dialog");
  const r = await open({ directory: false, multiple: true, title });
  return r == null ? [] : Array.isArray(r) ? r : [r];
}

export async function openExternal(url: string) {
  if (!isTauri()) {
    window.open(url, "_blank", "noopener");
    return;
  }
  const { openUrl } = await import("@tauri-apps/plugin-opener");
  await openUrl(url);
}

export async function homeDir(): Promise<string> {
  if (!isTauri()) return "/home/ola";
  const { homeDir } = await import("@tauri-apps/api/path");
  return (await homeDir()).replace(/\/+$/, "");
}

/** Open a local folder in the file manager. */
export async function openFolder(path: string) {
  if (!isTauri()) {
    window.alert(path);
    return;
  }
  const { openPath } = await import("@tauri-apps/plugin-opener");
  await openPath(path);
}

/** Copy text to the clipboard. Falls back to execCommand where the
 * asynchronous clipboard API isn't available (some WebKitGTK versions). */
export async function copyText(text: string): Promise<void> {
  try {
    await navigator.clipboard.writeText(text);
    return;
  } catch {
    /* fall through */
  }
  const area = document.createElement("textarea");
  area.value = text;
  area.style.position = "fixed";
  area.style.opacity = "0";
  document.body.appendChild(area);
  area.select();
  const ok = document.execCommand("copy");
  area.remove();
  if (!ok) throw new Error("clipboard unavailable");
}

export async function defaultDownloadDir(): Promise<string> {
  const saved = await api.getSetting<string>("downloadDir").catch(() => null);
  if (saved) return saved;
  if (!isTauri()) return "/home/ola/Downloads";
  const { downloadDir } = await import("@tauri-apps/api/path");
  return downloadDir();
}
