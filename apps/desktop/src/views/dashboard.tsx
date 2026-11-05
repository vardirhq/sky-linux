import {
  ArrowDown,
  ArrowLeftRight,
  ArrowUp,
  CheckCircle2,
  Clock,
  Database,
  FolderPlus,
  RefreshCw,
  ShieldCheck,
  TriangleAlert,
  Upload,
} from "lucide-react";
import { api, pickFiles } from "@/lib/api";
import { keys, useAction, useAccounts, useBackups, useHistory, useHome, useSyncPairs, useTransfers, useUsage } from "@/lib/hooks";
import { SyncStateBadge } from "./sync";
import { canWriteIn, DEFAULT_AREA, hasAreas } from "@/lib/areas";
import type { HistoryEntry } from "@/lib/types";
import { cn, formatBytes, greeting, percent, tildePath, timeAgo } from "@/lib/utils";
import { useUi } from "@/store/ui";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Progress } from "@/components/ui/progress";
import { localizeError, t as tr, useT } from "@/i18n";
import { PROVIDER_NAMES } from "@/lib/types";

type Health =
  | { kind: "attention"; lines: string[] }
  | { kind: "busy"; count: number; done: number; total: number }
  | { kind: "safe"; when: string | null }
  | { kind: "empty" };

export function Dashboard() {
  const account = useAccounts().data?.[0];
  const transfers = useTransfers().data ?? [];
  const backups = useBackups().data ?? [];
  const syncPairs = useSyncPairs().data ?? [];
  const ui = useUi();
  const firstName = account?.displayName?.split(" ")[0];
  const t = useT();
  const provider = account ? PROVIDER_NAMES[account.provider] : "Telia Sky";

  const running = transfers.filter((x) => x.state === "running" || x.state === "queued");
  const health: Health = (() => {
    const failing = backups.filter((b) => b.enabled && b.lastError);
    const review = syncPairs.filter((p) => p.state.kind === "needsReview" || p.state.kind === "error");
    const lines = [
      ...(ui.authExpired ? [t("dashboard.lineSignIn", { provider })] : []),
      ...review.map((p) => t("dashboard.lineSync", { name: p.name })),
      ...failing.map((b) => t("dashboard.lineBackup", { name: b.name })),
    ];
    if (lines.length) return { kind: "attention", lines };
    if (running.length)
      return {
        kind: "busy",
        count: running.length,
        done: running.reduce((a, x) => a + x.bytesDone, 0),
        total: running.reduce((a, x) => a + (x.bytesTotal ?? 0), 0),
      };
    if (!backups.length && !syncPairs.length) return { kind: "empty" };
    const last = [...backups.map((b) => b.lastSuccess), ...syncPairs.map((p) => p.lastSync)].filter(Boolean).sort().pop() ?? null;
    return { kind: "safe", when: last };
  })();

  // Uploads go to the open folder, or to Archive when it can't take files
  // (the list of storage areas, or a read-only area such as Photos).
  const uploadDir = canWriteIn(account, ui.cwd) ? ui.cwd : hasAreas(account) ? DEFAULT_AREA : ui.cwd;
  const upload = useAction((p: string[]) => api.upload(p, uploadDir), [keys.transfers], t("files.toast.uploadStarted"));

  return (
    <div className="flex min-h-0 flex-1 flex-col gap-6 overflow-auto p-8">
      <section className="flex items-center gap-4">
        <h1 className="flex-1 text-[28px] font-bold tracking-tight">
          {greeting()}
          {firstName ? `, ${firstName}` : ""}
        </h1>
        <Button variant="primary" size="lg" onClick={async () => upload.mutate(await pickFiles(t("files.uploadTo", { provider })))}>
          <Upload /> {t("dashboard.upload")}
        </Button>
      </section>

      <HealthCard health={health} onFolders={() => ui.setView("folders")} />

      <section className="grid gap-6 lg:grid-cols-[1fr_380px]">
        <MyFolders onManage={() => ui.setView("folders")} />
        <div className="flex flex-col gap-6">
          <StorageCard />
          <RecentActivity onMore={() => ui.setView("activity")} />
        </div>
      </section>
    </div>
  );
}

/** The one thing to know: is everything safe? */
function HealthCard({ health, onFolders }: { health: Health; onFolders: () => void }) {
  const t = useT();
  const tone =
    health.kind === "attention"
      ? "border-warning/40 bg-warning-soft"
      : health.kind === "safe"
        ? "border-success/30 bg-success-soft"
        : "border-border bg-surface";
  return (
    <Card className={cn("flex items-center gap-5 p-6", tone)}>
      <span className="grid size-14 shrink-0 place-items-center rounded-full bg-surface/70">
        {health.kind === "attention" ? (
          <TriangleAlert className="size-7 text-warning" />
        ) : health.kind === "busy" ? (
          <RefreshCw className="size-7 animate-spin text-accent [animation-duration:2.5s]" />
        ) : health.kind === "safe" ? (
          <ShieldCheck className="size-7 text-success" />
        ) : (
          <FolderPlus className="size-7 text-accent" />
        )}
      </span>
      <div className="min-w-0 flex-1">
        {health.kind === "attention" && (
          <>
            <p className="text-lg font-semibold">{t("dashboard.attentionTitle")}</p>
            <ul className="text-fg-muted">
              {health.lines.map((l) => (
                <li key={l}>{l}</li>
              ))}
            </ul>
          </>
        )}
        {health.kind === "busy" && (
          <>
            <p className="text-lg font-semibold">{t("dashboard.syncingItems", { count: health.count })}</p>
            <div className="mt-2 flex items-center gap-3">
              <Progress value={percent(health.done, health.total)} label={t("dashboard.syncProgress")} />
              <span className="shrink-0 text-sm tabular-nums text-fg-muted">
                {t("dashboard.remaining", { size: formatBytes(health.total - health.done) })}
              </span>
            </div>
          </>
        )}
        {health.kind === "safe" && (
          <>
            <p className="text-lg font-semibold">{t("dashboard.safeTitle")}</p>
            <p className="text-fg-muted">{health.when ? t("dashboard.safeWhen", { when: timeAgo(health.when) }) : t("dashboard.safeWaiting")}</p>
          </>
        )}
        {health.kind === "empty" && (
          <>
            <p className="text-lg font-semibold">{t("dashboard.emptyTitle")}</p>
            <p className="text-fg-muted">{t("dashboard.emptyBody")}</p>
          </>
        )}
      </div>
      {(health.kind === "attention" || health.kind === "empty") && (
        <Button variant={health.kind === "empty" ? "primary" : "outline"} onClick={onFolders}>
          {health.kind === "empty" ? t("dashboard.chooseFolders") : t("dashboard.seeFolders")}
        </Button>
      )}
    </Card>
  );
}

function MyFolders({ onManage }: { onManage: () => void }) {
  const t = useT();
  const home = useHome().data;
  const backups = useBackups().data ?? [];
  const syncPairs = useSyncPairs().data ?? [];
  const rows = [
    ...syncPairs.map((p) => ({
      id: p.id,
      icon: <ArrowLeftRight className="size-5 text-accent" />,
      name: p.name,
      path: tildePath(p.localPath, home),
      status: <SyncStateBadge pair={p} />,
    })),
    ...backups.map((b) => ({
      id: b.id,
      icon: <ShieldCheck className="size-5 text-success" />,
      name: b.name,
      path: tildePath(b.localPath, home),
      status: !b.enabled ? (
        <Badge>{t("sync.state.paused")}</Badge>
      ) : b.lastError ? (
        <Badge tone="warning">{t("dashboard.attention")}</Badge>
      ) : (
        <span className="text-xs text-fg-muted">{b.lastSuccess ? timeAgo(b.lastSuccess) : t("backups.waiting")}</span>
      ),
    })),
  ];
  return (
    <Card>
      <CardHeader className="border-b border-border">
        <CardTitle>{t("dashboard.myFolders")}</CardTitle>
        <Button variant="link" className="ml-auto" onClick={onManage}>
          {t("dashboard.manage")}
        </Button>
      </CardHeader>
      <CardContent className="divide-y divide-border pt-1">
        {rows.map((r) => (
          <div key={r.id} className="flex items-center gap-3 py-3 text-sm">
            {r.icon}
            <div className="min-w-0 flex-1">
              <p className="truncate font-medium">{r.name}</p>
              <p className="truncate text-xs text-fg-muted">{r.path}</p>
            </div>
            {r.status}
          </div>
        ))}
        {!rows.length && <p className="py-6 text-center text-sm text-fg-muted">{t("dashboard.noFolders")}</p>}
      </CardContent>
    </Card>
  );
}

function StorageCard() {
  const usage = useUsage().data;
  const t = useT();
  return (
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
          <Progress value={percent(usage.used, usage.total)} label={t("dashboard.storageUsed")} />
        ) : (
          // Telia Sky reports only space used (seen on a real account),
          // e.g. for plans with unlimited storage.
          usage && <p className="text-xs text-fg-subtle">{t("dashboard.noLimit")}</p>
        )}
      </CardContent>
    </Card>
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
          {t("dashboard.allActivity")}
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

