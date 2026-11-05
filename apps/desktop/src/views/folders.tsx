import { ArrowLeftRight, ChevronDown, FolderPlus, ShieldCheck } from "lucide-react";
import { useAccounts, useBackups, useSyncPairs } from "@/lib/hooks";
import { PROVIDER_NAMES } from "@/lib/types";
import { useT } from "@/i18n";
import { Button } from "@/components/ui/button";
import { Card } from "@/components/ui/card";
import { Menu } from "@/components/ui/menu";
import { BackupCard, Page, useBackupAdder } from "./pages";
import { DEFAULT_SYNC_FOLDER, PairCard, useSyncAdder } from "./sync";

/** Every folder Sky Linux looks after: synced both ways, or backed up. */
export function FoldersView() {
  const t = useT();
  const account = useAccounts().data?.[0];
  const provider = account ? PROVIDER_NAMES[account.provider] : "Telia Sky";
  const pairs = useSyncPairs().data ?? [];
  const backups = useBackups().data ?? [];
  const sync = useSyncAdder();
  const backup = useBackupAdder();
  const empty = pairs.length === 0 && backups.length === 0;

  return (
    <Page
      title={t("folders.title")}
      subtitle={t("folders.subtitle", { provider })}
      actions={
        !empty && (
          <Menu
            trigger={(p) => (
              <Button variant="primary" disabled={!account} {...p}>
                <FolderPlus /> {t("folders.add")} <ChevronDown />
              </Button>
            )}
            items={[
              { label: t("folders.addSync"), icon: <ArrowLeftRight />, onSelect: pairs.length ? sync.choose : sync.startDefault },
              { label: t("folders.addBackup"), icon: <ShieldCheck />, onSelect: backup.choose },
            ]}
          />
        )
      }
    >
      {empty ? (
        <div className="grid gap-4 md:grid-cols-2">
          <Choice
            icon={<ArrowLeftRight className="size-6 text-accent" />}
            title={t("folders.syncTitle")}
            body={t("folders.syncBody", { provider })}
            action={t("sync.createDefault", { name: DEFAULT_SYNC_FOLDER })}
            onClick={sync.startDefault}
            disabled={!account}
          />
          <Choice
            icon={<ShieldCheck className="size-6 text-success" />}
            title={t("folders.backupTitle")}
            body={t("folders.backupBody", { provider })}
            action={t("folders.addBackup")}
            onClick={backup.choose}
            disabled={!account}
          />
        </div>
      ) : (
        <div className="grid grid-cols-[repeat(auto-fill,minmax(380px,1fr))] gap-4">
          {pairs.map((p) => (
            <PairCard key={p.id} pair={p} provider={provider} />
          ))}
          {backups.map((b) => (
            <BackupCard key={b.id} job={b} />
          ))}
        </div>
      )}
      {sync.dialog}
      {backup.dialog}
    </Page>
  );
}

function Choice(props: { icon: React.ReactNode; title: string; body: string; action: string; onClick: () => void; disabled?: boolean }) {
  return (
    <Card className="flex flex-col gap-3 p-6">
      {props.icon}
      <h2 className="text-lg font-semibold">{props.title}</h2>
      <p className="flex-1 text-sm text-fg-muted">{props.body}</p>
      <Button variant="primary" className="self-start" onClick={props.onClick} disabled={props.disabled}>
        <FolderPlus /> {props.action}
      </Button>
    </Card>
  );
}
