// Demo backend used when the UI runs outside Tauri (browser dev server,
// screenshots, UI tests). It mimics sky-daemon's IPC methods with in-memory
// data so designers and contributors can work on the UI without an account.

import type { Transport } from "./api";
import { SkyError } from "./api";
import { canModify, canWriteIn } from "./areas";
import type {
  Account,
  BackupJob,
  CloudEntry,
  HistoryEntry,
  LiveStats,
  NewBackupJob,
  NewSyncPair,
  SyncPair,
  SkyEvent,
  Transfer,
} from "./types";

const GB = 1e9;
const TB = 1e12;
const MB = 1e6;
const now = () => new Date().toISOString();
const ago = (min: number) => new Date(Date.now() - min * 60_000).toISOString();
const uid = () =>
  globalThis.crypto?.randomUUID?.() ?? Math.random().toString(16).slice(2) + Date.now().toString(16);

const account: Account = {
  id: "00000000-0000-4000-8000-000000000001",
  provider: "telia_sky_no",
  remote: "sky-telia-no-all",
  areas: ["Archive", "Photos", "Shared", "Sync"],
  displayName: "Ola Nordmann",
  email: "ola@example.no",
  connectedAt: ago(60 * 24 * 30),
};

function entry(path: string, kind: "file" | "folder", size: number | null, modified: string, mimeType: string | null = null): CloudEntry {
  return { name: path.split("/").pop()!, path, kind, size, modified, mimeType };
}

// Laid out like a real Telia Sky account: one top-level folder per storage
// area (see crates/sky-core/src/areas.rs).
const tree: Record<string, CloudEntry[]> = {
  "": [
    entry("Archive", "folder", null, ago(3)),
    entry("Photos", "folder", null, ago(2)),
    entry("Shared", "folder", null, "2024-11-02T10:00:00Z"),
    entry("Sync", "folder", null, "2024-11-02T10:00:00Z"),
  ],
  Archive: [
    entry("Archive/Telia Sky", "folder", 48 * GB, ago(3)),
    entry("Archive/Documents", "folder", 24.6 * GB, "2025-01-21T15:04:00Z"),
    entry("Archive/Family Backup", "folder", 620 * GB, "2025-01-20T08:27:00Z"),
    entry("Archive/Work", "folder", 310 * GB, "2025-01-22T17:40:00Z"),
    entry("Archive/Holiday plan.pdf", "file", 3.7 * MB, "2025-01-20T10:03:00Z", "application/pdf"),
    entry("Archive/Norway.jpg", "file", 8.9 * MB, "2025-01-21T18:55:00Z", "image/jpeg"),
    entry("Archive/Budget 2025.xlsx", "file", 1.2 * MB, "2025-01-19T09:36:00Z",
      "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet"),
  ],
  Photos: [
    entry("Photos/2024", "folder", 820 * GB, "2024-12-31T12:00:00Z"),
    entry("Photos/2025", "folder", 580 * GB, "2025-01-22T20:18:00Z"),
    entry("Photos/IMG_4827.jpg", "file", 28.4 * MB, ago(2), "image/jpeg"),
  ],
  "Archive/Documents": [
    entry("Archive/Documents/Project brief.pdf", "file", 12.1 * MB, ago(7), "application/pdf"),
    entry("Archive/Documents/Tax 2024.pdf", "file", 2.2 * MB, "2025-01-10T11:00:00Z", "application/pdf"),
    entry("Archive/Documents/Notes.odt", "file", 0.4 * MB, "2025-01-18T08:00:00Z", "application/vnd.oasis.opendocument.text"),
  ],
  Shared: [],
  Sync: [],
};

function syncPair(p: Partial<SyncPair>): SyncPair {
  return {
    id: uid(),
    accountId: account.id,
    name: "Telia Sky",
    localPath: "/home/ola/Telia Sky",
    remotePath: "Archive/Telia Sky",
    enabled: true,
    maxDeletePercent: 50,
    pollMinutes: 5,
    state: { kind: "idle" },
    lastSync: ago(3),
    lastSummary: { uploaded: 4, downloaded: 2, deletedLocal: 0, deletedRemote: 1, conflicts: 1 },
    lastError: null,
    recoveryPath: "/home/ola/.local/share/sky-linux/sync/demo/recovery",
    createdAt: ago(60 * 24 * 14),
    ...p,
  };
}

const syncPairs: SyncPair[] = [syncPair({ id: "00000000-0000-4000-8000-000000000201" })];

const backups: BackupJob[] = [
  { name: "Family Backup", local: "/home/ola/Family", remote: "Archive/Family Backup" },
  { name: "Pictures", local: "/home/ola/Pictures", remote: "Archive/Pictures" },
  { name: "Documents", local: "/home/ola/Documents", remote: "Archive/Documents" },
].map((b, i) => ({
  id: `00000000-0000-4000-8000-00000000010${i}`,
  accountId: account.id,
  name: b.name,
  localPath: b.local,
  remotePath: b.remote,
  schedule: { kind: "onChange", debounceSeconds: 30, fallbackHours: 24 },
  enabled: true,
  exclude: ["*.tmp", ".cache/**"],
  lastRun: ago(60 + i * 5),
  lastSuccess: ago(60 + i * 5),
  lastError: null,
  createdAt: ago(60 * 24 * 30),
}));

function transfer(p: Partial<Transfer>): Transfer {
  return {
    id: uid(),
    accountId: account.id,
    direction: "upload",
    source: "",
    destination: "",
    state: "completed",
    bytesTotal: null,
    bytesDone: 0,
    speed: null,
    etaSeconds: null,
    error: null,
    rcloneJob: null,
    backupJob: null,
    startedAt: now(),
    finishedAt: now(),
    ...p,
  };
}

const transfers: Transfer[] = [
  transfer({
    direction: "upload",
    source: "/home/ola/Pictures",
    destination: "Archive/Pictures",
    state: "running",
    bytesTotal: 15 * GB,
    bytesDone: 10.8 * GB,
    speed: 12.4 * MB,
    etaSeconds: 340,
    backupJob: backups[1].id,
    startedAt: ago(14),
    finishedAt: null,
  }),
  transfer({ source: "/home/ola/Pictures/IMG_4827.jpg", destination: "Archive/Pictures/IMG_4827.jpg", bytesTotal: 28.4 * MB, bytesDone: 28.4 * MB, startedAt: ago(2), finishedAt: ago(2) }),
  transfer({ direction: "download", source: "Archive/Documents/Project brief.pdf", destination: "/home/ola/Downloads/Project brief.pdf", bytesTotal: 12.1 * MB, bytesDone: 12.1 * MB, startedAt: ago(7), finishedAt: ago(7) }),
  transfer({ source: "/home/ola/Work/slides.odp", destination: "Archive/Work/slides.odp", bytesTotal: 540 * MB, bytesDone: 540 * MB, startedAt: ago(40), finishedAt: ago(38) }),
  transfer({ source: "/home/ola/Work/video.mp4", destination: "Archive/Work/video.mp4", bytesTotal: 520 * MB, bytesDone: 520 * MB, startedAt: ago(55), finishedAt: ago(50) }),
  transfer({ direction: "download", source: "Photos/2024/summer.zip", destination: "/home/ola/Downloads/summer.zip", bytesTotal: 568 * MB, bytesDone: 568 * MB, startedAt: ago(80), finishedAt: ago(78) }),
];

const history: HistoryEntry[] = [
  { id: 4, at: ago(2), event: { type: "transferUpdated", data: transfers[1] } },
  { id: 3, at: ago(7), event: { type: "transferUpdated", data: transfers[2] } },
  { id: 2, at: ago(60), event: { type: "backupFinished", data: backups[0] } },
  { id: 1, at: ago(60 * 24 * 30), event: { type: "accountConnected", data: account } },
];

/** Refuse writes like Sky Core does (crates/sky-core/src/areas.rs). */
function refuse(path: string): never {
  const area = path.split("/")[0];
  const readOnly = area === "Photos" || area === "Shared";
  throw new SkyError({
    code: "invalid",
    title: "Something isn't right",
    message: readOnly ? `Sky Linux can't change files in ${area} yet.` : "Choose a folder inside one of your storage areas.",
    action: "none",
    technical: "",
    rule: readOnly ? "area_read_only" : "area_root",
    params: { area },
  });
}

export class DemoTransport implements Transport {
  private listeners = new Set<(e: SkyEvent) => void>();
  private connected = true;
  private settings: Record<string, unknown> = { onboardingComplete: true };

  constructor() {
    if (typeof window !== "undefined") {
      const q = new URLSearchParams(window.location.search);
      if (q.has("review")) {
        syncPairs.push(
          syncPair({
            id: "00000000-0000-4000-8000-000000000202",
            name: "Studier",
            localPath: "/home/ola/Studier",
            remotePath: "Archive/Studier",
            state: { kind: "needsReview", reason: "too_many_deletes" },
            lastSync: ago(90),
            lastSummary: null,
          }),
        );
      }
      if (q.has("onboarding")) {
        this.connected = false;
        this.settings.onboardingComplete = false;
      }
    }
    setInterval(() => this.tick(), 1000);
  }

  private emit(e: SkyEvent) {
    this.listeners.forEach((l) => l(e));
  }

  private tick() {
    let speed = 0;
    for (const t of transfers) {
      if (t.state !== "running" || !t.bytesTotal) continue;
      const s = (t.speed ?? 8 * MB) * (0.85 + Math.random() * 0.3);
      t.speed = s;
      t.bytesDone = Math.min(t.bytesTotal, t.bytesDone + s);
      t.etaSeconds = Math.round((t.bytesTotal - t.bytesDone) / s);
      speed += s;
      if (t.bytesDone >= t.bytesTotal) {
        t.state = "completed";
        t.finishedAt = now();
        t.speed = null;
      }
      this.emit({ type: "transferUpdated", data: { ...t } });
    }
    const running = transfers.filter((t) => t.state === "running");
    const stats: LiveStats = {
      bytes: running.reduce((a, t) => a + t.bytesDone, 0),
      totalBytes: running.reduce((a, t) => a + (t.bytesTotal ?? 0), 0),
      speed,
      etaSeconds: running[0]?.etaSeconds ?? null,
      transfers: 0,
      totalTransfers: 1248,
      errors: 0,
    };
    this.emit({ type: "stats", data: stats });
  }

  onEvent(cb: (e: SkyEvent) => void) {
    this.listeners.add(cb);
    return () => this.listeners.delete(cb);
  }

  async request<T>(method: string, params: any = {}): Promise<T> {
    await new Promise((r) => setTimeout(r, 60));
    return this.handle(method, params) as T;
  }

  private handle(method: string, p: any): unknown {
    switch (method) {
      case "daemon.status":
        return {
          version: "0.1.0-demo",
          rcloneVersion: "v1.75.2",
          rcloneRunning: true,
          accounts: this.connected ? [account] : [],
          activeTransfers: transfers.filter((t) => t.state === "running").length,
          backupJobs: backups.length,
          syncPairs: syncPairs.length,
        };
      case "daemon.diagnostics":
        return {
          generatedAt: now(),
          rcloneRunning: true,
          recentProblems: 0,
          report: [
            "# Sky Linux diagnostics",
            `Generated: ${now()}`,
            "Sky Linux: 0.1.0-demo",
            "rclone: v1.75.2",
            "System: demo",
            `Bandwidth limit: ${this.settings.bandwidthLimit ? `${this.settings.bandwidthLimit} MB/s` : "none"}`,
            "",
            `## Accounts (${this.connected ? 1 : 0})`,
            `## Backups (${backups.length})`,
            ...backups.map((b) => `- ${b.localPath.replace("/home/ola", "~")} → ${b.remotePath}`),
            "",
            "## Recent problems (0)",
          ].join("\n"),
        };
      case "accounts.providers":
        return [
          { id: "telia_sky_no", name: "Telia Sky", supported: true },
          { id: "jottacloud", name: "Jottacloud", supported: false },
          { id: "telia_cloud_se", name: "Telia Cloud", supported: false },
          { id: "elkjop_no", name: "Elkjøp Cloud", supported: false },
        ];
      case "accounts.list":
        return this.connected ? [account] : [];
      case "accounts.connect":
        return new Promise((resolve) =>
          setTimeout(() => {
            this.connected = true;
            this.emit({ type: "accountConnected", data: account });
            resolve(account);
          }, 1500),
        );
      case "accounts.disconnect":
        this.connected = false;
        this.emit({ type: "accountRemoved", data: { id: account.id } });
        return null;
      case "files.list":
        return tree[(p.path ?? "").replace(/^\/+|\/+$/g, "")] ?? [];
      case "files.search": {
        const q = String(p.query ?? "").toLowerCase();
        return Object.values(tree).flat().filter((e) => e.name.toLowerCase().includes(q));
      }
      case "files.usage":
        return { used: 2.8 * TB, total: 5 * TB, free: 2.2 * TB, trashed: 12 * GB };
      case "files.mkdir": {
        if (!canModify(account, p.path)) refuse(p.path);
        const parent = p.path.includes("/") ? p.path.slice(0, p.path.lastIndexOf("/")) : "";
        (tree[parent] ??= []).push(entry(p.path, "folder", null, now()));
        return null;
      }
      case "files.move": {
        if (!canModify(account, p.from)) refuse(p.from);
        if (!canModify(account, p.to)) refuse(p.to);
        for (const list of Object.values(tree)) {
          const e = list.find((x) => x.path === p.from);
          if (e) {
            e.path = p.to;
            e.name = p.to.split("/").pop();
          }
        }
        return null;
      }
      case "files.delete":
        if (!canModify(account, p.path)) refuse(p.path);
        for (const k of Object.keys(tree)) tree[k] = tree[k].filter((e) => e.path !== p.path);
        return null;
      case "transfers.upload":
      case "transfers.download": {
        const up = method === "transfers.upload";
        if (up && !canWriteIn(account, p.remoteDir ?? "")) refuse(p.remoteDir ?? "");
        const items: string[] = up ? p.localPaths : p.entries.map((e: CloudEntry) => e.path);
        const created = items.map((src) => {
          const name = src.split("/").pop()!;
          const t = transfer({
            direction: up ? "upload" : "download",
            source: src,
            destination: up ? `${p.remoteDir ? p.remoteDir + "/" : ""}${name}` : `${p.localDir}/${name}`,
            state: "running",
            bytesTotal: (20 + Math.random() * 200) * MB,
            bytesDone: 0,
            speed: 9 * MB,
            finishedAt: null,
          });
          transfers.unshift(t);
          return t;
        });
        return created;
      }
      case "transfers.list":
        return transfers.slice(0, p.limit ?? 100);
      case "transfers.cancel": {
        const t = transfers.find((x) => x.id === p.id);
        if (t) {
          t.state = "cancelled";
          t.finishedAt = now();
          this.emit({ type: "transferUpdated", data: { ...t } });
        }
        return null;
      }
      case "sync.list":
        return syncPairs;
      case "sync.create": {
        const n = p as NewSyncPair;
        if (n.localPath === "/home/ola")
          throw new SkyError({
            code: "safety",
            title: "Stopped to protect your files",
            message: "Your whole home folder can't be synced.",
            action: "none",
            technical: "",
            rule: "home_folder",
          });
        const pair = syncPair({
          name: n.name || n.localPath.split("/").filter(Boolean).pop() || "Sync",
          localPath: n.localPath,
          remotePath: n.remotePath,
          state: { kind: "new" },
          lastSync: null,
          lastSummary: null,
        });
        syncPairs.push(pair);
        setTimeout(() => {
          pair.state = { kind: "idle" };
          pair.lastSync = now();
          pair.lastSummary = { uploaded: 0, downloaded: 12, deletedLocal: 0, deletedRemote: 0, conflicts: 0 };
          this.emit({ type: "syncFinished", data: pair });
        }, 2500);
        return pair;
      }
      case "sync.setEnabled": {
        const s = syncPairs.find((x) => x.id === p.id)!;
        s.enabled = p.enabled;
        return s;
      }
      case "sync.delete": {
        const i = syncPairs.findIndex((x) => x.id === p.id);
        if (i >= 0) syncPairs.splice(i, 1);
        return null;
      }
      case "sync.runNow":
      case "sync.resolve": {
        const s = syncPairs.find((x) => x.id === p.id)!;
        s.state = { kind: "idle" };
        s.lastSync = now();
        this.emit({ type: "syncFinished", data: s });
        return s;
      }
      case "backups.list":
        return backups;
      case "backups.create": {
        const n = p as NewBackupJob;
        if (n.localPath === "/" || n.localPath === "/etc")
          throw new SkyError({
            code: "safety",
            title: "Stopped to protect your files",
            message: `${n.localPath} is a system folder. Choose a folder with your own files, such as Documents or Pictures.`,
            action: "none",
            technical: "",
            rule: "system_folder",
            params: { path: n.localPath },
          });
        const job: BackupJob = {
          id: uid(),
          accountId: n.accountId,
          name: n.name || n.localPath.split("/").filter(Boolean).pop() || "Backup",
          localPath: n.localPath,
          remotePath: n.remotePath,
          schedule: n.schedule ?? { kind: "onChange", debounceSeconds: 30, fallbackHours: 24 },
          enabled: true,
          exclude: n.exclude ?? [],
          lastRun: null,
          lastSuccess: null,
          lastError: null,
          createdAt: now(),
        };
        backups.push(job);
        this.emit({ type: "backupJobChanged", data: job });
        return job;
      }
      case "backups.setEnabled": {
        const j = backups.find((b) => b.id === p.id)!;
        j.enabled = p.enabled;
        return j;
      }
      case "backups.delete": {
        const i = backups.findIndex((b) => b.id === p.id);
        if (i >= 0) backups.splice(i, 1);
        return null;
      }
      case "backups.runNow": {
        const j = backups.find((b) => b.id === p.id)!;
        j.lastRun = j.lastSuccess = now();
        this.emit({ type: "backupFinished", data: j });
        return j;
      }
      case "history.list":
        return history;
      case "settings.get":
        return this.settings[p.key] ?? null;
      case "settings.set":
        this.settings[p.key] = p.value;
        return null;
      default:
        throw new SkyError({
          code: "invalid",
          title: "Unknown request",
          message: method,
          action: "none",
          technical: method,
        });
    }
  }
}
