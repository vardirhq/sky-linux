import { useMemo, useState, type DragEvent } from "react";
import {
  ArrowDown,
  ArrowUp,
  Check,
  ChevronLeft,
  ChevronRight,
  Cloud,
  CloudCheck,
  Download,
  FolderPlus,
  LayoutGrid,
  Lock,
  List,
  MoreVertical,
  Pencil,
  RefreshCw,
  ShieldCheck,
  Trash2,
  Upload,
} from "lucide-react";
import { useQueryClient } from "@tanstack/react-query";
import { api, defaultDownloadDir, pickFiles } from "@/lib/api";
import { keys, useAction, useBackups, useListing, useSearch, useSyncPairs, useTransfers } from "@/lib/hooks";
import type { BackupJob, CloudEntry, SyncPair, Transfer } from "@/lib/types";
import { cn, formatBytes, formatDate, joinPath, parentPath } from "@/lib/utils";
import { useUi } from "@/store/ui";
import { useT } from "@/i18n";
import { PROVIDER_NAMES } from "@/lib/types";
import { useAccounts } from "@/lib/hooks";
import { Badge } from "./ui/badge";
import { Button } from "./ui/button";
import { Card } from "./ui/card";
import { Dialog } from "./ui/dialog";
import { Menu } from "./ui/menu";
import { FileIcon } from "./file-icon";
import { areaLabel, canModify, canWriteIn, isAreaRoot, READ_ONLY_AREAS } from "@/lib/areas";

type SortKey = "name" | "size" | "modified";
type EntryStatus = "syncing" | "synced" | "protected" | "cloud";

const under = (path: string, root: string) => path === root || path.startsWith(root + "/");

export function entryStatus(
  e: CloudEntry,
  backups: BackupJob[],
  transfers: Transfer[],
  syncPairs: SyncPair[] = [],
): EntryStatus {
  const busy = transfers.some(
    (t) =>
      (t.state === "running" || t.state === "queued") &&
      (t.direction === "upload" ? under(t.destination, e.path) || under(e.path, t.destination) : under(t.source, e.path)),
  );
  const pairs = syncPairs.filter((p) => p.enabled && (under(e.path, p.remotePath) || under(p.remotePath, e.path)));
  if (busy || pairs.some((p) => p.state.kind === "syncing" || p.state.kind === "new")) return "syncing";
  if (pairs.some((p) => under(e.path, p.remotePath))) return "synced";
  if (backups.some((b) => b.enabled && (under(e.path, b.remotePath) || under(b.remotePath, e.path)))) return "protected";
  return "cloud";
}

export function StatusBadge({ status }: { status: EntryStatus }) {
  const t = useT();
  if (status === "syncing")
    return (
      <Badge tone="accent">
        <RefreshCw className="animate-spin [animation-duration:2.5s]" /> {t("files.syncing")}
      </Badge>
    );
  if (status === "protected")
    return (
      <Badge tone="success" title={t("files.protectedHint")}>
        <ShieldCheck /> {t("files.protected")}
      </Badge>
    );
  if (status === "synced")
    return (
      <Badge tone="success" title={t("files.syncedHint")}>
        <Check /> {t("files.synced")}
      </Badge>
    );
  return (
    <Badge tone="success" title={t("files.inCloudHint")}>
      <CloudCheck /> {t("files.inCloud")}
    </Badge>
  );
}

function sortEntries(list: CloudEntry[], key: SortKey, asc: boolean) {
  const dir = asc ? 1 : -1;
  return [...list].sort((a, b) => {
    if (a.kind !== b.kind) return a.kind === "folder" ? -1 : 1;
    if (key === "size") return ((a.size ?? -1) - (b.size ?? -1)) * dir;
    if (key === "modified") return ((a.modified ?? "") < (b.modified ?? "") ? -1 : 1) * dir;
    return a.name.localeCompare(b.name, undefined, { numeric: true, sensitivity: "base" }) * dir;
  });
}

export function FileBrowser({ compact = false, limit }: { compact?: boolean; limit?: number }) {
  const ui = useUi();
  const qc = useQueryClient();
  const t = useT();
  const account = useAccounts().data?.[0];
  const provider = account ? PROVIDER_NAMES[account.provider] : "Telia Sky";
  const searching = ui.search.trim().length >= 2;
  const listing = useListing(ui.cwd);
  const results = useSearch(ui.search);
  const backups = useBackups().data ?? [];
  const transfers = useTransfers().data ?? [];
  const syncPairs = useSyncPairs().data ?? [];
  const [sort, setSort] = useState<{ key: SortKey; asc: boolean }>({ key: "name", asc: true });
  const [renaming, setRenaming] = useState<CloudEntry | null>(null);
  const [newFolder, setNewFolder] = useState(false);
  const [confirmDelete, setConfirmDelete] = useState<CloudEntry | null>(null);
  const [dragOver, setDragOver] = useState(false);

  const query = searching ? results : listing;
  const entries = useMemo(() => {
    const s = sortEntries(query.data ?? [], sort.key, sort.asc);
    return limit ? s.slice(0, limit) : s;
  }, [query.data, sort, limit]);

  const invalidate = [keys.list(ui.cwd)] as const;
  const download = useAction(
    async (list: CloudEntry[]) => api.download(list, await defaultDownloadDir()),
    [keys.transfers],
    t("files.toast.downloadStarted"),
  );
  const upload = useAction((paths: string[]) => api.upload(paths, ui.cwd), [keys.transfers], t("files.toast.uploadStarted"));
  const rename = useAction(
    ({ e, name }: { e: CloudEntry; name: string }) => api.move(e.path, joinPath(parentPath(e.path), name), e.kind === "folder"),
    invalidate,
    t("files.toast.renamed"),
  );
  const mkdir = useAction((name: string) => api.mkdir(joinPath(ui.cwd, name)), invalidate, t("files.toast.folderCreated"));
  const del = useAction((e: CloudEntry) => api.deleteFile(e.path), invalidate, t("files.toast.trashed"));

  const selected = entries.filter((e) => ui.selection.has(e.path));
  const allSelected = entries.length > 0 && selected.length === entries.length;

  const open = (e: CloudEntry) => {
    if (e.kind === "folder") {
      ui.setSearch("");
      ui.navigate(e.path);
      if (compact) ui.setView("files");
    }
  };

  const onDrop = (ev: DragEvent) => {
    ev.preventDefault();
    setDragOver(false);
    // In Tauri, OS file drops arrive through the window drag-drop event
    // (see App.tsx); this handles in-browser drags of File objects.
    const paths = Array.from(ev.dataTransfer.files)
      .map((f) => (f as File & { path?: string }).path)
      .filter(Boolean) as string[];
    if (paths.length) upload.mutate(paths);
  };

  const crumbs = ui.cwd ? ui.cwd.split("/") : [];
  // Storage areas (Archive, Photos, …) show translated names; Photos and
  // Shared are read-only, so their write actions are hidden.
  const writable = canWriteIn(account, ui.cwd);
  const label = (e: CloudEntry) => (isAreaRoot(account, e.path) ? areaLabel(account, e.name) : e.name);
  const readOnlyArea = (e: CloudEntry) => isAreaRoot(account, e.path) && READ_ONLY_AREAS.includes(e.name);
  const header = (key: SortKey, label: string, cls?: string) => (
    <th className={cn("py-2.5 text-left font-medium", cls)}>
      <button
        className="inline-flex items-center gap-1 hover:text-fg"
        onClick={() => setSort((s) => ({ key, asc: s.key === key ? !s.asc : true }))}
      >
        {label}
        {sort.key === key && (sort.asc ? <ArrowUp className="size-3.5" /> : <ArrowDown className="size-3.5" />)}
      </button>
    </th>
  );

  return (
    <Card
      className={cn("flex min-h-0 flex-col transition-shadow", dragOver && "ring-2 ring-accent")}
      onDragOver={(e) => {
        e.preventDefault();
        setDragOver(true);
      }}
      onDragLeave={() => setDragOver(false)}
      onDrop={onDrop}
    >
      <div className="flex items-center gap-1 border-b border-border px-3 py-2.5">
        <Button variant="ghost" size="icon" aria-label={t("files.back")} disabled={!ui.backStack.length} onClick={ui.back}>
          <ChevronLeft />
        </Button>
        <Button variant="ghost" size="icon" aria-label={t("files.forward")} disabled={!ui.forwardStack.length} onClick={ui.forward}>
          <ChevronRight />
        </Button>
        <nav aria-label={t("files.breadcrumb")} className="ml-1 flex min-w-0 flex-1 items-center gap-1 text-[15px]">
          <button className="flex items-center gap-2 font-semibold hover:text-accent" onClick={() => ui.navigate("")}>
            <Cloud className="size-5" /> {t("files.myCloud")}
          </button>
          {searching ? (
            <span className="truncate text-fg-muted">/ {t("files.searchCrumb", { query: ui.search })}</span>
          ) : (
            crumbs.map((c, i) => (
              <span key={i} className="flex min-w-0 items-center gap-1">
                <ChevronRight className="size-4 text-fg-subtle" />
                <button
                  className={cn("truncate hover:text-accent", i === crumbs.length - 1 && "font-semibold")}
                  onClick={() => ui.navigate(crumbs.slice(0, i + 1).join("/"))}
                >
                  {i === 0 ? areaLabel(account, c) : c}
                </button>
              </span>
            ))
          )}
        </nav>
        {!writable && ui.cwd && !searching && (
          <Badge title={t("files.readOnlyHint")}>
            <Lock /> {t("files.readOnly")}
          </Badge>
        )}
        {selected.length > 0 && (
          <Button variant="secondary" size="sm" onClick={() => download.mutate(selected)}>
            <Download /> {t("files.downloadN", { count: selected.length })}
          </Button>
        )}
        <div className="ml-2 flex rounded-lg border border-border bg-surface-muted p-0.5">
          {(["list", "grid"] as const).map((m) => (
            <button
              key={m}
              aria-label={t(m === "list" ? "files.view.list" : "files.view.grid")}
              aria-pressed={ui.viewMode === m}
              onClick={() => ui.setViewMode(m)}
              className={cn(
                "rounded-md p-1.5 text-fg-muted",
                ui.viewMode === m && "bg-surface text-fg shadow-sm",
              )}
            >
              {m === "list" ? <List className="size-4" /> : <LayoutGrid className="size-4" />}
            </button>
          ))}
        </div>
        <Menu
          trigger={(p) => (
            <Button variant="ghost" size="icon" aria-label={t("files.moreActions")} {...p}>
              <MoreVertical />
            </Button>
          )}
          items={[
            { label: t("files.uploadFiles"), icon: <Upload />, disabled: !writable, onSelect: async () => upload.mutate(await pickFiles(t("files.uploadTo", { provider }))) },
            { label: t("files.newFolder"), icon: <FolderPlus />, disabled: !writable, onSelect: () => setNewFolder(true) },
            { label: t("files.refresh"), icon: <RefreshCw />, onSelect: () => qc.fetchQuery({ queryKey: keys.list(ui.cwd), queryFn: () => api.list(ui.cwd, true) }) },
          ]}
        />
      </div>

      <div className="min-h-0 flex-1 overflow-auto">
        {query.isLoading ? (
          <SkeletonRows />
        ) : query.isError ? (
          <EmptyState title={t("files.loadError")} message={(query.error as Error).message} />
        ) : entries.length === 0 ? (
          <EmptyState
            title={searching ? t("files.noMatches") : t("files.empty")}
            message={searching ? t("files.tryDifferent") : t("files.emptyHint")}
          />
        ) : ui.viewMode === "grid" ? (
          <div className="grid grid-cols-[repeat(auto-fill,minmax(140px,1fr))] gap-3 p-4">
            {entries.map((e) => (
              <button
                key={e.path}
                onDoubleClick={() => open(e)}
                onClick={() => ui.toggleSelected(e.path)}
                className={cn(
                  "flex flex-col items-center gap-2 rounded-xl border border-transparent p-3 text-center hover:bg-surface-muted",
                  ui.selection.has(e.path) && "border-accent bg-accent-soft",
                )}
              >
                <FileIcon entry={e} area={isAreaRoot(account, e.path) ? e.name : undefined} className="size-14 p-3" />
                <span className="line-clamp-2 text-sm">{label(e)}</span>
                <span className="text-xs text-fg-subtle">{formatBytes(e.size)}</span>
              </button>
            ))}
          </div>
        ) : (
          <table className="w-full text-sm">
            <thead className="sticky top-0 z-10 bg-surface text-fg-muted">
              <tr className="border-b border-border">
                <th className="w-12 pl-5">
                  <input
                    type="checkbox"
                    aria-label={t("files.selectAll")}
                    className="size-4 accent-[var(--color-accent)]"
                    checked={allSelected}
                    onChange={() => ui.setSelection(allSelected ? [] : entries.map((e) => e.path))}
                  />
                </th>
                {header("name", t("files.name"))}
                {header("size", t("files.size"), "w-28")}
                {header("modified", t("files.modified"), "w-44")}
                <th className="w-36 py-2.5 text-left font-medium">{t("files.status")}</th>
                <th className="w-10" />
              </tr>
            </thead>
            <tbody>
              {entries.map((e) => (
                <tr
                  key={e.path}
                  className={cn(
                    "group border-b border-border/70 last:border-0 hover:bg-surface-muted",
                    ui.selection.has(e.path) && "bg-accent-soft/60",
                  )}
                  onDoubleClick={() => open(e)}
                >
                  <td className="pl-5">
                    <input
                      type="checkbox"
                      aria-label={t("files.select", { name: e.name })}
                      className="size-4 accent-[var(--color-accent)]"
                      checked={ui.selection.has(e.path)}
                      onChange={() => ui.toggleSelected(e.path)}
                    />
                  </td>
                  <td className="py-1.5">
                    <button className="flex items-center gap-3 text-left" onClick={() => open(e)}>
                      <FileIcon entry={e} area={isAreaRoot(account, e.path) ? e.name : undefined} />
                      <span className="truncate">{label(e)}</span>
                    </button>
                  </td>
                  <td className="text-fg-muted tabular-nums">{formatBytes(e.size)}</td>
                  <td className="text-fg-muted tabular-nums">{formatDate(e.modified)}</td>
                  <td>
                    {readOnlyArea(e) ? (
                      <Badge title={t("files.readOnlyHint")}>
                        <Lock /> {t("files.readOnly")}
                      </Badge>
                    ) : (
                      <StatusBadge status={entryStatus(e, backups, transfers, syncPairs)} />
                    )}
                  </td>
                  <td className="pr-2">
                    <Menu
                      trigger={(p) => (
                        <Button variant="ghost" size="icon" aria-label={t("files.actionsFor", { name: e.name })} className="opacity-0 group-hover:opacity-100 aria-expanded:opacity-100" {...p}>
                          <MoreVertical />
                        </Button>
                      )}
                      items={[
                        { label: t("dashboard.download"), icon: <Download />, onSelect: () => download.mutate([e]) },
                        ...(canModify(account, e.path)
                          ? [{ label: t("files.rename"), icon: <Pencil />, onSelect: () => setRenaming(e) }]
                          : []),
                        ...(e.kind === "file" && canModify(account, e.path)
                          ? [{ label: t("files.delete"), icon: <Trash2 />, danger: true, onSelect: () => setConfirmDelete(e) }]
                          : []),
                      ]}
                    />
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        )}
      </div>

      <NameDialog
        open={!!renaming}
        title={t("files.renameTitle", { name: renaming?.name ?? "" })}
        initial={renaming?.name ?? ""}
        onClose={() => setRenaming(null)}
        onSubmit={(name) => renaming && rename.mutate({ e: renaming, name })}
      />
      <NameDialog
        open={newFolder}
        title={t("files.newFolder")}
        initial=""
        onClose={() => setNewFolder(false)}
        onSubmit={(name) => mkdir.mutate(name)}
      />
      <Dialog
        open={!!confirmDelete}
        onClose={() => setConfirmDelete(null)}
        title={t("files.deleteTitle")}
        footer={
          <>
            <Button onClick={() => setConfirmDelete(null)}>{t("files.cancel")}</Button>
            <Button
              variant="danger"
              onClick={() => {
                if (confirmDelete) del.mutate(confirmDelete);
                setConfirmDelete(null);
              }}
            >
              {t("files.delete")}
            </Button>
          </>
        }
      >
        {t("files.deleteBody", { name: confirmDelete?.name ?? "", provider })}
      </Dialog>
    </Card>
  );
}

function NameDialog({
  open,
  title,
  initial,
  onClose,
  onSubmit,
}: {
  open: boolean;
  title: string;
  initial: string;
  onClose: () => void;
  onSubmit: (name: string) => void;
}) {
  const [value, setValue] = useState(initial);
  const t = useT();
  const invalid = !value.trim() || /[/\\]/.test(value) || value === "." || value === "..";
  return (
    <Dialog
      open={open}
      onClose={onClose}
      title={title}
      footer={
        <>
          <Button onClick={onClose}>{t("files.cancel")}</Button>
          <Button
            variant="primary"
            disabled={invalid}
            onClick={() => {
              onSubmit(value.trim());
              onClose();
            }}
          >
            {t("files.save")}
          </Button>
        </>
      }
    >
      <input
        autoFocus
        key={initial + String(open)}
        defaultValue={initial}
        onChange={(e) => setValue(e.target.value)}
        onKeyDown={(e) => {
          if (e.key === "Enter" && !invalid) {
            onSubmit(value.trim());
            onClose();
          }
        }}
        className="h-10 w-full rounded-lg border border-border bg-surface px-3 outline-none focus:border-accent"
        aria-label={t("files.nameLabel")}
      />
      {invalid && value && <p className="mt-2 text-xs text-danger">{t("files.noSlashes")}</p>}
    </Dialog>
  );
}

function SkeletonRows() {
  return (
    <div className="space-y-2 p-5" aria-busy>
      {Array.from({ length: 6 }).map((_, i) => (
        <div key={i} className="h-9 animate-pulse rounded-lg bg-surface-muted" />
      ))}
    </div>
  );
}

export function EmptyState({ title, message }: { title: string; message: string }) {
  return (
    <div className="flex flex-col items-center justify-center gap-2 px-6 py-14 text-center">
      <Cloud className="size-10 text-fg-subtle" strokeWidth={1.5} />
      <p className="font-medium">{title}</p>
      <p className="max-w-sm text-sm text-fg-muted">{message}</p>
    </div>
  );
}
