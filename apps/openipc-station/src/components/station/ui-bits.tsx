"use client";

import type React from "react";
import { cn } from "@/lib/utils";

/* Reusable atoms for the console */

export function Stat({
  label,
  value,
  unit,
  tone = "default",
  className,
}: {
  label: string;
  value: React.ReactNode;
  unit?: string;
  tone?: "default" | "good" | "warn" | "bad" | "muted";
  className?: string;
}) {
  const toneClass = {
    default: "text-foreground",
    good: "text-primary",
    warn: "text-warning",
    bad: "text-destructive",
    muted: "text-muted-foreground",
  }[tone];
  return (
    <div className={cn("flex flex-col gap-0.5", className)}>
      <span className="text-[10px] uppercase tracking-wider text-muted-foreground">
        {label}
      </span>
      <span className={cn("font-mono text-sm tabular leading-none", toneClass)}>
        {value}
        {unit && (
          <span className="ml-0.5 text-[10px] text-muted-foreground">
            {unit}
          </span>
        )}
      </span>
    </div>
  );
}

export function FieldRow({
  label,
  hint,
  children,
}: {
  label: string;
  hint?: string;
  children: React.ReactNode;
}) {
  return (
    <div className="flex items-center justify-between gap-3 py-2">
      <div className="min-w-0">
        <div className="text-xs font-medium text-foreground">{label}</div>
        {hint && (
          <div className="text-[10px] leading-tight text-muted-foreground">
            {hint}
          </div>
        )}
      </div>
      <div className="shrink-0">{children}</div>
    </div>
  );
}

export function Toggle({
  checked,
  onChange,
  label,
  disabled,
}: {
  checked: boolean;
  onChange: (v: boolean) => void;
  label?: string;
  disabled?: boolean;
}) {
  return (
    <button
      type="button"
      role="switch"
      aria-checked={checked}
      aria-label={label}
      disabled={disabled}
      onClick={() => onChange(!checked)}
      className={cn(
        "relative inline-flex h-5 w-9 items-center rounded-full border border-border transition-colors",
        checked ? "bg-primary/80" : "bg-muted",
        disabled && "cursor-not-allowed opacity-45",
      )}
    >
      <span
        className={cn(
          "inline-block h-3.5 w-3.5 transform rounded-full bg-background shadow transition-transform",
          checked ? "translate-x-4" : "translate-x-0.5",
        )}
      />
    </button>
  );
}

export function Segmented<T extends string | number>({
  value,
  options,
  onChange,
  size = "sm",
}: {
  value: T;
  options: { label: string; value: T }[];
  onChange: (v: T) => void;
  size?: "sm" | "xs";
}) {
  return (
    <div className="inline-flex rounded-md border border-border bg-muted/40 p-0.5">
      {options.map((o) => (
        <button
          key={String(o.value)}
          type="button"
          onClick={() => onChange(o.value)}
          className={cn(
            "rounded-[5px] font-medium transition-colors",
            size === "xs" ? "px-2 py-0.5 text-[11px]" : "px-2.5 py-1 text-xs",
            value === o.value
              ? "bg-secondary text-foreground shadow-sm"
              : "text-muted-foreground hover:text-foreground",
          )}
        >
          {o.label}
        </button>
      ))}
    </div>
  );
}

export function Panel({
  title,
  right,
  children,
  className,
}: {
  title?: string;
  right?: React.ReactNode;
  children: React.ReactNode;
  className?: string;
}) {
  return (
    <section
      className={cn("rounded-lg border border-border bg-card", className)}
    >
      {title && (
        <header className="flex items-center justify-between border-b border-border px-3 py-2">
          <h3 className="text-[11px] font-semibold uppercase tracking-wider text-muted-foreground">
            {title}
          </h3>
          {right}
        </header>
      )}
      {children}
    </section>
  );
}

export function StatusDot({
  tone,
}: {
  tone: "good" | "warn" | "bad" | "idle";
}) {
  const c = {
    good: "bg-primary shadow-[0_0_8px_var(--color-primary)]",
    warn: "bg-warning shadow-[0_0_8px_var(--color-warning)]",
    bad: "bg-destructive shadow-[0_0_8px_var(--color-destructive)]",
    idle: "bg-muted-foreground/50",
  }[tone];
  return <span className={cn("inline-block h-2 w-2 rounded-full", c)} />;
}
