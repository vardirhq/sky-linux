import { beforeAll, describe, expect, it } from "vitest";
import { useLocale } from "@/i18n";

beforeAll(() => useLocale.getState().setLocale("en"));
import { formatBytes, formatEta, joinPath, parentPath, percent, tildePath, timeAgo } from "./utils";
import { entryStatus } from "@/components/file-browser";
import type { BackupJob, CloudEntry, SyncPair, Transfer } from "./types";

describe("formatting", () => {
  it("formats sizes with decimal units", () => {
    expect(formatBytes(0)).toBe("0 B");
    expect(formatBytes(3_700_000)).toBe("3.7 MB");
    expect(formatBytes(1.4e12)).toBe("1.4 TB");
    expect(formatBytes(null)).toBe("—");
  });
  it("formats eta and relative time", () => {
    expect(formatEta(30)).toBe("30s left");
    expect(formatEta(3900)).toBe("1 h 5 min left");
    const now = Date.parse("2025-01-23T10:24:00Z");
    expect(timeAgo("2025-01-23T10:22:00Z", now)).toBe("2 min ago");
    expect(timeAgo(null)).toBe("never");
  });
  it("clamps percentages", () => {
    expect(percent(5, 10)).toBe(50);
    expect(percent(20, 10)).toBe(100);
    expect(percent(1, null)).toBe(0);
  });
  it("abbreviates the home folder", () => {
    expect(tildePath("/home/ola/Telia Sky", "/home/ola")).toBe("~/Telia Sky");
    expect(tildePath("/home/ola2/x", "/home/ola")).toBe("/home/ola2/x");
    expect(tildePath("/srv/x", null)).toBe("/srv/x");
  });

  it("joins and splits cloud paths", () => {
    expect(joinPath("/Photos/", "a.jpg")).toBe("Photos/a.jpg");
    expect(joinPath("", "a.jpg")).toBe("a.jpg");
    expect(parentPath("Photos/2025/a.jpg")).toBe("Photos/2025");
    expect(parentPath("a.jpg")).toBe("");
  });
});

describe("entryStatus", () => {
  const folder = (path: string): CloudEntry => ({ name: path, path, kind: "folder", size: null, modified: null, mimeType: null });
  const job = { remotePath: "Backup/Pictures", enabled: true } as BackupJob;
  const upload = { state: "running", direction: "upload", destination: "Photos/2025/x.jpg", source: "/x.jpg" } as Transfer;

  it("marks folders with an active upload as syncing", () => {
    expect(entryStatus(folder("Photos"), [], [upload])).toBe("syncing");
  });
  it("marks backup destinations and their parents as protected", () => {
    expect(entryStatus(folder("Backup"), [job], [])).toBe("protected");
    expect(entryStatus(folder("Backup/Pictures/2024"), [job], [])).toBe("protected");
  });
  it("marks folders inside a synced pair as synced, and its parents as syncing while it runs", () => {
    const pair = { remotePath: "Telia Sky", enabled: true, state: { kind: "idle" } } as SyncPair;
    expect(entryStatus(folder("Telia Sky"), [], [], [pair])).toBe("synced");
    expect(entryStatus(folder("Telia Sky/Notes"), [], [], [pair])).toBe("synced");
    expect(entryStatus(folder("Telia Sky 2"), [], [], [pair])).toBe("cloud");
    const running = { ...pair, state: { kind: "syncing" } } as SyncPair;
    expect(entryStatus(folder("Telia Sky"), [], [], [running])).toBe("syncing");
    expect(entryStatus(folder("Telia Sky"), [], [], [{ ...pair, enabled: false }])).toBe("cloud");
  });

  it("does not confuse sibling prefixes", () => {
    expect(entryStatus(folder("Backup/Pictures2"), [job], [])).toBe("cloud");
  });
});
