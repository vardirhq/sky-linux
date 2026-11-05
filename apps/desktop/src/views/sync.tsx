import { useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { ArrowLeftRight, FolderOpen, FolderPlus, LifeBuoy, RefreshCw, TriangleAlert, Unlink } from "lucide-react";
import { api, homeDir, openFolder, pickFolders } from "@/lib/api";
import { keys, useAccounts, useAction, useHome, useSyncPairs } from "@/lib/hooks";
import { PROVIDER_NAMES, type SyncPair } from "@/lib/types";
import { cn, formatBytes, tildePath, timeAgo } from "@/lib/utils";
import { defaultRemote, displayPath } from "@/lib/areas";
import { localizeError, useT } from "@/i18n";
import { EmptyState } from "@/components/file-browser";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Dialog } from "@/components/ui/dialog";
import { Switch } from "@/components/ui/switch";

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

function PairCard({ pair, provider }: { pair: SyncPair; provider: string }) {
  const t = useT();
  const [confirm, setConfirm] = useState(false);
  const [recovering, setRecovering] = useState(false);
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
        <SyncStateBadge pair={pair} />
        <div className="ml-auto">
          <Switch label={t("sync.pause", { name: pair.name })} checked={pair.enabled} onChange={(v) => toggle.mutate(v)} />
        </div>
      </CardHeader>
      <CardContent className="space-y-3 text-sm">
        <div className="grid grid-cols-[1fr_auto_1fr] items-center gap-3 rounded-lg bg-surface-muted px-3 py-2.5">
          <p className="truncate" title={pair.localPath}>{tildePath(pair.localPath, home)}</p>
          <ArrowLeftRight className="size-4 text-fg-subtle" />
          <p className="truncate text-right">{provider} / {displayPath(account, pair.remotePath)}</p>
        </div>
        <ReviewPanel pair={pair} />
        <div className="flex flex-wrap items-center gap-x-3 gap-y-1 text-fg-muted">
          <span>{pair.lastSync ? t("sync.lastSync", { when: timeAgo(pair.lastSync) }) : t("sync.never")}</span>
          {s && (
            <span className="tabular-nums">
              {t("sync.summary", { up: s.uploaded, down: s.downloaded, deleted: s.deletedLocal + s.deletedRemote })}
            </span>
          )}
          {s && s.conflicts > 0 && <Badge tone="warning">{t("sync.conflicts", { count: s.conflicts })}</Badge>}
        </div>
        {pair.state.kind === "error" && pair.lastError && (
          <p className="text-xs text-fg-muted">{localizeError(pair.lastError).message}</p>
        )}
        <div className="flex flex-wrap gap-2 pt-1">
          <Button size="sm" onClick={() => run.mutate(undefined)} disabled={!pair.enabled || pair.state.kind === "syncing"}>
            <RefreshCw /> {t("sync.syncNow")}
          </Button>
          <Button size="sm" variant="ghost" aria-label={t("sync.openFolder")} title={t("sync.openFolder")} onClick={() => openFolder(pair.localPath)}>
            <FolderOpen />
          </Button>
          <Button size="sm" variant="ghost" onClick={() => setRecovering(true)}>
            <LifeBuoy /> {t("sync.recovery")}
          </Button>
          <Button size="sm" variant="ghost" className="ml-auto text-danger" onClick={() => setConfirm(true)}>
            <Unlink /> {t("sync.remove")}
          </Button>
        </div>
      </CardContent>
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

export function SyncView() {
  const t = useT();
  const pairs = useSyncPairs().data ?? [];
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

  return (
    <div className="flex min-h-0 flex-1 flex-col gap-5 overflow-auto p-6">
      <div className="flex items-end gap-4">
        <div className="flex-1">
          <h1 className="text-2xl font-bold tracking-tight">{t("sync.title")}</h1>
          <p className="text-fg-muted">{t("sync.subtitle", { provider })}</p>
        </div>
        <Button variant="primary" onClick={pairs.length ? choose : startDefault} disabled={!account}>
          <FolderPlus /> {t("sync.add")}
        </Button>
      </div>

      {pairs.length === 0 ? (
        <Card>
          <EmptyState title={t("sync.emptyTitle")} message={t("sync.emptyBody")} />
          <div className="flex justify-center gap-2 pb-8">
            <Button variant="primary" onClick={startDefault} disabled={!account}>
              <FolderPlus /> {t("sync.createDefault", { name: DEFAULT_SYNC_FOLDER })}
            </Button>
            <Button onClick={choose} disabled={!account}>{t("sync.chooseLocal")}</Button>
          </div>
        </Card>
      ) : (
        <div className="grid grid-cols-[repeat(auto-fill,minmax(440px,1fr))] gap-4">
          {pairs.map((p) => (
            <PairCard key={p.id} pair={p} provider={provider} />
          ))}
        </div>
      )}

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
    </div>
  );
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
