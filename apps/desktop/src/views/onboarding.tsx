import { useEffect, useState } from "react";
import { ArrowLeftRight, Check, CheckCircle2, Cloud, FolderPlus, Loader2, ShieldCheck, Trash2 } from "lucide-react";
import { useQueryClient } from "@tanstack/react-query";
import { api, homeDir, openExternal, pickFolders, SkyError } from "@/lib/api";
import { DEFAULT_SYNC_FOLDER } from "./sync";
import { defaultRemote, displayPath } from "@/lib/areas";
import { Switch } from "@/components/ui/switch";
import { useAccounts } from "@/lib/hooks";
import type { Account } from "@/lib/types";
import { cn, tildePath } from "@/lib/utils";
import { useUi } from "@/store/ui";
import { Button } from "@/components/ui/button";
import { SkyLogo } from "@/components/layout";
import { localizeError, useT, type Key } from "@/i18n";

const STEPS: Key[] = ["onboarding.steps.welcome", "onboarding.steps.connect", "onboarding.steps.folders", "onboarding.steps.finish"];

/** First-run experience: welcome → connect → sync & backup → finish. No terminal needed. */
export function Onboarding({ onDone }: { onDone: () => void }) {
  const [step, setStep] = useState(0);
  const [account, setAccount] = useState<Account | null>(useAccounts().data?.[0] ?? null);
  const [folders, setFolders] = useState<string[]>([]);
  // Dropbox-style default: a "Telia Sky" folder in the home folder, synced.
  const [syncFolder, setSyncFolder] = useState(true);
  const [syncPath, setSyncPath] = useState("");
  useEffect(() => {
    homeDir().then((h) => setSyncPath(`${h}/${DEFAULT_SYNC_FOLDER}`));
  }, []);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const authUrl = useUi((s) => s.authUrl);
  const qc = useQueryClient();
  const t = useT();

  const connect = async () => {
    setBusy(true);
    setError(null);
    try {
      setAccount(await api.connect("telia_sky_no"));
      setStep(2);
    } catch (e) {
      setError(e instanceof SkyError ? localizeError(e.info).message : String(e));
    } finally {
      setBusy(false);
    }
  };

  const finish = async () => {
    setBusy(true);
    setError(null);
    try {
      if (syncFolder && syncPath) {
        await api.createSync({ accountId: account!.id, localPath: syncPath, remotePath: defaultRemote(account, DEFAULT_SYNC_FOLDER), createLocal: true });
      }
      for (const f of folders) {
        const name = f.split("/").filter(Boolean).pop() ?? "Folder";
        await api.createBackup({ accountId: account!.id, localPath: f, remotePath: defaultRemote(account, `Backup/${name}`) });
      }
      setStep(3);
    } catch (e) {
      setError(e instanceof SkyError ? localizeError(e.info).message : String(e));
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="flex h-full items-center justify-center bg-bg p-6 pt-0">
      <div className="w-full max-w-xl rounded-2xl border border-border bg-surface p-8 shadow-[var(--shadow-card)]">
        <ol className="mb-8 flex items-center gap-2" aria-label={t("onboarding.progress")}>
          {STEPS.map((s, i) => (
            <li key={s} className="flex flex-1 items-center gap-2">
              <span
                className={cn(
                  "grid size-7 shrink-0 place-items-center rounded-full text-xs font-semibold",
                  i < step ? "bg-success text-white" : i === step ? "bg-accent text-white" : "bg-surface-muted text-fg-muted",
                )}
                aria-current={i === step ? "step" : undefined}
              >
                {i < step ? <Check className="size-4" /> : i + 1}
              </span>
              <span className={cn("hidden text-xs sm:block", i === step ? "font-medium" : "text-fg-muted")}>{t(s)}</span>
              {i < STEPS.length - 1 && <span className="h-px flex-1 bg-border" />}
            </li>
          ))}
        </ol>

        {step === 0 && (
          <div className="space-y-5 text-center">
            <SkyLogo className="mx-auto size-20" />
            <h1 className="text-2xl font-bold">{t("onboarding.welcomeTitle")}</h1>
            <p className="text-fg-muted">
              {t("onboarding.welcomeBody")}
            </p>
            <Button variant="primary" size="lg" onClick={() => setStep(account ? 2 : 1)}>{t("onboarding.start")}</Button>
          </div>
        )}

        {step === 1 && (
          <div className="space-y-5 text-center">
            <Cloud className="mx-auto size-16 text-accent" strokeWidth={1.4} />
            <h1 className="text-2xl font-bold">{t("onboarding.connectTitle")}</h1>
            <p className="text-fg-muted">
              {t("onboarding.connectBody")}
            </p>
            <Button variant="primary" size="lg" onClick={connect} disabled={busy}>
              {busy ? <><Loader2 className="animate-spin" /> {t("onboarding.waiting")}</> : t("onboarding.signIn")}
            </Button>
            {busy && authUrl && (
              <p className="text-sm text-fg-muted">
                {t("onboarding.noBrowser")}{" "}
                <Button variant="link" onClick={() => openExternal(authUrl)}>{t("onboarding.openPage")}</Button>
              </p>
            )}
            {error && <p className="text-sm text-danger">{error}</p>}
          </div>
        )}

        {step === 2 && (
          <div className="space-y-5">
            <div className="text-center">
              <h1 className="text-2xl font-bold">{t("onboarding.foldersTitle")}</h1>
              <p className="mt-2 text-fg-muted">{t("onboarding.foldersBody")}</p>
            </div>
            <div className={cn("flex gap-3 rounded-xl border p-4", syncFolder ? "border-accent bg-accent-soft/50" : "border-border")}>
              <ArrowLeftRight className="mt-0.5 size-5 shrink-0 text-accent" />
              <div className="flex-1 text-sm">
                <p className="font-semibold">{t("onboarding.syncOption", { name: DEFAULT_SYNC_FOLDER })}</p>
                <p className="text-fg-muted">{t("onboarding.syncOptionBody", { path: tildePath(syncPath, syncPath.slice(0, syncPath.lastIndexOf("/"))), remote: displayPath(account, defaultRemote(account, DEFAULT_SYNC_FOLDER)) })}</p>
              </div>
              <Switch label={t("onboarding.syncOption", { name: DEFAULT_SYNC_FOLDER })} checked={syncFolder} onChange={setSyncFolder} />
            </div>
            <div className="flex gap-3 rounded-xl border border-border p-4">
              <ShieldCheck className="mt-0.5 size-5 shrink-0 text-success" />
              <div className="flex-1 text-sm">
                <p className="font-semibold">{t("onboarding.backupOption")}</p>
                <p className="text-fg-muted">{t("onboarding.backupOptionBody")}</p>
              </div>
            </div>
            <ul className="space-y-2">
              {folders.map((f) => (
                <li key={f} className="flex items-center gap-3 rounded-lg border border-border px-3 py-2 text-sm">
                  <FolderPlus className="size-4 text-accent" />
                  <span className="flex-1 truncate">{f}</span>
                  <button aria-label={t("onboarding.removeFolder", { path: f })} onClick={() => setFolders(folders.filter((x) => x !== f))}>
                    <Trash2 className="size-4 text-fg-muted" />
                  </button>
                </li>
              ))}
            </ul>
            <Button className="w-full" onClick={async () => setFolders([...new Set([...folders, ...(await pickFolders(t("onboarding.chooseFolders")))])])}>
              <FolderPlus /> {t("onboarding.addFolder")}
            </Button>
            {error && <p className="text-sm text-danger">{error}</p>}
            <div className="flex justify-between">
              <Button variant="ghost" onClick={() => setStep(3)}>{t("onboarding.skip")}</Button>
              <Button variant="primary" disabled={(!folders.length && !syncFolder) || busy} onClick={finish}>
                {busy && <Loader2 className="animate-spin" />} {t("onboarding.continue")}
              </Button>
            </div>
          </div>
        )}

        {step === 3 && (
          <div className="space-y-5 text-center">
            <CheckCircle2 className="mx-auto size-16 text-success" strokeWidth={1.4} />
            <h1 className="text-2xl font-bold">{t("onboarding.doneTitle")}</h1>
            <p className="text-fg-muted">
              {t("onboarding.doneBody")}
            </p>
            <Button
              variant="primary"
              size="lg"
              onClick={async () => {
                await api.setSetting("onboardingComplete", true);
                await qc.invalidateQueries();
                onDone();
              }}
            >
              {t("onboarding.open")}
            </Button>
          </div>
        )}
      </div>
    </div>
  );
}
