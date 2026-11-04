import {
  ArrowDown,
  ArrowDownUp,
  ArrowLeftRight,
  ArrowUp,
  CheckCircle2,
  Clock,
  Database,
  Download,
  FolderPlus,
  MoreHorizontal,
  Plus,
  RefreshCw,
  ShieldCheck,
  Upload,
} from "lucide-react";
import { useMemo } from "react";
import { api, pickFiles, pickFolders } from "@/lib/api";
import { keys, useAction, useAccounts, useBackups, useHistory, useSyncPairs, useTransfers, useUsage } from "@/lib/hooks";
import { SyncStateBadge } from "./sync";
import type { HistoryEntry } from "@/lib/types";
import { cn, formatBytes, greeting, percent, timeAgo } from "@/lib/utils";
import { useUi } from "@/store/ui";
import { FileBrowser } from "@/components/file-browser";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Menu } from "@/components/ui/menu";
import { Progress } from "@/components/ui/progress";
import { SkyLogo } from "@/components/layout";
import { localizeError, t as tr, useT } from "@/i18n";
import { PROVIDER_NAMES } from "@/lib/types";

export function Dashboard() {
  const account = useAccounts().data?.[0];
  const usage = useUsage().data;
  const transfers = useTransfers().data ?? [];
  const backups = useBackups().data ?? [];
  const syncPairs = useSyncPairs().data ?? [];
  const reviewing = syncPairs.filter((p) => p.state.kind === "needsReview");
  const stats = useUi((s) => s.stats);
  const ui = useUi();
  const firstName = account?.displayName?.split(" ")[0];
  const t = useT();
  const provider = account ? PROVIDER_NAMES[account.provider] : "Telia Sky";

  const running = transfers.filter((t) => t.state === "running" || t.state === "queued");
  const runningBytes = running.reduce((a, t) => a + t.bytesDone, 0);
  const runningTotal = running.reduce((a, t) => a + (t.bytesTotal ?? 0), 0);
  const recent = useMemo(() => {
    const dayAgo = Date.now() - 86_400_000;
    const done = transfers.filter((t) => t.state === "completed" && new Date(t.startedAt).getTime() > dayAgo);
    const sum = (dir: string) => done.filter((t) => t.direction === dir);
    return {
      up: sum("upload"),
      down: sum("download"),
      bytes: (dir: string) => sum(dir).reduce((a, t) => a + (t.bytesTotal ?? t.bytesDone), 0),
    };
  }, [transfers]);
  const activeBackups = backups.filter((b) => b.enabled);
  const healthy = activeBackups.every((b) => !b.lastError) && reviewing.length === 0;

  const upload = useAction((p: string[]) => api.upload(p, ui.cwd), [keys.transfers], t("files.toast.uploadStarted"));

  return (
    <div className="flex min-h-0 flex-1 flex-col gap-5 overflow-auto p-6">
      <section className="flex items-center gap-5">
        <div className="grid size-16 place-items-center rounded-full bg-accent-soft">
          <SkyLogo className="size-11" />
        </div>
        <div className="flex-1">
          <h1 className="text-[28px] font-bold tracking-tight">
            {greeting()}
            {firstName ? `, ${firstName}` : ""}
          </h1>
          <p className="text-fg-muted">
            {running.length
              ? t("dashboard.syncingItems", { count: running.length })
              : healthy
                ? t("dashboard.allGood")
                : t("dashboard.needsAttention")}
          </p>
        </div>
        <Button variant="outline" size="lg" onClick={async () => upload.mutate(await pickFiles(t("files.uploadTo", { provider })))}>
          <Upload /> {t("dashboard.upload")}
        </Button>
        <Button variant="primary" size="lg" onClick={() => ui.setView("files")} title={t("dashboard.downloadHint")}>
          <Download /> {t("dashboard.download")}
        </Button>
        <Menu
          trigger={(p) => (
            <Button size="lg" {...p}>
              <Plus /> {t("dashboard.new")}
            </Button>
          )}
          items={[
            { label: t("dashboard.backupFolder"), icon: <ShieldCheck />, onSelect: () => ui.setView("backups") },
            { label: t("dashboard.uploadFolder"), icon: <FolderPlus />, onSelect: async () => upload.mutate(await pickFolders(t("dashboard.uploadFolder"))) },
          ]}
        />
      </section>

      <section className="grid grid-cols-4 gap-4">
        <Card>
          <CardHeader>
            <Database className="size-5 text-fg-muted" />
            <CardTitle>{t("dashboard.storage")}</CardTitle>
          </CardHeader>
          <CardContent className="space-y-3">
            <p className="text-sm text-fg-muted">
              <span className="font-semibold text-fg">{formatBytes(usage?.used)}</span>
              {usage?.total ? t("dashboard.ofUsed", { total: formatBytes(usage.total) }) : t("dashboard.used")}
            </p>
            {usage?.total ? (
              <div className="flex items-center gap-3">
                <Progress value={percent(usage.used, usage.total)} label={t("dashboard.storageUsed")} />
                <span className="text-sm tabular-nums text-fg-muted">{percent(usage.used, usage.total)}%</span>
              </div>
            ) : (
              // Telia Sky reports only space used (seen on a real account),
              // e.g. for plans with unlimited storage.
              usage && <p className="text-xs text-fg-subtle">{t("dashboard.noLimit")}</p>
            )}
          </CardContent>
        </Card>
        <Card>
          <CardHeader>
            <RefreshCw className="size-5 text-accent" />
            <CardTitle>{t("dashboard.syncStatus")}</CardTitle>
            {reviewing.length ? (
              <Badge tone="warning" className="ml-auto">{t("dashboard.attention")}</Badge>
            ) : running.length || syncPairs.some((p) => p.state.kind === "syncing") ? (
              <Badge tone="accent" className="ml-auto">{t("dashboard.syncingBadge")}</Badge>
            ) : (
              <Badge tone="success" className="ml-auto">{t("dashboard.upToDateBadge")}</Badge>
            )}
          </CardHeader>
          <CardContent className="space-y-3">
            <p className="text-sm text-fg-muted">
              {running.length
                ? t("dashboard.filesRemaining", {
                    count: stats && stats.totalTransfers > stats.transfers ? stats.totalTransfers - stats.transfers : running.length,
                    size: formatBytes(runningTotal - runningBytes),
                  })
                : t("dashboard.nothingWaiting")}
            </p>
            <Progress value={running.length ? percent(runningBytes, runningTotal) : 100} label={t("dashboard.syncProgress")} />
          </CardContent>
        </Card>
        <Card>
          <CardHeader>
            <ArrowDownUp className="size-5 text-fg-muted" />
            <CardTitle>{t("dashboard.recentTransfers")}</CardTitle>
          </CardHeader>
          <CardContent className="space-y-1.5 text-sm">
            <p className="flex items-center gap-2">
              <ArrowUp className="size-4 text-success" /> {t("dashboard.uploads", { count: recent.up.length, size: formatBytes(recent.bytes("upload")) })}
            </p>
            <p className="flex items-center gap-2">
              <ArrowDown className="size-4 text-success" /> {t("dashboard.downloads", { count: recent.down.length, size: formatBytes(recent.bytes("download")) })}
            </p>
          </CardContent>
        </Card>
        <Card>
          <CardHeader>
            <ShieldCheck className="size-5 text-success" />
            <CardTitle>{t("dashboard.protectedFolders")}</CardTitle>
          </CardHeader>
          <CardContent className="space-y-2">
            <p className="text-sm text-fg-muted">
              {t("dashboard.folders", { count: activeBackups.length })} •{" "}
              {activeBackups.length ? t("dashboard.backupActive") : t("dashboard.noBackups")}
            </p>
            {activeBackups.length > 0 &&
              (healthy ? (
                <Badge tone="success">
                  <CheckCircle2 /> {t("dashboard.protected")}
                </Badge>
              ) : (
                <Badge tone="warning">{t("dashboard.attention")}</Badge>
              ))}
          </CardContent>
        </Card>
      </section>

      <section className="grid min-h-[420px] flex-1 grid-cols-[1fr_390px] gap-4">
        <FileBrowser compact />
        <div className="flex flex-col gap-4">
          <Card>
            <CardHeader className="border-b border-border">
              <RefreshCw className="size-5 text-accent" />
              <CardTitle>{t("dashboard.syncBackup")}</CardTitle>
              <Button variant="ghost" size="icon" className="ml-auto" aria-label={t("dashboard.openBackups")} onClick={() => ui.setView("backups")}>
                <MoreHorizontal />
              </Button>
            </CardHeader>
            <CardContent className="space-y-3 pt-4 text-sm">
              <div className="flex items-end justify-between">
                <div>
                  <p className="font-semibold">{t("dashboard.background")}</p>
                  <p className="text-fg-muted">
                    {running.length ? t("dashboard.uploadingChanges", { provider }) : t("dashboard.watching")}
                  </p>
                </div>
                <span className="font-semibold tabular-nums">
                  {running.length ? `${percent(runningBytes, runningTotal)}%` : ""}
                </span>
              </div>
              <Progress value={running.length ? percent(runningBytes, runningTotal) : 100} />
              <ul className="space-y-2 pt-1">
                {syncPairs.slice(0, 2).map((p) => (
                  <li key={p.id} className="flex items-center gap-2">
                    <ArrowLeftRight className="size-4 text-fg-muted" />
                    <button className="truncate hover:text-accent" onClick={() => ui.setView("sync")}>
                      {p.name}
                    </button>
                    <span className="ml-auto">
                      <SyncStateBadge pair={p} />
                    </span>
                  </li>
                ))}
                {activeBackups.slice(0, Math.max(1, 3 - syncPairs.length)).map((b) => (
                  <li key={b.id} className="flex items-center gap-2">
                    <ShieldCheck className="size-4 text-fg-muted" />
                    <span className="truncate">{b.name}</span>
                    <span className="ml-auto text-xs text-fg-subtle">{timeAgo(b.lastSuccess)}</span>
                    {b.lastError ? (
                      <span className="size-2 rounded-full bg-warning" />
                    ) : (
                      <CheckCircle2 className="size-4 text-success" />
                    )}
                  </li>
                ))}
                {!syncPairs.length && (
                  <li>
                    <Button variant="link" onClick={() => ui.setView("sync")}>
                      {t("dashboard.syncCta")}
                    </Button>
                  </li>
                )}
                {!activeBackups.length && (
                  <li>
                    <Button variant="link" onClick={() => ui.setView("backups")}>
                      {t("dashboard.protectCta")}
                    </Button>
                  </li>
                )}
              </ul>
            </CardContent>
          </Card>
          <RecentActivity onMore={() => ui.setView("transfers")} />
        </div>
      </section>
    </div>
  );
}

export function describeHistory(h: HistoryEntry): { icon: "up" | "down" | "ok" | "warn" | "sync"; title: string; detail: string } {
  const e = h.event;
  const name = (p: string) => p.split("/").filter(Boolean).pop() ?? p;
  switch (e.type) {
    case "transferUpdated": {
      const x = e.data;
      const up = x.direction === "upload";
      if (x.state === "failed")
        return { icon: "warn", title: tr(up ? "activity.uploadFailed" : "activity.downloadFailed"), detail: name(x.source) };
      if (x.state === "cancelled") return { icon: "warn", title: tr("activity.cancelled"), detail: name(x.source) };
      return { icon: up ? "up" : "down", title: tr(up ? "activity.uploaded" : "activity.downloaded"), detail: name(x.source) };
    }
    case "backupFinished":
      return e.data.lastError
        ? { icon: "warn", title: tr("activity.backupAttention"), detail: e.data.name }
        : { icon: "ok", title: tr("activity.backupDone"), detail: e.data.name };
    case "syncFinished": {
      const st = e.data.state.kind;
      if (st === "needsReview") return { icon: "warn", title: tr("activity.syncReview"), detail: e.data.name };
      if (st === "error") return { icon: "warn", title: tr("activity.syncFailed"), detail: e.data.name };
      return { icon: "sync", title: tr("activity.synced"), detail: e.data.name };
    }
    case "accountConnected":
      return { icon: "ok", title: tr("activity.accountConnected"), detail: e.data.displayName ?? PROVIDER_NAMES[e.data.provider] };
    case "accountRemoved":
      return { icon: "warn", title: tr("activity.signedOut"), detail: "" };
    case "authExpired":
      return { icon: "warn", title: tr("activity.signInExpired"), detail: tr("activity.signInAgainToResume") };
    case "error":
      return { icon: "warn", ...(({ title, message }) => ({ title, detail: message }))(localizeError(e.data)) };
    default:
      return { icon: "sync", title: e.type, detail: "" };
  }
}

export function ActivityIcon({ kind }: { kind: ReturnType<typeof describeHistory>["icon"] }) {
  const cls = "size-5";
  return (
    <span
      className={cn(
        "grid size-9 shrink-0 place-items-center rounded-full",
        kind === "warn" ? "bg-warning-soft text-warning" : kind === "sync" ? "bg-accent-soft text-accent" : "bg-success-soft text-success",
        kind === "down" && "bg-accent-soft text-accent",
      )}
    >
      {kind === "up" ? <ArrowUp className={cls} /> : kind === "down" ? <ArrowDown className={cls} /> : kind === "sync" ? <RefreshCw className={cls} /> : kind === "warn" ? <span className="font-bold">!</span> : <CheckCircle2 className={cls} />}
    </span>
  );
}

function RecentActivity({ onMore }: { onMore: () => void }) {
  const history = useHistory().data ?? [];
  const transfers = useTransfers().data ?? [];
  const running = transfers.find((x) => x.state === "running");
  const t = useT();
  return (
    <Card className="flex-1">
      <CardHeader className="border-b border-border">
        <Clock className="size-5 text-fg-muted" />
        <CardTitle>{t("dashboard.recentActivity")}</CardTitle>
        <Button variant="link" className="ml-auto" onClick={onMore}>
          {t("dashboard.transferHistory")}
        </Button>
      </CardHeader>
      <CardContent className="divide-y divide-border pt-1">
        {running && (
          <div className="flex items-center gap-3 py-2.5 text-sm">
            <ActivityIcon kind="sync" />
            <div className="min-w-0">
              <p className="truncate">
                <span className="font-semibold">{t("dashboard.syncingItem")}</span> {running.source.split("/").pop()}
              </p>
              <p className="text-xs text-fg-muted">
                {t("dashboard.remainingInProgress", { size: formatBytes((running.bytesTotal ?? 0) - running.bytesDone) })}
              </p>
            </div>
          </div>
        )}
        {history.slice(0, running ? 2 : 3).map((h) => {
          const d = describeHistory(h);
          const size = h.event.type === "transferUpdated" ? formatBytes(h.event.data.bytesTotal) + " • " : "";
          return (
            <div key={h.id} className="flex items-center gap-3 py-2.5 text-sm">
              <ActivityIcon kind={d.icon} />
              <div className="min-w-0">
                <p className="truncate">
                  <span className="font-semibold">{d.title}</span> {d.detail}
                </p>
                <p className="text-xs text-fg-muted">
                  {size}
                  {timeAgo(h.at)}
                </p>
              </div>
            </div>
          );
        })}
        {!history.length && !running && <p className="py-6 text-center text-sm text-fg-muted">{t("dashboard.noActivity")}</p>}
      </CardContent>
    </Card>
  );
}

