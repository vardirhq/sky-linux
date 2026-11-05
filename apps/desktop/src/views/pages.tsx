import { useState } from "react";
import { ArrowDown, ArrowUp, FolderOpen, MoreHorizontal, Pause, Play, RefreshCw, ShieldCheck, Trash2, X } from "lucide-react";
import { api, copyText, openFolder, pickFolders } from "@/lib/api";
import { setAutostart } from "@/lib/autostart";
import { useQuery } from "@tanstack/react-query";
import { useUi } from "@/store/ui";
import { keys, useAccounts, useAction, useHistory, useHome, useSetting, useStatus, useTransfers } from "@/lib/hooks";
import type { BackupJob, Transfer } from "@/lib/types";
import { cn, formatBytes, formatDate, formatEta, formatSpeed, percent, tildePath, timeAgo } from "@/lib/utils";
import { Menu } from "@/components/ui/menu";
import { FileBrowser, EmptyState } from "@/components/file-browser";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Dialog } from "@/components/ui/dialog";
import { Progress } from "@/components/ui/progress";
import { Switch } from "@/components/ui/switch";
import { ActivityIcon, describeHistory } from "./dashboard";
import { LOCALES, isLocale, localizeError, useLocale, useT } from "@/i18n";
import { PROVIDER_NAMES } from "@/lib/types";
import { defaultRemote, displayPath } from "@/lib/areas";

function useProvider() {
  const a = useAccounts().data?.[0];
  return a ? PROVIDER_NAMES[a.provider] : "Telia Sky";
}

export function Page({ title, subtitle, actions, children }: { title: string; subtitle?: string; actions?: React.ReactNode; children: React.ReactNode }) {
  return (
    <div className="flex min-h-0 flex-1 flex-col gap-5 overflow-auto p-6">
      <div className="flex items-end gap-4">
        <div className="flex-1">
          <h1 className="text-2xl font-bold tracking-tight">{title}</h1>
          {subtitle && <p className="text-fg-muted">{subtitle}</p>}
        </div>
        {actions}
      </div>
      {children}
    </div>
  );
}

export function FilesView() {
  return (
    <div className="flex min-h-0 flex-1 flex-col p-6">
      <FileBrowser />
    </div>
  );
}

/** A folder backed up to the cloud (upload-only). */
export function BackupCard({ job }: { job: BackupJob }) {
  const t = useT();
  const provider = useProvider();
  const account = useAccounts().data?.[0];
  const home = useHome().data;
  const [confirm, setConfirm] = useState(false);
  const toggle = useAction((enabled: boolean) => api.setBackupEnabled(job.id, enabled), [keys.backups]);
  const run = useAction(() => api.runBackup(job.id), [keys.backups], t("backups.toast.finished"));
  const remove = useAction(() => api.deleteBackup(job.id), [keys.backups], t("backups.toast.removed"));
  return (
    <Card>
      <CardHeader>
        <ShieldCheck className={cn("size-5", job.enabled ? "text-success" : "text-fg-subtle")} />
        <CardTitle>{job.name}</CardTitle>
        <span className="ml-auto">
          {!job.enabled ? (
            <Badge>{t("sync.state.paused")}</Badge>
          ) : job.lastError ? (
            <Badge tone="warning">{t("dashboard.attention")}</Badge>
          ) : job.lastSuccess ? (
            <Badge tone="success">{t("folders.backedUp")}</Badge>
          ) : (
            <Badge tone="accent">{t("backups.waiting")}</Badge>
          )}
        </span>
      </CardHeader>
      <CardContent className="space-y-3 text-sm">
        <div>
          <p className="truncate" title={job.localPath}>{tildePath(job.localPath, home)}</p>
          <p className="truncate text-fg-muted">{t("folders.backedUpTo", { place: `${provider} / ${displayPath(account, job.remotePath)}` })}</p>
        </div>
        {job.lastError && (
          <p className="text-xs text-fg-muted">
            <span className="font-medium text-warning">{localizeError(job.lastError).title}.</span> {localizeError(job.lastError).message}
          </p>
        )}
        <div className="flex items-center gap-2 pt-1">
          <Button size="sm" onClick={() => run.mutate(undefined)} disabled={run.isPending || !job.enabled}>
            <RefreshCw /> {t("backups.runNow")}
          </Button>
          <span className="text-fg-muted">{job.lastSuccess ? t("folders.lastBackup", { when: timeAgo(job.lastSuccess) }) : ""}</span>
          <span className="ml-auto">
            <Menu
              trigger={(p) => (
                <Button size="sm" variant="ghost" aria-label={t("folders.more", { name: job.name })} {...p}>
                  <MoreHorizontal />
                </Button>
              )}
              items={[
                { label: t("sync.openFolder"), icon: <FolderOpen />, onSelect: () => openFolder(job.localPath) },
                job.enabled
                  ? { label: t("folders.pause"), icon: <Pause />, onSelect: () => toggle.mutate(false) }
                  : { label: t("folders.resume"), icon: <Play />, onSelect: () => toggle.mutate(true) },
                { label: t("backups.remove"), icon: <Trash2 />, danger: true, onSelect: () => setConfirm(true) },
              ]}
            />
          </span>
        </div>
      </CardContent>
      <Dialog
        open={confirm}
        onClose={() => setConfirm(false)}
        title={t("folders.removeBackupTitle", { name: job.name })}
        footer={
          <>
            <Button onClick={() => setConfirm(false)}>{t("files.cancel")}</Button>
            <Button
              variant="danger"
              onClick={() => {
                remove.mutate(undefined);
                setConfirm(false);
              }}
            >
              {t("backups.remove")}
            </Button>
          </>
        }
      >
        {t("folders.removeBackupBody", { provider })}
      </Dialog>
    </Card>
  );
}

/** Starting a backup of a folder. Returns the action and the dialog to render. */
export function useBackupAdder() {
  const account = useAccounts().data?.[0];
  const [adding, setAdding] = useState<string | null>(null);
  const [remote, setRemote] = useState("");
  const t = useT();
  const provider = useProvider();
  const home = useHome().data;
  const create = useAction(api.createBackup, [keys.backups], t("backups.toast.created"));

  const choose = async () => {
    const [p] = await pickFolders(t("backups.choose"));
    if (!p) return;
    setAdding(p);
    setRemote(defaultRemote(account, `Backup/${p.split("/").filter(Boolean).pop() ?? "Folder"}`));
  };

  const dialog = (
    <Dialog
      open={!!adding}
      onClose={() => setAdding(null)}
      title={t("backups.dialogTitle")}
      footer={
        <>
          <Button onClick={() => setAdding(null)}>{t("files.cancel")}</Button>
          <Button
            variant="primary"
            disabled={!remote.trim()}
            onClick={() => {
              if (adding && account) create.mutate({ accountId: account.id, localPath: adding, remotePath: remote.trim() });
              setAdding(null);
            }}
          >
            {t("backups.start")}
          </Button>
        </>
      }
    >
      <p className="mb-1 text-fg-muted">{t("backups.localFolder")}</p>
      <p className="mb-4 font-medium" title={adding ?? ""}>{tildePath(adding ?? "", home)}</p>
      <label className="mb-1 block text-fg-muted" htmlFor="remote">{t("backups.saveAs", { provider })}</label>
      <input id="remote" value={remote} onChange={(e) => setRemote(e.target.value)} className="h-10 w-full rounded-lg border border-border bg-surface px-3 outline-none focus:border-accent" />
      <p className="mt-3 text-xs text-fg-muted">{t("backups.dialogHint")}</p>
    </Dialog>
  );
  return { choose, dialog };
}

function TransferRow({ t }: { t: Transfer }) {
  const cancel = useAction(api.cancelTransfer, [keys.transfers]);
  const tt = useT();
  const account = useAccounts().data?.[0];
  const home = useHome().data;
  const provider = useProvider();
  const active = t.state === "running" || t.state === "queued";
  const name = t.source.split("/").filter(Boolean).pop();
  // Uploads go to a cloud path ("Archive/…"), downloads to a local one.
  const to = t.direction === "upload" ? `${provider} / ${displayPath(account, t.destination)}` : tildePath(t.destination, home);
  return (
    <div className="flex items-center gap-4 border-b border-border px-5 py-3 last:border-0 text-sm">
      <span className={cn("grid size-9 place-items-center rounded-full", t.direction === "upload" ? "bg-success-soft text-success" : "bg-accent-soft text-accent")}>
        {t.direction === "upload" ? <ArrowUp className="size-5" /> : <ArrowDown className="size-5" />}
      </span>
      <div className="min-w-0 flex-1">
        <p className="truncate font-medium">{name}</p>
        <p className="truncate text-xs text-fg-muted">{tt("transfers.to", { path: to })}</p>
        {active && <Progress className="mt-2 h-1.5" value={percent(t.bytesDone, t.bytesTotal)} />}
      </div>
      <div className="w-44 text-right text-xs tabular-nums text-fg-muted">
        {active ? (
          <>
            <p>{tt("transfers.progress", { done: formatBytes(t.bytesDone), total: formatBytes(t.bytesTotal) })}</p>
            <p>{formatSpeed(t.speed)} {formatEta(t.etaSeconds) && `• ${formatEta(t.etaSeconds)}`}</p>
          </>
        ) : (
          <p>{t.state === "interrupted" ? tt("transfers.resuming") : formatBytes(t.bytesTotal ?? t.bytesDone)}</p>
        )}
      </div>
      <div className="w-28 text-right">
        {active && (
          <Button size="sm" variant="ghost" onClick={() => cancel.mutate(t.id)}>
            <X /> {tt("transfers.cancel")}
          </Button>
        )}
      </div>
    </div>
  );
}

/** What is happening now (transfers in progress), then everything that happened. */
export function ActivityView() {
  const transfers = useTransfers().data ?? [];
  const history = useHistory().data ?? [];
  const active = transfers.filter((t) => t.state === "running" || t.state === "queued" || t.state === "interrupted");
  const t = useT();
  return (
    <Page title={t("activity.title")} subtitle={t("activity.subtitle")}>
      {active.length > 0 && (
        <Card>
          <CardHeader className="border-b border-border"><CardTitle>{t("transfers.active", { count: active.length })}</CardTitle></CardHeader>
          {active.map((x) => <TransferRow key={x.id} t={x} />)}
        </Card>
      )}
      <Card>
        {history.length === 0 && <EmptyState title={t("history.emptyTitle")} message={t("history.emptyBody")} />}
        {history.map((h) => {
          const d = describeHistory(h);
          const size = h.event.type === "transferUpdated" && h.event.data.state === "completed" ? formatBytes(h.event.data.bytesTotal) : "";
          return (
            <div key={h.id} className="flex items-center gap-3 border-b border-border px-5 py-3 text-sm last:border-0">
              <ActivityIcon kind={d.icon} />
              <p className="min-w-0 flex-1 truncate"><span className="font-semibold">{d.title}</span> {d.detail}</p>
              {size && <span className="text-xs tabular-nums text-fg-muted">{size}</span>}
              <span className="w-36 text-right text-xs text-fg-muted">{formatDate(h.at)}</span>
            </div>
          );
        })}
      </Card>
    </Page>
  );
}

export function SettingsView() {
  const status = useStatus().data;
  const account = useAccounts().data?.[0];
  const startOnLogin = useSetting<boolean>("startOnLogin").data ?? true;
  const notifications = useSetting<boolean>("notifications").data ?? true;
  const theme = useSetting<string>("theme").data ?? "system";
  const t = useT();
  const provider = useProvider();
  const { locale, setLocale } = useLocale();
  const bandwidth = useSetting<number>("bandwidthLimit").data ?? 0;
  const set = useAction(({ key, value }: { key: string; value: unknown }) => api.setSetting(key, value), [["setting"]]);
  const setStartOnLogin = useAction(async (on: boolean) => {
    await setAutostart(on);
    await api.setSetting("startOnLogin", on);
  }, [["setting"]]);
  const disconnect = useAction(api.disconnect, [keys.accounts, keys.status], t("settings.toast.signedOut"));
  const reconnect = useAction(api.connect, [keys.accounts], t("settings.toast.signedIn"));
  const [confirm, setConfirm] = useState(false);

  const row = (label: string, hint: string, control: React.ReactNode) => (
    <div className="flex items-center gap-4 border-b border-border py-3.5 last:border-0">
      <div className="flex-1">
        <p className="font-medium">{label}</p>
        <p className="text-sm text-fg-muted">{hint}</p>
      </div>
      {control}
    </div>
  );

  return (
    <Page title={t("settings.title")}>
      <Card>
        <CardHeader><CardTitle>{t("settings.account")}</CardTitle></CardHeader>
        <CardContent>
          {account ? (
            row(
              account.displayName ?? provider,
              t("settings.connectedSince", { when: timeAgo(account.connectedAt) }),
              <div className="flex gap-2">
                <Button size="sm" onClick={() => reconnect.mutate(account.provider)}>{t("settings.signInAgain")}</Button>
                <Button size="sm" variant="ghost" className="text-danger" onClick={() => setConfirm(true)}>{t("settings.signOut")}</Button>
              </div>,
            )
          ) : (
            <p className="text-sm text-fg-muted">{t("settings.noAccount")}</p>
          )}
        </CardContent>
      </Card>
      <Card>
        <CardHeader><CardTitle>{t("settings.general")}</CardTitle></CardHeader>
        <CardContent>
          {row(t("settings.language"), t("settings.languageHint"),
            <select
              aria-label={t("settings.language")}
              value={locale}
              onChange={(e) => {
                if (!isLocale(e.target.value)) return;
                setLocale(e.target.value);
                set.mutate({ key: "language", value: e.target.value });
              }}
              className="h-9 rounded-lg border border-border bg-surface px-2 text-sm"
            >
              {LOCALES.map((l) => (
                <option key={l.id} value={l.id}>{l.name}</option>
              ))}
            </select>)}
          {row(t("settings.startOnLogin"), t("settings.startOnLoginHint"),
            <Switch label={t("settings.startOnLogin")} checked={startOnLogin} onChange={(v) => setStartOnLogin.mutate(v)} />)}
          {row(t("settings.notifications"), t("settings.notificationsHint"),
            <Switch label={t("settings.notifications")} checked={notifications} onChange={(v) => set.mutate({ key: "notifications", value: v })} />)}
          {row(t("settings.bandwidth"), t("settings.bandwidthHint"),
            <select
              aria-label={t("settings.bandwidth")}
              value={bandwidth}
              onChange={(e) => {
                const v = Number(e.target.value);
                set.mutate({ key: "bandwidthLimit", value: v > 0 ? v : null });
              }}
              className="h-9 rounded-lg border border-border bg-surface px-2 text-sm"
            >
              {BANDWIDTH_CHOICES.map((mb) => (
                <option key={mb} value={mb}>{mb === 0 ? t("settings.bandwidthNone") : t("settings.bandwidthMb", { mb })}</option>
              ))}
            </select>)}
          {row(t("settings.appearance"), t("settings.appearanceHint"),
            <select
              aria-label={t("settings.appearance")}
              value={theme}
              onChange={(e) => set.mutate({ key: "theme", value: e.target.value })}
              className="h-9 rounded-lg border border-border bg-surface px-2 text-sm"
            >
              <option value="system">{t("settings.theme.system")}</option>
              <option value="light">{t("settings.theme.light")}</option>
              <option value="dark">{t("settings.theme.dark")}</option>
            </select>)}
        </CardContent>
      </Card>
      <DiagnosticsCard />
      <Card>
        <CardHeader><CardTitle>{t("settings.about")}</CardTitle></CardHeader>
        <CardContent className="space-y-1 text-sm text-fg-muted">
          <p>{t("settings.aboutVersions", { version: status?.version ?? "", rclone: status?.rcloneVersion ?? t("settings.notRunning") })}</p>
          <p>{t("settings.notAffiliated")}</p>
        </CardContent>
      </Card>
      <Dialog
        open={confirm}
        onClose={() => setConfirm(false)}
        title={t("settings.signOutTitle", { provider })}
        footer={
          <>
            <Button onClick={() => setConfirm(false)}>{t("files.cancel")}</Button>
            <Button variant="danger" onClick={() => { if (account) disconnect.mutate(account.id); setConfirm(false); }}>
              <Pause /> {t("settings.signOut")}
            </Button>
          </>
        }
      >
        {t("settings.signOutBody")}
      </Dialog>
    </Page>
  );
}



/** MB/s; 0 = no limit. */
const BANDWIDTH_CHOICES = [0, 1, 2, 5, 10, 20, 50];

function DiagnosticsCard() {
  const t = useT();
  const [open, setOpen] = useState(false);
  const toast = useUi((s) => s.toast);
  const diag = useQuery({ queryKey: ["diagnostics"], queryFn: api.diagnostics, enabled: open });
  const copy = async () => {
    try {
      const d = diag.data ?? (await api.diagnostics());
      await copyText(d.report);
      toast({ kind: "success", title: t("settings.diagnostics.copied") });
    } catch {
      toast({ kind: "error", title: t("settings.diagnostics.copyFailed") });
    }
  };
  return (
    <Card id="diagnostics">
      <CardHeader><CardTitle>{t("settings.diagnostics.title")}</CardTitle></CardHeader>
      <CardContent className="space-y-3">
        <p className="text-sm text-fg-muted">{t("settings.diagnostics.hint")}</p>
        <div className="flex flex-wrap gap-2">
          <Button size="sm" onClick={copy}>{t("settings.diagnostics.copy")}</Button>
          <Button size="sm" variant="ghost" onClick={() => setOpen((o) => !o)} aria-expanded={open}>
            {open ? t("settings.diagnostics.hide") : t("settings.diagnostics.show")}
          </Button>
        </div>
        {open && (
          <>
            {diag.data && (
              <p className="text-sm">
                {diag.data.rcloneRunning ? t("settings.diagnostics.engineOk") : t("settings.diagnostics.engineDown")}
                {" • "}
                {t("settings.diagnostics.problems", { count: diag.data.recentProblems })}
              </p>
            )}
            <pre className="max-h-72 overflow-auto whitespace-pre-wrap break-words rounded-lg border border-border bg-bg p-3 font-mono text-xs">
              {diag.isLoading ? "…" : diag.data?.report}
            </pre>
            <Button size="sm" variant="ghost" onClick={() => diag.refetch()}>{t("settings.diagnostics.refresh")}</Button>
          </>
        )}
      </CardContent>
    </Card>
  );
}
