// TypeScript mirrors of Sky Core's models (crates/sky-core/src/models.rs).
// Keep in sync; the IPC protocol is JSON with camelCase fields.

export type Provider =
  | "telia_sky_no"
  | "telia_cloud_se"
  | "jottacloud"
  | "elkjop_no"
  | "tele2_se"
  | "onlime"
  | "phonero_sky";

export const PROVIDER_NAMES: Record<Provider, string> = {
  telia_sky_no: "Telia Sky",
  telia_cloud_se: "Telia Cloud",
  jottacloud: "Jottacloud",
  elkjop_no: "Elkjøp Cloud",
  tele2_se: "Tele2 Cloud",
  onlime: "Onlime",
  phonero_sky: "Phonero Sky",
};

export interface ProviderInfo {
  id: Provider;
  name: string;
  supported: boolean;
}

export interface Account {
  id: string;
  provider: Provider;
  remote: string;
  displayName: string | null;
  email: string | null;
  connectedAt: string;
}

export type EntryKind = "file" | "folder";

export interface CloudEntry {
  name: string;
  path: string;
  kind: EntryKind;
  size: number | null;
  modified: string | null;
  mimeType: string | null;
}

export interface StorageUsage {
  used: number;
  total: number | null;
  free: number | null;
  trashed: number | null;
}

export type TransferDirection = "upload" | "download";
export type TransferState =
  | "queued"
  | "running"
  | "completed"
  | "failed"
  | "cancelled"
  | "interrupted";

export type UserAction =
  | "retry"
  | "sign_in_again"
  | "check_connection"
  | "free_up_space"
  | "check_permissions"
  | "contact_support"
  | "none";

export interface UserError {
  code: string;
  title: string;
  message: string;
  action: UserAction;
  technical: string;
  /** Stable rule code for `invalid` / `safety` errors (translated in the UI). */
  rule?: string;
  params?: Record<string, string>;
}

export interface Transfer {
  id: string;
  accountId: string;
  direction: TransferDirection;
  source: string;
  destination: string;
  state: TransferState;
  bytesTotal: number | null;
  bytesDone: number;
  speed: number | null;
  etaSeconds: number | null;
  error: UserError | null;
  rcloneJob: number | null;
  backupJob: string | null;
  startedAt: string;
  finishedAt: string | null;
}

export type Schedule =
  | { kind: "onChange"; debounceSeconds: number; fallbackHours: number }
  | { kind: "interval"; minutes: number }
  | { kind: "manual" };

export interface BackupJob {
  id: string;
  accountId: string;
  name: string;
  localPath: string;
  remotePath: string;
  schedule: Schedule;
  enabled: boolean;
  exclude: string[];
  lastRun: string | null;
  lastSuccess: string | null;
  lastError: UserError | null;
  createdAt: string;
}

export interface NewBackupJob {
  accountId: string;
  name?: string | null;
  localPath: string;
  remotePath: string;
  schedule?: Schedule;
  exclude?: string[];
}

export interface LiveStats {
  bytes: number;
  totalBytes: number;
  speed: number;
  etaSeconds: number | null;
  transfers: number;
  totalTransfers: number;
  errors: number;
}

export interface DaemonStatus {
  version: string;
  rcloneVersion: string | null;
  rcloneRunning: boolean;
  accounts: Account[];
  activeTransfers: number;
  backupJobs: number;
}

export type SkyEvent =
  | { type: "rcloneStatus"; data: { running: boolean } }
  | { type: "authUrl"; data: { url: string } }
  | { type: "accountConnected"; data: Account }
  | { type: "accountRemoved"; data: { id: string } }
  | { type: "authExpired"; data: { accountId: string } }
  | { type: "transferUpdated"; data: Transfer }
  | { type: "stats"; data: LiveStats }
  | { type: "backupStarted"; data: { jobId: string } }
  | { type: "backupFinished"; data: BackupJob }
  | { type: "backupJobChanged"; data: BackupJob }
  | { type: "backupJobRemoved"; data: { id: string } }
  | { type: "error"; data: UserError };

export interface HistoryEntry {
  id: number;
  at: string;
  event: SkyEvent;
}
