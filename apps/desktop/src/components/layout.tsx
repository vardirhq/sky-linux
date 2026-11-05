import { EDITION } from "@/edition";
import { useEffect, useRef } from "react";
import {
  ArrowDown,
  ArrowDownUp,
  ArrowUp,
  Bell,
  CheckCircle2,
  Cloud,
  Clock,
  Folder,
  History,
  LayoutDashboard,
  Laptop,
  RefreshCw,
  Search,
  Settings,
  ShieldCheck,
  TriangleAlert,
  X,
} from "lucide-react";
import { useAccounts, useBackups, useStatus } from "@/lib/hooks";
import { cn, formatSpeed, timeAgo } from "@/lib/utils";
import { useUi, type View } from "@/store/ui";
import { Button } from "./ui/button";
import { openExternal } from "@/lib/api";
import { WindowControls } from "./window-controls";
import { localizeError, useT, type Key } from "@/i18n";
import { PROVIDER_NAMES } from "@/lib/types";

const NAV: { id: View; label: Key; icon: typeof Folder }[] = [
  { id: "dashboard", label: "nav.dashboard", icon: LayoutDashboard },
  { id: "files", label: "nav.files", icon: Folder },
  { id: "backups", label: "nav.backups", icon: ShieldCheck },
  { id: "sync", label: "nav.sync", icon: RefreshCw },
  { id: "transfers", label: "nav.transfers", icon: ArrowDownUp },
  { id: "history", label: "nav.history", icon: History },
  { id: "devices", label: "nav.devices", icon: Laptop },
];

export function SkyLogo({ className }: { className?: string }) {
  return <img src={EDITION.id === "telia" ? "/sky-telia.svg" : "/sky.svg"} alt="" className={cn("size-10", className)} />;
}

export function Sidebar() {
  const { view, setView } = useUi();
  const status = useStatus().data;
  const account = useAccounts().data?.[0];
  const t = useT();
  const item = (n: (typeof NAV)[number]) => (
    <button
      key={n.id}
      onClick={() => setView(n.id)}
      aria-current={view === n.id ? "page" : undefined}
      className={cn(
        "flex w-full items-center gap-3 rounded-xl px-3.5 py-2.5 text-[15px] text-fg hover:bg-surface-muted",
        view === n.id && "bg-accent-soft font-semibold text-fg hover:bg-accent-soft",
      )}
    >
      <n.icon className="size-5" strokeWidth={1.8} />
      {t(n.label)}
    </button>
  );
  return (
    <aside className="flex w-56 shrink-0 flex-col gap-1 border-r border-border bg-surface px-3 py-3">
      <nav className="flex flex-col gap-1" aria-label={t("nav.main")}>
        {NAV.map(item)}
        <div className="my-2 border-t border-border" />
        {item({ id: "settings", label: "nav.settings", icon: Settings })}
      </nav>
      <div className="mt-auto space-y-3 px-2 pb-1 text-sm">
        {account && (
          <div className="flex items-center gap-2.5">
            <Cloud className="size-8 text-accent" strokeWidth={1.6} />
            <div>
              <div className="font-semibold">{PROVIDER_NAMES[account.provider]}</div>
              <div className="text-xs text-fg-muted">{t("app.via")}</div>
            </div>
          </div>
        )}
        <div className="flex items-center gap-2 text-fg-muted">
          <span className={cn("size-2.5 rounded-full", status?.rcloneRunning ? "bg-success" : "bg-warning")} />
          {status?.rcloneRunning ? t("app.connected") : t("app.reconnecting")}
        </div>
        <div className="text-xs text-fg-subtle">{t("app.version", { version: status?.version ?? "—" })}</div>
      </div>
    </aside>
  );
}

export function TopBar() {
  const { search, setSearch, setView, view } = useUi();
  const account = useAccounts().data?.[0];
  const t = useT();
  const ref = useRef<HTMLInputElement>(null);
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === "k") {
        e.preventDefault();
        ref.current?.focus();
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);
  const providerName = account ? PROVIDER_NAMES[account.provider] : "";
  const initials = (account?.displayName ?? providerName ?? "?")
    .split(" ")
    .map((s) => s[0])
    .join("")
    .slice(0, 2);
  return (
    <header data-tauri-drag-region className="flex h-16 shrink-0 select-none items-center gap-6 border-b border-border bg-surface pr-3 pl-5">
      <div data-tauri-drag-region className="flex w-60 shrink-0 items-center gap-3">
        <SkyLogo />
        <div className="leading-tight">
          <div className="text-lg font-bold tracking-tight">{EDITION.appName}</div>
          <div className="text-xs text-fg-muted">{t(EDITION.tagline)}</div>
        </div>
      </div>
      <label className="relative mx-auto flex w-full max-w-lg items-center">
        <Search className="pointer-events-none absolute left-3 size-4 text-fg-muted" />
        <input
          ref={ref}
          value={search}
          onChange={(e) => {
            setSearch(e.target.value);
            if (view !== "files" && view !== "dashboard") setView("files");
          }}
          placeholder={t("top.search")}
          aria-label={t("top.searchLabel")}
          className="h-10 w-full rounded-xl border border-border bg-surface-muted pl-10 pr-9 text-sm outline-none focus:border-accent focus:bg-surface"
        />
        {search && (
          <button aria-label={t("top.clearSearch")} className="absolute right-3 text-fg-muted" onClick={() => setSearch("")}>
            <X className="size-4" />
          </button>
        )}
      </label>
      <Button variant="ghost" size="icon" aria-label={t("top.activity")} onClick={() => setView("history")}>
        <Bell className="size-5" />
      </Button>
      {account && (
        <button
          onClick={() => setView("settings")}
          className="flex items-center gap-2.5 rounded-xl border border-border px-3 py-1.5 hover:bg-surface-muted"
        >
          <span className="grid size-8 place-items-center rounded-full bg-gradient-to-br from-[var(--color-avatar-from)] to-[var(--color-avatar-to)] text-xs font-semibold text-white">
            {initials}
          </span>
          <span className="text-left leading-tight">
            <span className="block text-sm font-semibold">{account.displayName ?? providerName}</span>
            <span className="block text-xs text-fg-muted">{account.email ?? (account.displayName ? providerName : t("app.connected"))}</span>
          </span>
        </button>
      )}
      <span className="h-6 border-l border-border" aria-hidden />
      <WindowControls />
    </header>
  );
}

export function StatusBar() {
  const { stats, rcloneRunning } = useUi();
  const t = useT();
  const backups = useBackups().data ?? [];
  const last = backups
    .map((b) => b.lastSuccess)
    .filter(Boolean)
    .sort()
    .pop();
  const failing = backups.filter((b) => b.lastError).length;
  const busy = (stats?.speed ?? 0) > 0;
  return (
    <footer className="flex h-11 shrink-0 items-center gap-5 border-t border-border bg-surface px-5 text-sm text-fg-muted">
      {rcloneRunning && !failing ? (
        <span className="flex items-center gap-2">
          <CheckCircle2 className="size-4 text-success" /> {t("status.allGood")}
        </span>
      ) : (
        <span className="flex items-center gap-2 text-warning">
          <TriangleAlert className="size-4" />
          {rcloneRunning ? t("status.backupsAttention", { count: failing }) : t("status.serviceRestarting")}
        </span>
      )}
      <span className="h-4 border-l border-border" />
      <span className="flex items-center gap-2">
        <Clock className="size-4" /> {t("status.lastBackup", { when: timeAgo(last) })}
      </span>
      <span className="ml-auto flex items-center gap-1.5 tabular-nums">
        <ArrowUp className="size-4" /> {formatSpeed(busy ? stats?.speed : 0)}
      </span>
      <span className="flex items-center gap-1.5 tabular-nums">
        <ArrowDown className="size-4" /> {formatSpeed(0)}
      </span>
      <span className="flex items-center gap-1.5">
        <RefreshCw className={cn("size-4", busy && "animate-spin [animation-duration:2.5s]")} />
        {busy ? t("status.syncing") : t("status.upToDate")}
      </span>
    </footer>
  );
}

export function Toasts() {
  const { toasts, dismiss } = useUi();
  const t = useT();
  return (
    <div className="pointer-events-none fixed right-4 bottom-14 z-50 flex w-96 flex-col gap-2" aria-live="polite">
      {toasts.map((toast) => (
        <div
          key={toast.id}
          className={cn(
            "pointer-events-auto rounded-xl border bg-surface p-4 shadow-lg",
            toast.kind === "error" ? "border-danger/40" : "border-border",
          )}
        >
          <div className="flex items-start gap-3">
            {toast.kind === "error" ? (
              <TriangleAlert className="mt-0.5 size-5 text-danger" />
            ) : (
              <CheckCircle2 className="mt-0.5 size-5 text-success" />
            )}
            <div className="min-w-0 flex-1">
              <p className="font-medium">{toast.error ? localizeError(toast.error).title : toast.title}</p>
              {(toast.error || toast.message) && (
                <p className="mt-0.5 text-sm text-fg-muted">
                  {toast.error ? localizeError(toast.error).message : toast.message}
                </p>
              )}
              {toast.error?.technical && (
                <details className="mt-1 text-xs text-fg-subtle">
                  <summary className="cursor-default">{t("toast.details")}</summary>
                  <code className="break-all">{toast.error.technical}</code>
                </details>
              )}
            </div>
            <button aria-label={t("toast.dismiss")} onClick={() => dismiss(toast.id)} className="text-fg-subtle">
              <X className="size-4" />
            </button>
          </div>
        </div>
      ))}
    </div>
  );
}

export function Banners() {
  const { authUrl, authExpired, setView } = useUi();
  const t = useT();
  const account = useAccounts().data?.[0];
  const provider = account ? PROVIDER_NAMES[account.provider] : "Telia Sky";
  if (!authUrl && !authExpired) return null;
  return (
    <div className="flex items-center gap-3 border-b border-warning/30 bg-warning-soft px-5 py-2.5 text-sm">
      <TriangleAlert className="size-4 text-warning" />
      {authExpired ? (
        <>
          <span>{t("banner.expired", { provider })}</span>
          <Button size="sm" variant="primary" className="ml-auto" onClick={() => setView("settings")}>
            {t("banner.signInAgain")}
          </Button>
        </>
      ) : (
        <>
          <span>{t("banner.finishInBrowser")}</span>
          <Button size="sm" variant="link" onClick={() => authUrl && openExternal(authUrl)}>
            {t("banner.openSignIn")}
          </Button>
        </>
      )}
    </div>
  );
}
