import { useEffect, useState } from "react";
import { Copy, Minus, Square, X } from "lucide-react";
import { isTauri } from "@/lib/api";
import { useT } from "@/i18n";

type Win = import("@tauri-apps/api/window").Window;

async function win(): Promise<Win | null> {
  if (!isTauri()) return null;
  const { getCurrentWindow } = await import("@tauri-apps/api/window");
  return getCurrentWindow();
}

/**
 * Client-side window controls (minimize / maximize / close), GNOME-style on
 * the right of the header. "Close" hides the window: the Rust side
 * intercepts CloseRequested and keeps Sky Linux in the tray, so backups
 * continue.
 */
export function WindowControls() {
  const [maximized, setMaximized] = useState(false);
  const t = useT();
  useEffect(() => {
    let un: (() => void) | undefined;
    win().then(async (w) => {
      if (!w) return;
      setMaximized(await w.isMaximized());
      un = await w.onResized(async () => setMaximized(await w.isMaximized()));
    });
    return () => un?.();
  }, []);
  const btn =
    "grid size-9 place-items-center rounded-full text-fg-muted hover:bg-surface-muted hover:text-fg active:bg-border [&_svg]:size-[18px]";
  return (
    <div className="ml-1 flex items-center gap-1.5" role="group" aria-label={t("window.controls")}>
      <button className={btn} aria-label={t("window.minimize")} onClick={async () => (await win())?.minimize()}>
        <Minus strokeWidth={2} />
      </button>
      <button
        className={btn}
        aria-label={maximized ? t("window.restore") : t("window.maximize")}
        onClick={async () => (await win())?.toggleMaximize()}
      >
        {maximized ? <Copy strokeWidth={2} className="-scale-x-100" /> : <Square strokeWidth={2} className="size-4!" />}
      </button>
      <button
        className={`${btn} hover:bg-danger hover:text-white`}
        aria-label={t("window.close")}
        title={t("window.closeHint")}
        onClick={async () => (await win())?.close()}
      >
        <X strokeWidth={2} />
      </button>
    </div>
  );
}
