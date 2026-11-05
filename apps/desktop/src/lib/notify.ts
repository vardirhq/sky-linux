// Desktop notifications for things that need the user while the window is
// in the background: a backup that started failing, a sync that stopped to
// protect files, and an expired sign-in. Each problem is announced once,
// when it starts, not on every retry.

import { localizeError, t } from "@/i18n";
import { isTauri } from "./api";
import type { SkyEvent } from "./types";

export interface Note {
  title: string;
  body: string;
}

export class NotificationPolicy {
  /** Ids of backups and sync pairs whose current problem was announced. */
  private announced = new Set<string>();
  private authAnnounced = false;

  decide(e: SkyEvent): Note | null {
    switch (e.type) {
      case "backupFinished": {
        const job = e.data;
        if (!job.lastError) {
          this.announced.delete(job.id);
          return null;
        }
        if (!this.first(job.id)) return null;
        return { title: t("notify.backupFailed", { name: job.name }), body: localizeError(job.lastError).message };
      }
      case "syncFinished": {
        const pair = e.data;
        if (pair.state.kind === "needsReview") {
          if (!this.first(pair.id)) return null;
          return { title: t("notify.syncReview", { name: pair.name }), body: t("notify.syncReviewBody") };
        }
        if (pair.state.kind === "error" && pair.lastError) {
          if (!this.first(pair.id)) return null;
          return { title: t("notify.syncFailed", { name: pair.name }), body: localizeError(pair.lastError).message };
        }
        this.announced.delete(pair.id);
        return null;
      }
      case "authExpired":
        if (this.authAnnounced) return null;
        this.authAnnounced = true;
        return { title: t("notify.authExpired"), body: t("notify.authExpiredBody") };
      case "accountConnected":
        this.authAnnounced = false;
        return null;
      case "backupJobRemoved":
      case "syncPairRemoved":
        this.announced.delete(e.data.id);
        return null;
      default:
        return null;
    }
  }

  private first(id: string): boolean {
    if (this.announced.has(id)) return false;
    this.announced.add(id);
    return true;
  }
}

/** Show a notification through the desktop's notification service. */
export async function showNotification(note: Note): Promise<void> {
  if (!isTauri()) return;
  const n = await import("@tauri-apps/plugin-notification");
  let granted = await n.isPermissionGranted();
  if (!granted) granted = (await n.requestPermission()) === "granted";
  if (granted) n.sendNotification({ title: note.title, body: note.body });
}
