import { clsx, type ClassValue } from "clsx";
import { twMerge } from "tailwind-merge";
import { intlLocale, t } from "@/i18n";

export function cn(...inputs: ClassValue[]) {
  return twMerge(clsx(inputs));
}

const UNITS = ["B", "KB", "MB", "GB", "TB", "PB"];

/** Human-readable size using decimal units, as cloud providers do. */
export function formatBytes(bytes: number | null | undefined, digits = 1): string {
  if (bytes == null || !Number.isFinite(bytes)) return "—";
  const fmt = (n: number, d: number) =>
    n.toLocaleString(intlLocale(), { minimumFractionDigits: d, maximumFractionDigits: d });
  if (bytes < 1000) return `${fmt(bytes, 0)} B`;
  let i = 0;
  let v = bytes;
  while (v >= 1000 && i < UNITS.length - 1) {
    v /= 1000;
    i++;
  }
  return `${fmt(v, v >= 100 ? 0 : digits)} ${UNITS[i]}`;
}

export function formatSpeed(bps: number | null | undefined): string {
  if (!bps) return "0 B/s";
  return `${formatBytes(bps)}/s`;
}

export function formatDate(iso: string | null | undefined, locale = intlLocale()): string {
  if (!iso) return "—";
  const d = new Date(iso);
  return d.toLocaleString(locale, {
    year: "numeric",
    month: "short",
    day: "numeric",
    hour: "2-digit",
    minute: "2-digit",
  });
}

export function timeAgo(iso: string | null | undefined, now = Date.now()): string {
  if (!iso) return t("time.never");
  const s = Math.max(0, Math.round((now - new Date(iso).getTime()) / 1000));
  if (s < 45) return t("time.justNow");
  const m = Math.round(s / 60);
  if (m < 60) return t("time.minutesAgo", { count: m });
  const h = Math.round(m / 60);
  if (h < 24) return t("time.hoursAgo", { count: h });
  const d = Math.round(h / 24);
  return d === 1 ? t("time.yesterday") : t("time.daysAgo", { count: d });
}

export function formatEta(seconds: number | null | undefined): string {
  if (seconds == null) return "";
  if (seconds < 60) return t("time.secondsLeft", { count: seconds });
  const m = Math.round(seconds / 60);
  if (m < 60) return t("time.minutesLeft", { count: m });
  return t("time.hoursLeft", { h: Math.floor(m / 60), m: m % 60 });
}

export function percent(done: number, total: number | null | undefined): number {
  if (!total) return 0;
  return Math.min(100, Math.round((done / total) * 100));
}

export function joinPath(dir: string, name: string): string {
  const d = dir.replace(/^\/+|\/+$/g, "");
  return d ? `${d}/${name}` : name;
}

export function parentPath(path: string): string {
  const p = path.replace(/\/+$/, "");
  const i = p.lastIndexOf("/");
  return i < 0 ? "" : p.slice(0, i);
}

export function greeting(date = new Date()): string {
  const h = date.getHours();
  if (h < 5) return t("greeting.night");
  if (h < 12) return t("greeting.morning");
  if (h < 18) return t("greeting.afternoon");
  return t("greeting.evening");
}
