import type { ReactNode } from "react";
import { Badge } from "@/components/ui/badge";
import { Card } from "@/components/ui/card";
import { Checkbox } from "@/components/ui/checkbox";
import type { RuntimeState } from "@/lib/types";
import { cn } from "@/lib/utils";

export function SectionHeading({
  icon,
  title,
  aside,
  className,
}: {
  icon: ReactNode;
  title: string;
  aside?: ReactNode;
  className?: string;
}) {
  return (
    <div
      className={cn("mb-3 flex items-center justify-between gap-3", className)}
    >
      <div className="flex min-w-0 items-center gap-2 text-muted-foreground">
        <span className="grid size-7 shrink-0 place-items-center rounded-md border border-primary/20 bg-primary/10 text-primary">
          {icon}
        </span>
        <h2 className="truncate text-[11px] font-semibold uppercase tracking-[0.12em]">
          {title}
        </h2>
      </div>
      {aside}
    </div>
  );
}

export function ControlBlock({
  children,
  className,
}: {
  children: ReactNode;
  className?: string;
}) {
  return (
    <section
      className={cn(
        "rounded-lg border border-border bg-card p-3 shadow-sm shadow-black/[0.03] dark:shadow-black/20",
        className,
      )}
    >
      {children}
    </section>
  );
}

export function FieldStack({
  label,
  children,
  className,
}: {
  label: string;
  children: ReactNode;
  className?: string;
}) {
  return (
    <label
      className={cn(
        "grid gap-1.5 text-xs font-medium text-muted-foreground",
        className,
      )}
    >
      <span className="uppercase tracking-[0.1em]">{label}</span>
      {children}
    </label>
  );
}

export function CheckSetting({
  checked,
  onCheckedChange,
  label,
  disabled,
}: {
  checked: boolean;
  onCheckedChange: (checked: boolean) => void;
  label: string;
  disabled?: boolean;
}) {
  return (
    <label className="flex min-h-10 cursor-pointer items-center gap-2 rounded-md border border-border bg-muted/25 px-2.5 text-sm text-foreground transition-colors hover:bg-accent/55">
      <Checkbox
        checked={checked}
        disabled={disabled}
        onCheckedChange={(value) => onCheckedChange(value === true)}
      />
      <span>{label}</span>
    </label>
  );
}

export function InfoTile({
  label,
  value,
  icon,
  className,
  labelClassName,
  valueClassName,
}: {
  label: string;
  value: string;
  icon?: ReactNode;
  className?: string;
  labelClassName?: string;
  valueClassName?: string;
}) {
  return (
    <Card className={cn("min-w-0 rounded-md p-2.5 shadow-none", className)}>
      <div
        className={cn(
          "flex min-w-0 items-center gap-2 text-xs text-muted-foreground",
          labelClassName,
        )}
      >
        {icon}
        <span className="truncate">{label}</span>
      </div>
      <strong
        className={cn(
          "mt-1 block min-w-0 overflow-hidden text-ellipsis text-sm font-semibold text-foreground",
          valueClassName,
        )}
      >
        {value}
      </strong>
    </Card>
  );
}

export function StatusBadge({
  runtime,
  label,
}: {
  runtime: RuntimeState;
  label: string;
}) {
  const variant =
    runtime === "running"
      ? "success"
      : runtime === "error"
        ? "destructive"
        : "secondary";
  return (
    <Badge className="w-fit gap-1.5 rounded-full px-2.5 py-1" variant={variant}>
      <span
        className={cn(
          "mr-1.5 size-1.5 rounded-full",
          runtime === "running" && "bg-emerald-500",
          runtime === "error" && "bg-destructive-foreground",
          runtime !== "running" && runtime !== "error" && "bg-muted-foreground",
        )}
      />
      {label}
    </Badge>
  );
}

export function LinkBar({ label, value }: { label: string; value: number }) {
  const width = Math.min(100, Math.max(0, ((value - 1000) / 1000) * 100));
  return (
    <div className="space-y-1">
      <div className="flex items-center justify-between gap-3 text-xs">
        <span className="text-muted-foreground">{label}</span>
        <strong className="font-mono font-semibold text-foreground">
          {value > 0 ? value.toLocaleString() : "No signal"}
        </strong>
      </div>
      <div className="h-2 overflow-hidden rounded-full bg-secondary">
        <div
          className="h-full rounded-full bg-gradient-to-r from-destructive via-amber-400 to-emerald-400"
          style={{ width: `${width}%` }}
        />
      </div>
    </div>
  );
}
