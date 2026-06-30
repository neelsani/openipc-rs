"use client";

import { Activity, Network, ShieldCheck } from "lucide-react";
import type { StationApi } from "@/lib/use-station";
import { cn } from "@/lib/utils";
import { FieldRow, Panel, Toggle } from "./ui-bits";

export function VpnPanel({ api }: { api: StationApi }) {
  const { state, actions } = api;
  const enabled = state.settings.vpnEnabled;
  const status = state.vpnStatus;
  const nativeMode = state.usbMode === "native";
  const canToggle = nativeMode && !state.receiving;
  const myIp = status
    ? `${status.localIp}/${status.prefixLength}`
    : "10.5.0.3/24";
  const interfaceName = status?.interfaceName ?? "Created on start";
  const rxPort = status?.rxPort ?? 0x20;
  const txPort = status?.txPort ?? 0xa0;
  const statusTone = !nativeMode
    ? "text-muted-foreground"
    : status
      ? "text-primary"
      : enabled
      ? "text-primary"
      : "text-muted-foreground";

  return (
    <div className="space-y-3 p-3">
      <Panel
        title="VPN Tunnel"
        right={
          <span className={cn("font-mono text-[10px]", statusTone)}>
            {!nativeMode ? "unavailable" : enabled ? "enabled" : "disabled"}
          </span>
        }
      >
        <div className="divide-y divide-border px-3">
          <FieldRow
            label="OpenIPC VPN"
            hint="Bridges tunnel RX 0x20 and TX 0xa0"
          >
            <Toggle
              checked={enabled && nativeMode}
              disabled={!canToggle}
              onChange={(vpnEnabled) => actions.patchSettings({ vpnEnabled })}
              label="OpenIPC VPN"
            />
          </FieldRow>
          <FieldRow label="Interface" hint="Created when RX starts">
            <span className="font-mono text-[11px] text-foreground">
              {nativeMode ? interfaceName : "Unavailable"}
            </span>
          </FieldRow>
          <FieldRow label="My IP" hint="Local VPN address">
            <span className="font-mono text-[11px] text-foreground">
              {nativeMode ? myIp : "Browser mode"}
            </span>
          </FieldRow>
          <FieldRow label="Ports" hint="OpenIPC tunnel RX / TX">
            <span className="font-mono text-[11px] text-muted-foreground">
              0x{rxPort.toString(16).padStart(2, "0")} / 0x
              {txPort.toString(16).padStart(2, "0")}
            </span>
          </FieldRow>
        </div>
      </Panel>

      <div className="grid gap-2 sm:grid-cols-3">
        <div className="rounded-md border border-border bg-card/60 p-3">
          <ShieldCheck className="mb-2 h-4 w-4 text-primary" />
          <div className="text-xs font-medium text-foreground">Downlink</div>
          <div className="mt-1 text-[11px] leading-snug text-muted-foreground">
            Recovered tunnel payloads are written into the OS VPN interface.
          </div>
        </div>
        <div className="rounded-md border border-border bg-card/60 p-3">
          <Network className="mb-2 h-4 w-4 text-primary" />
          <div className="text-xs font-medium text-foreground">Uplink</div>
          <div className="mt-1 text-[11px] leading-snug text-muted-foreground">
            IP packets read from VPN are sent over WFB tunnel TX.
          </div>
        </div>
        <div className="rounded-md border border-border bg-card/60 p-3">
          <Activity className="mb-2 h-4 w-4 text-primary" />
          <div className="text-xs font-medium text-foreground">Lifecycle</div>
          <div className="mt-1 text-[11px] leading-snug text-muted-foreground">
            Changes apply on the next receiver start.
          </div>
        </div>
      </div>
    </div>
  );
}
