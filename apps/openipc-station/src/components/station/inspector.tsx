"use client";

import { useState, type ReactNode } from "react";
import {
  Activity,
  ScrollText,
  Shield,
  SlidersHorizontal,
  Stethoscope,
} from "lucide-react";
import { cn } from "@/lib/utils";
import type { StationApi } from "@/lib/use-station";
import { SettingsPanel } from "./settings-panel";
import { MetricsPanel } from "./metrics-panel";
import { DiagnosticsPanel } from "./diagnostics-panel";
import { LogsPanel } from "./logs-panel";
import { VpnPanel } from "./vpn-panel";

type Tab = "settings" | "vpn" | "metrics" | "diagnostics" | "logs";

const TABS: { id: Tab; label: string; icon: ReactNode }[] = [
  {
    id: "settings",
    label: "Settings",
    icon: <SlidersHorizontal className="h-3.5 w-3.5" />,
  },
  {
    id: "metrics",
    label: "Metrics",
    icon: <Activity className="h-3.5 w-3.5" />,
  },
  {
    id: "vpn",
    label: "VPN",
    icon: <Shield className="h-3.5 w-3.5" />,
  },
  {
    id: "diagnostics",
    label: "Diagnostics",
    icon: <Stethoscope className="h-3.5 w-3.5" />,
  },
  { id: "logs", label: "Logs", icon: <ScrollText className="h-3.5 w-3.5" /> },
];

export function Inspector({
  api,
  className,
}: {
  api: StationApi;
  className?: string;
}) {
  const [tab, setTab] = useState<Tab>("settings");
  return (
    <div
      className={cn(
        "flex flex-col rounded-lg border border-border bg-card lg:min-h-0 lg:overflow-hidden",
        className,
      )}
    >
      <div className="flex shrink-0 border-b border-border">
        {TABS.map((t) => (
          <button
            key={t.id}
            type="button"
            aria-label={t.label}
            onClick={() => setTab(t.id)}
            className={cn(
              "relative flex flex-1 items-center justify-center gap-1 py-2.5 text-[10px] font-medium transition-colors sm:gap-1.5 sm:text-xs",
              tab === t.id
                ? "text-foreground"
                : "text-muted-foreground hover:text-foreground",
            )}
          >
            {t.icon}
            <span>{t.label}</span>
            {tab === t.id && (
              <span className="absolute inset-x-3 -bottom-px h-0.5 rounded-full bg-primary" />
            )}
          </button>
        ))}
      </div>
      <div className="scroll-rail flex-1 lg:min-h-0 lg:overflow-y-auto">
        {tab === "settings" && <SettingsPanel api={api} />}
        {tab === "vpn" && <VpnPanel api={api} />}
        {tab === "metrics" && <MetricsPanel api={api} />}
        {tab === "diagnostics" && <DiagnosticsPanel api={api} />}
        {tab === "logs" && <LogsPanel api={api} />}
      </div>
    </div>
  );
}
