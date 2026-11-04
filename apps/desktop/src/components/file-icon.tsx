import {
  FileArchive,
  FileAudio,
  FileImage,
  FileSpreadsheet,
  FileText,
  FileVideo,
  File as FileGeneric,
  Folder,
  FolderHeart,
  Laptop,
  Users,
} from "lucide-react";
import type { CloudEntry } from "@/lib/types";
import { cn } from "@/lib/utils";

const ext = (name: string) => name.toLowerCase().split(".").pop() ?? "";

export function FileIcon({ entry, className }: { entry: CloudEntry; className?: string }) {
  const base = cn("size-9 shrink-0 rounded-lg p-1.5", className);
  if (entry.kind === "folder") {
    const n = entry.name.toLowerCase();
    const Glyph = n.includes("family") ? Users : n === "devices" ? Laptop : n === "photos" ? FolderHeart : Folder;
    return (
      <span className={cn(base, "bg-accent-soft text-accent")} aria-hidden>
        <Glyph className="size-full" strokeWidth={1.8} />
      </span>
    );
  }
  const e = ext(entry.name);
  const m = entry.mimeType ?? "";
  let Glyph = FileGeneric;
  let tone = "bg-surface-muted text-fg-muted";
  if (m.startsWith("image/") || ["jpg", "jpeg", "png", "heic", "webp", "gif"].includes(e)) {
    Glyph = FileImage;
    tone = "bg-sky-100 text-sky-700 dark:bg-sky-950 dark:text-sky-300";
  } else if (e === "pdf") {
    Glyph = FileText;
    tone = "bg-red-100 text-red-600 dark:bg-red-950 dark:text-red-300";
  } else if (["xlsx", "xls", "ods", "csv"].includes(e)) {
    Glyph = FileSpreadsheet;
    tone = "bg-emerald-100 text-emerald-700 dark:bg-emerald-950 dark:text-emerald-300";
  } else if (["doc", "docx", "odt", "txt", "md"].includes(e)) {
    Glyph = FileText;
    tone = "bg-blue-100 text-blue-700 dark:bg-blue-950 dark:text-blue-300";
  } else if (m.startsWith("video/") || ["mp4", "mkv", "mov", "webm"].includes(e)) {
    Glyph = FileVideo;
    tone = "bg-violet-100 text-violet-700 dark:bg-violet-950 dark:text-violet-300";
  } else if (m.startsWith("audio/") || ["mp3", "flac", "ogg", "wav"].includes(e)) {
    Glyph = FileAudio;
  } else if (["zip", "tar", "gz", "7z", "xz", "rar"].includes(e)) {
    Glyph = FileArchive;
    tone = "bg-amber-100 text-amber-700 dark:bg-amber-950 dark:text-amber-300";
  }
  return (
    <span className={cn(base, tone)} aria-hidden>
      <Glyph className="size-full" strokeWidth={1.8} />
    </span>
  );
}
