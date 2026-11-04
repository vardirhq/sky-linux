import { useState } from "react";
import {
  ArrowDown,
  ArrowUp,
  FolderPlus,
  Laptop,
  Pause,

  RefreshCw,
  ShieldCheck,
  Trash2,
  X,
} from "lucide-react";
import { api, pickFolders } from "@/lib/api";
import { keys, useAccounts, useAction, useBackups, useHistory, useSetting, useStatus, useTransfers } from "@/lib/hooks";
import type { Schedule, Transfer } from "@/lib/types";
import { cn, formatBytes, formatDate, formatEta, formatSpeed, percent, timeAgo } from "@/lib/utils";
import { FileBrowser, EmptyState } from "@/components/file-browser";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Dialog } from "@/components/ui/dialog";
import { Progress } from "@/components/ui/progress";
import { Switch } from "@/components/ui/switch";
import { ActivityIcon, describeHistory } from "./dashboard";
import { LOCALES, isLocale, localizeError, t as tr, useLocale, useT } from "@/i18n";
import { PROVIDER_NAMES } from "@/lib/types";
import { defaultRemote, displayPath } from "@/lib/areas";

function useProvider() {
  const a = useAccounts().data?.[0];
  return a ? PROVIDER_NAMES[a.provider] : "Telia Sky";
}

function Page({ title, subtitle, actions, children }: { title: string; subtitle?: string; actions?: React.ReactNode; children: React.ReactNode }) {
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

const scheduleLabel = (s: Schedule) =>
  s.kind === "onChange"
    ? tr("backups.onChange")
    : s.kind === "interval"
      ? tr("backups.every", { count: s.minutes })
      : tr("backups.manual");

export function BackupsView() {
  const backups = useBackups().data ?? [];
  const account = useAccounts().data?.[0];
  const [adding, setAdding] = useState<string | null>(null);
  const [remote, setRemote] = useState("");
  const t = useT();
  const provider = useProvider();
  const create = useAction(api.createBackup, [keys.backups], t("backups.toast.created"));
  const toggle = useAction(({ id, enabled }: { id: string; enabled: boolean }) => api.setBackupEnabled(id, enabled), [keys.backups]);
  const run = useAction(api.runBackup, [keys.backups], t("backups.toast.finished"));
  const remove = useAction(api.deleteBackup, [keys.backups], t("backups.toast.removed"));

  const choose = async () => {
    const [p] = await pickFolders(t("backups.choose"));
    if (!p) return;
    setAdding(p);
    setRemote(defaultRemote(account, `Backup/${p.split("/").filter(Boolean).pop() ?? "Folder"}`));
  };

  return (
    <Page
      title={t("backups.title")}
      subtitle={t("backups.subtitle", { provider })}
      actions={
        <Button variant="primary" onClick={choose} disabled={!account}>
          <FolderPlus /> {t("backups.add")}
        </Button>
      }
    >
      {backups.length === 0 ? (
        <Card>
          <EmptyState title={t("backups.emptyTitle")} message={t("backups.emptyBody")} />
        </Card>
      ) : (
        <div className="grid grid-cols-[repeat(auto-fill,minmax(340px,1fr))] gap-4">
          {backups.map((b) => (
            <Card key={b.id}>
              <CardHeader>
                <ShieldCheck className={cn("size-5", b.enabled ? "text-success" : "text-fg-subtle")} />
                <CardTitle>{b.name}</CardTitle>
                <div className="ml-auto">
                  <Switch label={t("backups.enable", { name: b.name })} checked={b.enabled} onChange={(enabled) => toggle.mutate({ id: b.id, enabled })} />
                </div>
              </CardHeader>
              <CardContent className="space-y-2 text-sm">
                <p className="truncate text-fg-muted" title={b.localPath}>{b.localPath}</p>
                <p className="truncate">→ {provider} / {displayPath(account, b.remotePath)}</p>
                <div className="flex flex-wrap gap-2 pt-1">
                  <Badge>{scheduleLabel(b.schedule)}</Badge>
                  {b.lastError ? (
                    <Badge tone="warning" title={localizeError(b.lastError).message}>{localizeError(b.lastError).title}</Badge>
                  ) : b.lastSuccess ? (
                    <Badge tone="success">{t("backups.lastBackup", { when: timeAgo(b.lastSuccess) })}</Badge>
                  ) : (
                    <Badge tone="accent">{t("backups.waiting")}</Badge>
                  )}
                </div>
                {b.lastError && <p className="text-xs text-fg-muted">{localizeError(b.lastError).message}</p>}
                <div className="flex gap-2 pt-2">
                  <Button size="sm" onClick={() => run.mutate(b.id)} disabled={run.isPending}>
                    <RefreshCw /> {t("backups.runNow")}
                  </Button>
                  <Button size="sm" variant="ghost" className="ml-auto text-danger" onClick={() => remove.mutate(b.id)}>
                    <Trash2 /> {t("backups.remove")}
                  </Button>
                </div>
              </CardContent>
            </Card>
          ))}
        </div>
      )}
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
        <p className="mb-4 font-medium">{adding}</p>
        <label className="mb-1 block text-fg-muted" htmlFor="remote">{t("backups.saveAs", { provider })}</label>
        <input id="remote" value={remote} onChange={(e) => setRemote(e.target.value)} className="h-10 w-full rounded-lg border border-border bg-surface px-3 outline-none focus:border-accent" />
        <p className="mt-3 text-xs text-fg-muted">{t("backups.dialogHint")}</p>
      </Dialog>
    </Page>
  );
}

function TransferRow({ t }: { t: Transfer }) {
  const cancel = useAction(api.cancelTransfer, [keys.transfers]);
  const tt = useT();
  const active = t.state === "running" || t.state === "queued";
  const name = t.source.split("/").filter(Boolean).pop();
  return (
    <div className="flex items-center gap-4 border-b border-border px-5 py-3 last:border-0 text-sm">
      <span className={cn("grid size-9 place-items-center rounded-full", t.direction === "upload" ? "bg-success-soft text-success" : "bg-accent-soft text-accent")}>
        {t.direction === "upload" ? <ArrowUp className="size-5" /> : <ArrowDown className="size-5" />}
      </span>
      <div className="min-w-0 flex-1">
        <p className="truncate font-medium">{name}</p>
        <p className="truncate text-xs text-fg-muted">
          {tt("transfers.to", { path: t.destination })}
        </p>
        {active && <Progress className="mt-2 h-1.5" value={percent(t.bytesDone, t.bytesTotal)} />}
      </div>
      <div className="w-44 text-right text-xs tabular-nums text-fg-muted">
        {active ? (
          <>
            <p>{tt("transfers.progress", { done: formatBytes(t.bytesDone), total: formatBytes(t.bytesTotal) })}</p>
            <p>{formatSpeed(t.speed)} {formatEta(t.etaSeconds) && `• ${formatEta(t.etaSeconds)}`}</p>
          </>
        ) : (
          <>
            <p>{formatBytes(t.bytesTotal ?? t.bytesDone)}</p>
            <p>{formatDate(t.finishedAt ?? t.startedAt)}</p>
          </>
        )}
      </div>
      <div className="w-28 text-right">
        {active ? (
          <Button size="sm" variant="ghost" onClick={() => cancel.mutate(t.id)}>
            <X /> {tt("transfers.cancel")}
          </Button>
        ) : t.state === "completed" ? (
          <Badge tone="success">{tt("transfers.completed")}</Badge>
        ) : t.state === "failed" ? (
          <Badge tone="danger" title={t.error ? localizeError(t.error).message : undefined}>{tt("transfers.failed")}</Badge>
        ) : (
          <Badge>{t.state === "interrupted" ? tt("transfers.resuming") : tt("transfers.cancelled")}</Badge>
        )}
      </div>
    </div>
  );
}

export function TransfersView() {
  const transfers = useTransfers().data ?? [];
  const active = transfers.filter((t) => t.state === "running" || t.state === "queued" || t.state === "interrupted");
  const done = transfers.filter((x) => !active.includes(x));
  const t = useT();
  return (
    <Page title={t("transfers.title")} subtitle={t("transfers.subtitle")}>
      <Card>
        <CardHeader className="border-b border-border"><CardTitle>{t("transfers.active", { count: active.length })}</CardTitle></CardHeader>
        {active.length ? active.map((x) => <TransferRow key={x.id} t={x} />) : <p className="px-5 py-6 text-sm text-fg-muted">{t("transfers.idle")}</p>}
      </Card>
      <Card>
        <CardHeader className="border-b border-border"><CardTitle>{t("transfers.recent")}</CardTitle></CardHeader>
        {done.length ? done.map((x) => <TransferRow key={x.id} t={x} />) : <p className="px-5 py-6 text-sm text-fg-muted">{t("transfers.none")}</p>}
      </Card>
    </Page>
  );
}

export function HistoryView() {
  const history = useHistory().data ?? [];
  const t = useT();
  return (
    <Page title={t("history.title")} subtitle={t("history.subtitle")}>
      <Card>
        {history.length === 0 && <EmptyState title={t("history.emptyTitle")} message={t("history.emptyBody")} />}
        {history.map((h) => {
          const d = describeHistory(h);
          return (
            <div key={h.id} className="flex items-center gap-3 border-b border-border px-5 py-3 text-sm last:border-0">
              <ActivityIcon kind={d.icon} />
              <p className="min-w-0 flex-1 truncate"><span className="font-semibold">{d.title}</span> {d.detail}</p>
              <span className="text-xs text-fg-muted">{formatDate(h.at)}</span>
            </div>
          );
        })}
      </Card>
    </Page>
  );
}

export function DevicesView() {
  const status = useStatus().data;
  const t = useT();
  const provider = useProvider();
  return (
    <Page title={t("devices.title")} subtitle={t("devices.subtitle")}>
      <Card>
        <CardContent className="flex items-center gap-4 pt-5">
          <Laptop className="size-10 text-accent" strokeWidth={1.5} />
          <div className="flex-1">
            <p className="font-semibold">{t("devices.thisComputer")}</p>
            <p className="text-sm text-fg-muted">{t("devices.summary", { count: status?.backupJobs ?? 0, version: status?.version ?? "" })}</p>
          </div>
          <Badge tone="success">{t("devices.online")}</Badge>
        </CardContent>
      </Card>
      <p className="text-sm text-fg-muted">{t("devices.future", { provider })}</p>
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
  const set = useAction(({ key, value }: { key: string; value: unknown }) => api.setSetting(key, value), [["setting"]]);
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
            <Switch label={t("settings.startOnLogin")} checked={startOnLogin} onChange={(v) => set.mutate({ key: "startOnLogin", value: v })} />)}
          {row(t("settings.notifications"), t("settings.notificationsHint"),
            <Switch label={t("settings.notifications")} checked={notifications} onChange={(v) => set.mutate({ key: "notifications", value: v })} />)}
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


