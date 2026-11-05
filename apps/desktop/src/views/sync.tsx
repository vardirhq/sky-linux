import { useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { ArrowLeftRight, FolderCheck, FolderOpen, LifeBuoy, MoreHorizontal, Pause, Play, RefreshCw, TriangleAlert, Unlink } from "lucide-react";
import { api, homeDir, openFolder, pickFolders } from "@/lib/api";
import { keys, useAccounts, useAction, useHome } from "@/lib/hooks";
import { PROVIDER_NAMES, type SyncPair } from "@/lib/types";
import { cn, formatBytes, tildePath, timeAgo } from "@/lib/utils";
import { defaultRemote, displayPath } from "@/lib/areas";
import { localizeError, useT } from "@/i18n";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Dialog } from "@/components/ui/dialog";
import { Menu } from "@/components/ui/menu";

/** Default local folder name, as users know the service. */
export const DEFAULT_SYNC_FOLDER = "Telia Sky";

export function SyncStateBadge({ pair }: { pair: SyncPair }) {
  const t = useT();
  if (!pair.enabled) return <Badge>{t("sync.state.paused")}</Badge>;
  switch (pair.state.kind) {
    case "idle":
      return <Badge tone="success">{t("sync.state.idle")}</Badge>;
    case "syncing":
      return (
        <Badge tone="accent">
          <RefreshCw className="animate-spin [animation-duration:2.5s]" /> {t("sync.state.syncing")}
        </Badge>
      );
    case "new":
      return <Badge tone="accent">{t("sync.state.new")}</Badge>;
    case "needsReview":
      return <Badge tone="warning">{t("sync.state.needsReview")}</Badge>;
    case "error":
      return <Badge tone="warning">{t("sync.state.error")}</Badge>;
  }
}

function ReviewPanel({ pair }: { pair: SyncPair }) {
  const t = useT();
  const resolve = useAction(
    ({ action }: { action: "allow_deletions" | "restore_all" | "retry" }) => api.resolveSync(pair.id, action),
    [keys.sync],
    t("sync.toast.resolved"),
  );
  if (pair.state.kind !== "needsReview") return null;
  const reason = pair.state.reason;
  const home = useHome().data;
  const vars = { name: pair.name, path: tildePath(pair.localPath, home), percent: pair.maxDeletePercent };
  return (
    <div className="space-y-3 rounded-xl border border-warning/40 bg-warning-soft p-4">
      <div className="flex gap-3">
        <TriangleAlert className="mt-0.5 size-5 shrink-0 text-warning" />
        <div>
          <p className="font-semibold">{t(`sync.review.${reason}.title`)}</p>
          <p className="text-fg-muted">{t(`sync.review.${reason}.body`, vars)}</p>
        </div>
      </div>
      <div className="flex flex-wrap gap-2 pl-8">
        <Button size="sm" variant="primary" disabled={resolve.isPending} onClick={() => resolve.mutate({ action: "restore_all" })}>
          <LifeBuoy /> {t("sync.restoreAll")}
        </Button>
        {reason === "too_many_deletes" ? (
          <Button size="sm" variant="danger" disabled={resolve.isPending} onClick={() => resolve.mutate({ action: "allow_deletions" })}>
            {t("sync.allowDeletions")}
          </Button>
        ) : (
          <Button size="sm" disabled={resolve.isPending} onClick={() => resolve.mutate({ action: "retry" })}>
            {t("sync.retry")}
          </Button>
        )}
      </div>
    </div>
  );
}

export function PairCard({ pair, provider }: { pair: SyncPair; provider: string }) {
  const t = useT();
  const [confirm, setConfirm] = useState(false);
  const [recovering, setRecovering] = useState(false);
  const [choosing, setChoosing] = useState(false);
  const home = useHome().data;
  const account = useAccounts().data?.[0];
  const toggle = useAction((enabled: boolean) => api.setSyncEnabled(pair.id, enabled), [keys.sync]);
  const run = useAction(() => api.runSync(pair.id), [keys.sync]);
  const remove = useAction(() => api.deleteSync(pair.id), [keys.sync], t("sync.toast.removed"));
  const s = pair.lastSummary;
  return (
    <Card>
      <CardHeader>
        <ArrowLeftRight className={cn("size-5", pair.enabled ? "text-accent" : "text-fg-subtle")} />
        <CardTitle>{pair.name}</CardTitle>
        <span className="ml-auto">
          <SyncStateBadge pair={pair} />
        </span>
      </CardHeader>
      <CardContent className="space-y-3 text-sm">
        <div>
          <p className="truncate" title={pair.localPath}>{tildePath(pair.localPath, home)}</p>
          <p className="truncate text-fg-muted">{t("folders.syncedWith", { place: `${provider} / ${displayPath(account, pair.remotePath)}` })}</p>
        </div>
        <ReviewPanel pair={pair} />
        {pair.state.kind === "error" && pair.lastError && (
          <p className="text-xs text-fg-muted">{localizeError(pair.lastError).message}</p>
        )}
        {/* Only what needs attention gets a badge. */}
        {s && s.conflicts > 0 && <Badge tone="warning">{t("sync.conflicts", { count: s.conflicts })}</Badge>}
        <div className="flex items-center gap-2 pt-1">
          <Button size="sm" onClick={() => run.mutate(undefined)} disabled={!pair.enabled || pair.state.kind === "syncing"}>
            <RefreshCw /> {t("sync.syncNow")}
          </Button>
          <span className="text-fg-muted">{pair.lastSync ? t("sync.lastSync", { when: timeAgo(pair.lastSync) }) : t("sync.never")}</span>
          <span className="ml-auto">
            <Menu
              trigger={(p) => (
                <Button size="sm" variant="ghost" aria-label={t("folders.more", { name: pair.name })} {...p}>
                  <MoreHorizontal />
                </Button>
              )}
              items={[
                { label: t("sync.openFolder"), icon: <FolderOpen />, onSelect: () => openFolder(pair.localPath) },
                {
                  label: pair.excluded.length
                    ? `${t("sync.folders.button")} (${t("sync.folders.excludedN", { count: pair.excluded.length })})`
                    : t("sync.folders.button"),
                  icon: <FolderCheck />,
                  onSelect: () => setChoosing(true),
                },
                { label: t("sync.recovery"), icon: <LifeBuoy />, onSelect: () => setRecovering(true) },
                pair.enabled
                  ? { label: t("folders.pause"), icon: <Pause />, onSelect: () => toggle.mutate(false) }
                  : { label: t("folders.resume"), icon: <Play />, onSelect: () => toggle.mutate(true) },
                { label: t("sync.remove"), icon: <Unlink />, danger: true, onSelect: () => setConfirm(true) },
              ]}
            />
          </span>
        </div>
      </CardContent>
      {choosing && <FoldersDialog pair={pair} onClose={() => setChoosing(false)} />}
      {recovering && <RecoveryDialog pair={pair} onClose={() => setRecovering(false)} />}
      <Dialog
        open={confirm}
        onClose={() => setConfirm(false)}
        title={t("sync.removeTitle", { name: pair.name })}
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
              {t("sync.remove")}
            </Button>
          </>
        }
      >
        {t("sync.removeBody")}
      </Dialog>
    </Card>
  );
}

/** Starting a two-way sync: the "Telia Sky" folder, or a folder of choice.
 * Returns the actions and the dialog to render. */
export function useSyncAdder() {
  const t = useT();
  const account = useAccounts().data?.[0];
  const provider = account ? PROVIDER_NAMES[account.provider] : "Telia Sky";
  const [draft, setDraft] = useState<{ local: string; remote: string; create: boolean } | null>(null);
  const create = useAction(api.createSync, [keys.sync], t("sync.toast.created"));
  const home = useHome().data;

  const startDefault = async () => {
    const home = await homeDir();
    setDraft({ local: `${home}/${DEFAULT_SYNC_FOLDER}`, remote: defaultRemote(account, DEFAULT_SYNC_FOLDER), create: true });
  };
  const choose = async () => {
    const [p] = await pickFolders(t("sync.chooseLocal"));
    if (p) setDraft({ local: p, remote: defaultRemote(account, p.split("/").filter(Boolean).pop() ?? DEFAULT_SYNC_FOLDER), create: false });
  };

  const dialog = (
    <Dialog
      open={!!draft}
      onClose={() => setDraft(null)}
      title={t("sync.dialogTitle")}
      footer={
        <>
          <Button onClick={() => setDraft(null)}>{t("files.cancel")}</Button>
          <Button
            variant="primary"
            disabled={!draft?.remote.trim()}
            onClick={() => {
              if (draft && account)
                create.mutate({
                  accountId: account.id,
                  localPath: draft.local,
                  remotePath: draft.remote.trim(),
                  createLocal: draft.create,
                });
              setDraft(null);
            }}
          >
            {t("sync.start")}
          </Button>
        </>
      }
    >
      <p className="mb-1 text-fg-muted">{t("sync.localFolder")}</p>
      <div className="mb-4 flex items-center gap-2">
        <p className="flex-1 truncate font-medium" title={draft?.local}>{tildePath(draft?.local ?? "", home)}</p>
        <Button size="sm" variant="ghost" onClick={choose}>{t("sync.change")}</Button>
      </div>
      <label className="mb-1 block text-fg-muted" htmlFor="sync-remote">
        {t("sync.remoteFolder", { provider })}
      </label>
      <input
        id="sync-remote"
        value={draft?.remote ?? ""}
        onChange={(e) => draft && setDraft({ ...draft, remote: e.target.value })}
        className="h-10 w-full rounded-lg border border-border bg-surface px-3 outline-none focus:border-accent"
      />
      <p className="mt-3 text-xs text-fg-muted">{t("sync.dialogHint")}</p>
    </Dialog>
  );
  return { startDefault, choose, dialog };
}

/** Files sync removed from this computer, and a way to put them back. */
function RecoveryDialog({ pair, onClose }: { pair: SyncPair; onClose: () => void }) {
  const t = useT();
  const files = useQuery({ queryKey: ["recovery", pair.id], queryFn: () => api.recoveryFiles(pair.id) });
  const [selected, setSelected] = useState<Set<string>>(new Set());
  const restore = useAction(
    (paths: string[]) => api.restoreFiles(pair.id, paths),
    [["recovery", pair.id], keys.sync],
    t("sync.recover.restored", { count: selected.size }),
  );
  const list = files.data ?? [];
  const all = list.length > 0 && selected.size === list.length;
  const toggle = (path: string) =>
    setSelected((s) => {
      const next = new Set(s);
      if (next.has(path)) next.delete(path);
      else next.add(path);
      return next;
    });
  return (
    <Dialog
      open
      onClose={onClose}
      title={t("sync.recover.title", { name: pair.name })}
      footer={
        <>
          <Button variant="ghost" className="mr-auto" onClick={() => openFolder(pair.recoveryPath)}>
            <FolderOpen /> {t("sync.openFolder")}
          </Button>
          <Button onClick={onClose}>{t("sync.recover.close")}</Button>
          <Button
            variant="primary"
            disabled={selected.size === 0 || restore.isPending}
            onClick={() => restore.mutate([...selected], { onSuccess: () => setSelected(new Set()) })}
          >
            <LifeBuoy /> {selected.size ? t("sync.recover.restoreN", { count: selected.size }) : t("sync.recover.restore")}
          </Button>
        </>
      }
    >
      <p className="mb-3 text-fg-muted">{t("sync.recover.hint")}</p>
      {files.isLoading ? (
        <p className="text-fg-muted">…</p>
      ) : list.length === 0 ? (
        <p className="rounded-lg bg-surface-muted px-3 py-4 text-center text-fg-muted">{t("sync.recover.empty")}</p>
      ) : (
        <div className="max-h-72 overflow-auto rounded-lg border border-border">
          <label className="flex items-center gap-3 border-b border-border bg-surface-muted px-3 py-2 font-medium">
            <input
              type="checkbox"
              checked={all}
              onChange={() => setSelected(all ? new Set() : new Set(list.map((f) => f.path)))}
            />
            {t("sync.recover.selectAll", { count: list.length })}
          </label>
          {list.map((f) => (
            <label key={f.path} className="flex items-center gap-3 border-b border-border px-3 py-2 last:border-0">
              <input type="checkbox" checked={selected.has(f.path)} onChange={() => toggle(f.path)} />
              <span className="min-w-0 flex-1 truncate" title={f.path}>{f.path}</span>
              <span className="shrink-0 tabular-nums text-fg-muted">{formatBytes(f.size)}</span>
              {f.modified && <span className="hidden shrink-0 text-fg-muted sm:inline">{timeAgo(f.modified)}</span>}
            </label>
          ))}
        </div>
      )}
    </Dialog>
  );
}

/** Selective sync: which folders of the cloud folder are kept on this computer. */
function FoldersDialog({ pair, onClose }: { pair: SyncPair; onClose: () => void }) {
  const t = useT();
  const listing = useQuery({ queryKey: keys.list(pair.remotePath), queryFn: () => api.list(pair.remotePath) });
  const [excluded, setExcluded] = useState<Set<string>>(new Set(pair.excluded));
  const save = useAction((folders: string[]) => api.setSyncExcluded(pair.id, folders), [keys.sync], t("sync.folders.saved"));
  // Top-level folders in the cloud, plus excluded ones that no longer exist.
  const prefix = pair.remotePath.replace(/\/+$/, "") + "/";
  const names = [
    ...new Set([
      ...(listing.data ?? []).filter((e) => e.kind === "folder").map((e) => e.path.replace(prefix, "")),
      ...pair.excluded.filter((f) => !f.includes("/")),
    ]),
  ].sort((a, b) => a.localeCompare(b));
  const nested = pair.excluded.filter((f) => f.includes("/"));
  const changed = [...excluded].sort().join("\n") !== [...pair.excluded].sort().join("\n");
  const toggle = (name: string) =>
    setExcluded((s) => {
      const next = new Set(s);
      if (next.has(name)) next.delete(name);
      else next.add(name);
      return next;
    });
  return (
    <Dialog
      open
      onClose={onClose}
      title={t("sync.folders.title", { name: pair.name })}
      footer={
        <>
          <Button onClick={onClose}>{t("files.cancel")}</Button>
          <Button
            variant="primary"
            disabled={!changed || save.isPending}
            onClick={() => save.mutate([...excluded, ...nested.filter((n) => !excluded.has(n))], { onSuccess: onClose })}
          >
            {t("sync.folders.save")}
          </Button>
        </>
      }
    >
      <p className="mb-3 text-fg-muted">{t("sync.folders.hint")}</p>
      {listing.isLoading ? (
        <p className="text-fg-muted">…</p>
      ) : names.length === 0 ? (
        <p className="rounded-lg bg-surface-muted px-3 py-4 text-center text-fg-muted">{t("sync.folders.none")}</p>
      ) : (
        <div className="max-h-72 overflow-auto rounded-lg border border-border">
          {names.map((n) => (
            <label key={n} className="flex items-center gap-3 border-b border-border px-3 py-2 last:border-0">
              <input type="checkbox" checked={!excluded.has(n)} onChange={() => toggle(n)} />
              <span className="min-w-0 flex-1 truncate">{n}</span>
              {excluded.has(n) && <span className="shrink-0 text-xs text-fg-muted">{t("sync.folders.cloudOnly")}</span>}
            </label>
          ))}
        </div>
      )}
      <p className="mt-3 text-xs text-fg-muted">{t("sync.folders.note")}</p>
    </Dialog>
  );
}
