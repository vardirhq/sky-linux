import { cn } from "@/lib/utils";

export function Progress({ value, className, label }: { value: number; className?: string; label?: string }) {
  const v = Math.max(0, Math.min(100, value));
  return (
    <div
      role="progressbar"
      aria-valuemin={0}
      aria-valuemax={100}
      aria-valuenow={Math.round(v)}
      aria-label={label}
      className={cn("h-2 w-full overflow-hidden rounded-full bg-accent-soft", className)}
    >
      <div className="h-full rounded-full bg-accent transition-[width] duration-700 ease-out" style={{ width: `${v}%` }} />
    </div>
  );
}
