import { useEffect } from "react";
import { Loader2 } from "lucide-react";
import { useQueryClient } from "@tanstack/react-query";
import { t } from "@/i18n";
import { api, isTauri, onEvent } from "@/lib/api";
import { reconcileAutostart } from "@/lib/autostart";
import { NotificationPolicy, showNotification } from "@/lib/notify";
import { keys, useAccounts, useDaemonEvents, useSetting } from "@/lib/hooks";
import { useUi } from "@/store/ui";
import { Banners, Sidebar, StatusBar, Toasts, TopBar } from "@/components/layout";
import { WindowControls } from "@/components/window-controls";
import type { ReactNode } from "react";
import { isLocale, useLocale, useT } from "@/i18n";
import { Dashboard } from "@/views/dashboard";
import { Onboarding } from "@/views/onboarding";
import { ActivityView, FilesView, SettingsView } from "@/views/pages";
import { FoldersView } from "@/views/folders";

function useTheme() {
  const theme = useSetting<string>("theme").data;
  useEffect(() => {
    const root = document.documentElement;
    if (theme === "light" || theme === "dark") root.dataset.theme = theme;
    else delete root.dataset.theme;
  }, [theme]);
}

/** Apply the language saved in the daemon (shared by every window). */
function useLanguage() {
  const saved = useSetting<string>("language").data;
  const { locale, setLocale } = useLocale();
  useEffect(() => {
    if (isLocale(saved) && saved !== locale) setLocale(saved);
  }, [saved, locale, setLocale]);
  useEffect(() => {
    document.documentElement.lang = locale;
    if (isTauri())
      import("@tauri-apps/api/core").then(({ invoke }) => invoke("set_tray_language", { locale }).catch(() => {}));
  }, [locale]);
}

/** Desktop notifications while the window is in the background. */
function useNotifications() {
  const enabled = useSetting<boolean>("notifications");
  const on = enabled.isSuccess && enabled.data !== false;
  useEffect(() => {
    if (!on) return;
    const policy = new NotificationPolicy();
    let un: (() => void) | undefined;
    let alive = true;
    onEvent((e) => {
      const note = policy.decide(e);
      // In the foreground, banners and toasts already say it.
      if (note && !document.hasFocus()) showNotification(note).catch(() => {});
    }).then((u) => (alive ? (un = u) : u()));
    return () => {
      alive = false;
      un?.();
    };
  }, [on]);
}

/** Restore the autostart entry if it went missing (or remove a stray one). */
function useAutostart() {
  const saved = useSetting<boolean>("startOnLogin");
  useEffect(() => {
    if (saved.isSuccess) reconcileAutostart(saved.data).catch(() => {});
  }, [saved.isSuccess, saved.data]);
}

/** OS file drops onto the window upload into the current folder. */
function useNativeDrop() {
  const qc = useQueryClient();
  useEffect(() => {
    if (!isTauri()) return;
    let un: (() => void) | undefined;
    import("@tauri-apps/api/webview").then(({ getCurrentWebview }) =>
      getCurrentWebview()
        .onDragDropEvent(async (e) => {
          if (e.payload.type !== "drop" || !e.payload.paths.length) return;
          const ui = useUi.getState();
          try {
            await api.upload(e.payload.paths, ui.cwd);
            ui.toast({ kind: "success", title: t("files.toast.uploadingN", { count: e.payload.paths.length }) });
            qc.invalidateQueries({ queryKey: keys.transfers });
          } catch (err) {
            ui.toast({ kind: "error", title: t("files.toast.uploadFailed"), message: String((err as Error).message) });
          }
        })
        .then((u) => (un = u)),
    );
    return () => un?.();
  }, [qc]);
}

/** Header-less screens still need a drag handle and window controls. */
function Chrome({ children }: { children: ReactNode }) {
  return (
    <div className="flex h-full flex-col bg-bg">
      <div data-tauri-drag-region className="flex h-12 shrink-0 items-center justify-end px-3">
        <WindowControls />
      </div>
      <div className="min-h-0 flex-1">{children}</div>
    </div>
  );
}

export default function App() {
  useDaemonEvents();
  useTheme();
  useNativeDrop();
  useLanguage();
  useNotifications();
  useAutostart();
  const t = useT();
  const view = useUi((s) => s.view);
  const accounts = useAccounts();
  const onboarded = useSetting<boolean>("onboardingComplete");
  const qc = useQueryClient();

  if (accounts.isLoading || onboarded.isLoading) {
    return (
      <Chrome>
        <div className="grid h-full place-items-center text-fg-muted">
          <Loader2 className="size-6 animate-spin" />
        </div>
      </Chrome>
    );
  }
  if (accounts.isError) {
    return (
      <Chrome>
      <div className="grid h-full place-items-center p-8 text-center">
        <div className="max-w-md space-y-2">
          <p className="text-lg font-semibold">{t("service.downTitle")}</p>
          <p className="text-fg-muted">{t("service.downBody")}</p>
        </div>
      </div>
      </Chrome>
    );
  }
  if (!accounts.data?.length || !onboarded.data) {
    return (
      <Chrome>
        <Onboarding onDone={() => qc.invalidateQueries()} />
      </Chrome>
    );
  }

  const View = {
    dashboard: Dashboard,
    files: FilesView,
    folders: FoldersView,
    activity: ActivityView,
    settings: SettingsView,
  }[view];

  return (
    <div className="flex h-full flex-col">
      <TopBar />
      <Banners />
      <div className="flex min-h-0 flex-1">
        <Sidebar />
        <main className="flex min-w-0 flex-1 flex-col bg-bg">
          <View />
          <StatusBar />
        </main>
      </div>
      <Toasts />
    </div>
  );
}
