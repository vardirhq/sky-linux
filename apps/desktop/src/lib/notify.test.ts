import { describe, expect, it } from "vitest";
import { NotificationPolicy } from "./notify";
import type { BackupJob, SkyEvent, SyncPair, UserError } from "./types";

const err: UserError = { code: "offline", title: "Offline", message: "No connection", action: "retry", technical: "x" };
const job = (lastError: UserError | null): BackupJob => ({
  id: "b1", accountId: "a", name: "Bilder", localPath: "/home/k/Pictures", remotePath: "Archive/Pictures",
  schedule: { kind: "manual" }, enabled: true, exclude: [], lastRun: null, lastSuccess: null, lastError, createdAt: "",
});
const pair = (state: SyncPair["state"], lastError: UserError | null = null): SyncPair => ({
  id: "s1", accountId: "a", name: "Telia Sky", localPath: "/home/k/Telia Sky", remotePath: "Sync", enabled: true,
  maxDeletePercent: 50, pollMinutes: 5, state, lastSync: null, lastSummary: null, lastError, recoveryPath: "", createdAt: "",
});

describe("NotificationPolicy", () => {
  it("announces a failing backup once, and again after it recovers and fails", () => {
    const p = new NotificationPolicy();
    const fail: SkyEvent = { type: "backupFinished", data: job(err) };
    expect(p.decide(fail)?.title).toContain("Bilder");
    expect(p.decide(fail)).toBeNull();
    expect(p.decide({ type: "backupFinished", data: job(null) })).toBeNull();
    expect(p.decide(fail)).not.toBeNull();
  });

  it("announces a sync that needs review, and sync errors", () => {
    const p = new NotificationPolicy();
    const review: SkyEvent = { type: "syncFinished", data: pair({ kind: "needsReview", reason: "too_many_deletes" }) };
    expect(p.decide(review)).not.toBeNull();
    expect(p.decide(review)).toBeNull();
    expect(p.decide({ type: "syncFinished", data: pair({ kind: "idle" }) })).toBeNull();
    expect(p.decide({ type: "syncFinished", data: pair({ kind: "error" }, err) })?.body).toBeTruthy();
  });

  it("announces an expired sign-in once until the account is connected again", () => {
    const p = new NotificationPolicy();
    const expired: SkyEvent = { type: "authExpired", data: { accountId: "a" } };
    expect(p.decide(expired)).not.toBeNull();
    expect(p.decide(expired)).toBeNull();
    p.decide({ type: "accountConnected", data: {} as never });
    expect(p.decide(expired)).not.toBeNull();
  });

  it("ignores progress events", () => {
    const p = new NotificationPolicy();
    expect(p.decide({ type: "backupStarted", data: { jobId: "b1" } })).toBeNull();
  });
});
