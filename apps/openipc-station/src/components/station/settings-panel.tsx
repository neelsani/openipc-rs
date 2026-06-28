"use client";

import { useRef, useState, type ReactNode } from "react";
import {
  ChevronDown,
  FileKey,
  KeyRound,
  Radio,
  SlidersHorizontal,
  Usb,
  Video,
  Waypoints,
} from "lucide-react";
import { AVIATEUR_CHANNELS } from "@/lib/settings";
import { cn } from "@/lib/utils";
import { authorizedDeviceId, authorizedDeviceLabel } from "@/lib/usb";
import type { StationApi } from "@/lib/use-station";
import { FieldRow, Segmented, Toggle } from "./ui-bits";

function channelMhzFromLabel(label: string): number {
  const match = label.match(/^(\d+)\s+MHz/);
  return match ? Number(match[1]) : 0;
}

function Group({
  icon,
  title,
  defaultOpen,
  children,
}: {
  icon: ReactNode;
  title: string;
  defaultOpen?: boolean;
  children: ReactNode;
}) {
  const [open, setOpen] = useState(!!defaultOpen);
  return (
    <div className="border-b border-border last:border-b-0">
      <button
        type="button"
        onClick={() => setOpen((o) => !o)}
        className="flex w-full items-center gap-2 px-3 py-2.5 text-left transition-colors hover:bg-secondary/40"
      >
        <span className="text-muted-foreground">{icon}</span>
        <span className="text-xs font-semibold text-foreground">{title}</span>
        <ChevronDown
          className={cn(
            "ml-auto h-4 w-4 text-muted-foreground transition-transform",
            open && "rotate-180",
          )}
        />
      </button>
      {open && <div className="px-3 pb-3 pt-0.5">{children}</div>}
    </div>
  );
}

export function SettingsPanel({ api }: { api: StationApi }) {
  const { state, actions } = api;
  const s = state.settings;
  const fileInputRef = useRef<HTMLInputElement>(null);

  return (
    <div className="pb-3">
      <Group
        icon={<Usb className="h-4 w-4" />}
        title="Receiver / Device"
        defaultOpen
      >
        <FieldRow label="USB mode" hint="Selected by the current runtime">
          <span className="rounded-md border border-border bg-secondary px-2 py-1 font-mono text-[11px] uppercase text-foreground">
            {state.usbMode}
          </span>
        </FieldRow>
        <FieldRow
          label="Device"
          hint={state.adapterName ?? "Authorized adapters"}
        >
          <div className="flex max-w-[220px] items-center gap-1.5">
            <select
              value={s.wifiDevice}
              disabled={state.receiving || state.adapterConnected}
              onChange={(event) =>
                actions.patchSettings({ wifiDevice: event.target.value })
              }
              className="min-w-0 rounded-md border border-border bg-input/40 px-2 py-1 font-mono text-xs text-foreground disabled:opacity-50"
            >
              <option value="">Auto</option>
              {state.authorizedDevices.map((device, index) => (
                <option
                  key={`${authorizedDeviceId(device)}-${index}`}
                  value={authorizedDeviceId(device)}
                >
                  {authorizedDeviceLabel(device)}
                </option>
              ))}
            </select>
            <button
              type="button"
              onClick={actions.refreshDevices}
              disabled={state.receiving}
              className="rounded-md border border-border px-2 py-1 text-xs text-muted-foreground hover:text-foreground disabled:opacity-40"
            >
              Refresh
            </button>
          </div>
        </FieldRow>
        <FieldRow
          label="Adapter"
          hint={
            state.adapterConnected
              ? "Open and initialized"
              : "Choose or authorize USB"
          }
        >
          <button
            type="button"
            onClick={actions.connect}
            disabled={state.adapterConnected || state.receiving}
            className="rounded-md border border-border bg-secondary px-2.5 py-1 text-xs font-medium text-foreground hover:bg-secondary/70 disabled:text-muted-foreground"
          >
            {state.adapterConnected ? "Connected" : "Connect"}
          </button>
        </FieldRow>
        <FieldRow label="Endpoint" hint="Bulk IN transfer status">
          <span className="font-mono text-[11px] text-muted-foreground">
            {state.adapterInitialized ? "open · 0x81" : "—"}
          </span>
        </FieldRow>
      </Group>

      <Group icon={<Radio className="h-4 w-4" />} title="RF" defaultOpen>
        <FieldRow label="Channel" hint="Practical OpenIPC labels">
          <select
            value={s.channelNum}
            disabled={state.receiving}
            onChange={(e) => {
              const channel = Number(e.target.value);
              const label =
                AVIATEUR_CHANNELS.find(
                  ([candidate]) => candidate === channel,
                )?.[1] ?? "";
              actions.patchSettings({
                channelNum: channel,
                channelMhz: channelMhzFromLabel(label),
              });
            }}
            className="rounded-md border border-border bg-input/40 px-2 py-1 font-mono text-xs text-foreground disabled:opacity-50"
          >
            {AVIATEUR_CHANNELS.map(([channel, label]) => (
              <option key={channel} value={channel}>
                {label}
              </option>
            ))}
          </select>
        </FieldRow>
        <FieldRow
          label="Channel width"
          hint={state.receiving ? "Restart required to apply" : undefined}
        >
          <Segmented
            value={s.channelWidth}
            onChange={(v) => actions.patchSettings({ channelWidth: v })}
            options={[
              { label: "20 MHz", value: 20 },
              { label: "40 MHz", value: 40 },
            ]}
          />
        </FieldRow>
        <FieldRow label="Channel offset">
          <input
            type="number"
            min={0}
            max={3}
            value={s.channelOffset}
            disabled={state.receiving}
            onChange={(event) =>
              actions.patchSettings({
                channelOffset: Number(event.target.value),
              })
            }
            className="w-16 rounded-md border border-border bg-input/40 px-2 py-1 text-right font-mono text-xs text-foreground disabled:opacity-50"
          />
        </FieldRow>
      </Group>

      <Group icon={<KeyRound className="h-4 w-4" />} title="Key">
        <FieldRow
          label="Receiver key"
          hint={state.keyLoaded ? state.keyName : "No usable key loaded"}
        >
          {state.keyLoaded ? (
            <button
              type="button"
              onClick={actions.clearKey}
              className="rounded-md border border-border px-2.5 py-1 text-xs text-muted-foreground hover:text-foreground"
            >
              Clear
            </button>
          ) : (
            <span className="rounded bg-destructive/15 px-2 py-0.5 text-[10px] font-medium text-destructive">
              MISSING
            </span>
          )}
        </FieldRow>
        <div className="flex gap-2 pt-1">
          <input
            ref={fileInputRef}
            className="sr-only"
            type="file"
            onChange={actions.loadKeyFile}
          />
          <button
            type="button"
            onClick={() => fileInputRef.current?.click()}
            className="inline-flex flex-1 items-center justify-center gap-1.5 rounded-md border border-border bg-secondary px-2.5 py-1.5 text-xs font-medium text-foreground hover:bg-secondary/70"
          >
            <FileKey className="h-3.5 w-3.5" /> Load key file
          </button>
          <button
            type="button"
            onClick={actions.loadDefaultKey}
            className="rounded-md border border-border px-2.5 py-1.5 text-xs text-muted-foreground hover:text-foreground"
          >
            Default
          </button>
        </div>
      </Group>

      <Group icon={<Waypoints className="h-4 w-4" />} title="Adaptive Link">
        <FieldRow label="Adaptive link" hint="Requires uplink-capable air unit">
          <Toggle
            checked={s.adaptiveLink}
            onChange={(v) => actions.patchSettings({ adaptiveLink: v })}
            label="Adaptive link"
          />
        </FieldRow>
        <FieldRow label="TX feedback" hint="Sending status">
          <span
            className={cn(
              "font-mono text-[11px]",
              s.adaptiveLink ? "text-primary" : "text-muted-foreground",
            )}
          >
            {s.adaptiveLink ? `active · ${state.v.adaptiveTxFrames}` : "idle"}
          </span>
        </FieldRow>
        <FieldRow label="TX power" hint="Requested level">
          <input
            type="range"
            min={1}
            max={40}
            value={s.txPower}
            disabled={!s.adaptiveLink}
            onChange={(e) =>
              actions.patchSettings({ txPower: Number(e.target.value) })
            }
            className="w-28 accent-[var(--color-primary)] disabled:opacity-40"
          />
        </FieldRow>
      </Group>

      <Group icon={<Video className="h-4 w-4" />} title="Video">
        <FieldRow label="Codec preference">
          <Segmented
            value={s.codec}
            size="xs"
            onChange={(v) => actions.patchSettings({ codec: v })}
            options={[
              { label: "Auto", value: "auto" },
              { label: "H.264", value: "h264" },
              { label: "H.265", value: "h265" },
            ]}
          />
        </FieldRow>
        <FieldRow label="Decoder" hint="WebCodecs backend">
          <span className="font-mono text-[11px] text-muted-foreground">
            {state.v.decoderName}
          </span>
        </FieldRow>
        <FieldRow label="Availability">
          <span
            className={cn(
              "font-mono text-[11px]",
              state.decoderAvailable ? "text-primary" : "text-destructive",
            )}
          >
            {state.decoderAvailable ? "available" : "unavailable"}
          </span>
        </FieldRow>
      </Group>

      <Group icon={<SlidersHorizontal className="h-4 w-4" />} title="Advanced">
        <FieldRow label="Channel ID">
          <span className="font-mono text-[11px] text-muted-foreground">
            {s.channelId}
          </span>
        </FieldRow>
        <FieldRow label="Min epoch">
          <input
            type="number"
            value={s.minEpoch}
            onChange={(e) =>
              actions.patchSettings({ minEpoch: Number(e.target.value) })
            }
            className="w-20 rounded-md border border-border bg-input/40 px-2 py-1 text-right font-mono text-xs text-foreground"
          />
        </FieldRow>
        <FieldRow label="USB transfer size">
          <Segmented
            value={s.usbTransferSize}
            size="xs"
            onChange={(v) => actions.patchSettings({ usbTransferSize: v })}
            options={[
              { label: "16K", value: 16 * 1024 },
              { label: "32K", value: 32 * 1024 },
              { label: "64K", value: 64 * 1024 },
            ]}
          />
        </FieldRow>
        <FieldRow label="Diagnostic verbosity">
          <Segmented
            value={s.verbosity}
            size="xs"
            onChange={(v) => actions.patchSettings({ verbosity: v })}
            options={[
              { label: "Low", value: "low" },
              { label: "Normal", value: "normal" },
              { label: "High", value: "high" },
            ]}
          />
        </FieldRow>
        <div className="flex gap-2 pt-1">
          <button
            type="button"
            onClick={actions.resetCounters}
            className="flex-1 rounded-md border border-border px-2.5 py-1.5 text-xs text-muted-foreground hover:text-foreground"
          >
            Reset counters
          </button>
          <button
            type="button"
            onClick={actions.resetDecoder}
            className="flex-1 rounded-md border border-border px-2.5 py-1.5 text-xs text-muted-foreground hover:text-foreground"
          >
            Decoder reset
          </button>
        </div>
      </Group>
    </div>
  );
}
