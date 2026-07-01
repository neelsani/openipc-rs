"use client";

import { useRef, useState, type ReactNode } from "react";
import {
  ChevronDown,
  FileKey,
  KeyRound,
  Plus,
  Radio,
  Route,
  SlidersHorizontal,
  Trash2,
  Usb,
  Video,
  Volume2,
  VolumeX,
  Waypoints,
} from "lucide-react";
import { Slider } from "@/components/ui/slider";
import {
  AVIATEUR_CHANNELS,
  CHANNEL_ID_PRESETS,
  DEFAULT_LINK_ID,
  RTP_PAYLOAD_TYPE_OPUS,
  channelIdForLinkPort,
  channelIdForRadioPort,
  formatChannelIdHex,
  formatLinkIdHex,
  formatRadioPortHex,
  linkIdFromChannelId,
  parseRadioPort,
  radioPortFromChannelId,
} from "@/lib/settings";
import type { PayloadRouteConfig } from "@/lib/types";
import { cn } from "@/lib/utils";
import { authorizedDeviceId, authorizedDeviceLabel } from "@/lib/usb";
import type { StationApi } from "@/lib/use-station";
import { FieldRow, Segmented, Toggle } from "./ui-bits";

function channelMhzFromLabel(label: string): number {
  const match = label.match(/^(\d+)\s+MHz/);
  return match ? Number(match[1]) : 0;
}

function nextRouteId(routes: PayloadRouteConfig[]): number {
  return Math.max(4, ...routes.map((route) => route.id)) + 1;
}

function RadioPortSelector({
  channelId,
  linkChannelId,
  disabled,
  onChange,
}: {
  channelId: string;
  linkChannelId: string | number;
  disabled?: boolean;
  onChange: (value: string) => void;
}) {
  const linkId =
    linkIdFromChannelId(linkChannelId) ??
    linkIdFromChannelId(channelId) ??
    DEFAULT_LINK_ID;
  const port = radioPortFromChannelId(channelId);
  const preset =
    port === null
      ? null
      : (CHANNEL_ID_PRESETS.find((candidate) => candidate.port === port) ??
        null);
  const [customOpen, setCustomOpen] = useState(!preset);
  const showCustom = customOpen || !preset;
  const selectedValue = showCustom ? "__custom" : String(preset?.port);
  const [customPort, setCustomPort] = useState(
    port === null ? "" : formatRadioPortHex(port),
  );

  function applyPort(nextPort: number) {
    onChange(channelIdForLinkPort(linkId, nextPort));
  }

  function applyCustomPort(text: string) {
    setCustomPort(text);
    const nextPort = parseRadioPort(text);
    if (nextPort !== null) {
      applyPort(nextPort);
    }
  }

  return (
    <div className="space-y-1">
      <select
        value={selectedValue}
        disabled={disabled}
        onChange={(event) => {
          if (event.target.value === "__custom") {
            setCustomOpen(true);
            return;
          }
          setCustomOpen(false);
          applyPort(Number(event.target.value));
        }}
        className="w-full rounded-md border border-border bg-input/40 px-2 py-1 text-xs text-foreground disabled:opacity-50"
      >
        {CHANNEL_ID_PRESETS.map((candidate) => (
          <option key={candidate.port} value={candidate.port}>
            {candidate.name} ({formatRadioPortHex(candidate.port)})
          </option>
        ))}
        <option value="__custom">
          Custom ({port === null ? "invalid" : formatRadioPortHex(port)})
        </option>
      </select>
      <div className="flex items-center justify-between gap-2 text-[10px] text-muted-foreground">
        <span className="truncate">
          {preset?.hint ?? "Custom WFB radio port"}
        </span>
        <span className="font-mono">link {formatLinkIdHex(linkId)}</span>
      </div>
      {showCustom && (
        <input
          value={customPort}
          disabled={disabled}
          onChange={(event) => applyCustomPort(event.target.value)}
          placeholder="0x20"
          className="w-full rounded-md border border-border bg-input/40 px-2 py-1 font-mono text-[11px] text-foreground disabled:opacity-50"
        />
      )}
    </div>
  );
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
  const udpAvailable = state.usbMode === "native";

  function patchRoute(id: number, patch: Partial<PayloadRouteConfig>) {
    actions.patchSettings({
      payloadRoutes: s.payloadRoutes.map((route) =>
        route.id === id ? { ...route, ...patch } : route,
      ),
    });
  }

  function addRoute() {
    const id = nextRouteId(s.payloadRoutes);
    actions.patchSettings({
      payloadRoutes: [
        ...s.payloadRoutes,
        {
          id,
          enabled: true,
          name: `Route ${id}`,
          channelId: channelIdForRadioPort(0x20),
          action: "inspect",
          udpHost: "127.0.0.1",
          udpPort: 5600,
        },
      ],
    });
  }

  function removeRoute(id: number) {
    actions.patchSettings({
      payloadRoutes: s.payloadRoutes.filter((route) => route.id !== id),
    });
  }

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

      <Group icon={<Route className="h-4 w-4" />} title="Routes">
        <div className="space-y-2">
          {s.payloadRoutes.map((route) => {
            const stats = state.routeStats.find(
              (candidate) => candidate.routeId === route.id,
            );
            const udpUnavailable = route.action === "udp" && !udpAvailable;
            return (
              <div
                key={route.id}
                className="rounded-md border border-border bg-card/60 p-2"
              >
                <div className="flex items-center gap-2">
                  <Toggle
                    checked={route.enabled && !udpUnavailable}
                    disabled={state.receiving || udpUnavailable}
                    onChange={(enabled) => patchRoute(route.id, { enabled })}
                    label={`${route.name} enabled`}
                  />
                  <input
                    value={route.name}
                    onChange={(event) =>
                      patchRoute(route.id, { name: event.target.value })
                    }
                    className="min-w-0 flex-1 rounded-md border border-border bg-input/40 px-2 py-1 text-xs text-foreground"
                  />
                  <button
                    type="button"
                    onClick={() => removeRoute(route.id)}
                    className="rounded-md border border-border p-1 text-muted-foreground hover:text-destructive"
                    aria-label={`Remove ${route.name}`}
                  >
                    <Trash2 className="h-3.5 w-3.5" />
                  </button>
                </div>

                <div className="mt-2 grid grid-cols-2 gap-2">
                  <label className="space-y-1">
                    <span className="text-[10px] text-muted-foreground">
                      Radio port
                    </span>
                    <RadioPortSelector
                      channelId={route.channelId}
                      linkChannelId={s.channelId}
                      disabled={state.receiving}
                      onChange={(channelId) =>
                        patchRoute(route.id, { channelId })
                      }
                    />
                  </label>
                  <label className="space-y-1">
                    <span className="text-[10px] text-muted-foreground">
                      Action
                    </span>
                    <select
                      value={route.action}
                      disabled={state.receiving}
                      onChange={(event) => {
                        const action = event.target
                          .value as PayloadRouteConfig["action"];
                        patchRoute(route.id, {
                          action,
                          audioCodec:
                            action === "audio"
                              ? (route.audioCodec ?? "auto")
                              : route.audioCodec,
                          payloadType:
                            action === "audio"
                              ? (route.payloadType ?? RTP_PAYLOAD_TYPE_OPUS)
                              : route.payloadType,
                        });
                      }}
                      className="w-full rounded-md border border-border bg-input/40 px-2 py-1 text-xs text-foreground disabled:opacity-50"
                    >
                      <option value="inspect">Inspect</option>
                      <option value="log">Log</option>
                      <option value="audio">Audio</option>
                      <option value="udp" disabled={!udpAvailable}>
                        UDP forward{udpAvailable ? "" : " (native only)"}
                      </option>
                    </select>
                  </label>
                </div>

                {route.action === "audio" && (
                  <div className="mt-2 space-y-2">
                    <div className="grid grid-cols-2 gap-2 sm:grid-cols-4">
                      <label className="space-y-1">
                        <span className="text-[10px] text-muted-foreground">
                          Codec
                        </span>
                        <select
                          value={route.audioCodec ?? "auto"}
                          onChange={(event) =>
                            patchRoute(route.id, {
                              audioCodec: event.target
                                .value as PayloadRouteConfig["audioCodec"],
                            })
                          }
                          className="w-full rounded-md border border-border bg-input/40 px-2 py-1 text-xs text-foreground"
                        >
                          <option value="auto">Auto</option>
                          <option value="opus">Opus</option>
                        </select>
                      </label>
                      <label className="space-y-1">
                        <span className="text-[10px] text-muted-foreground">
                          RTP PT
                        </span>
                        <input
                          type="number"
                          min={0}
                          max={127}
                          value={route.payloadType ?? RTP_PAYLOAD_TYPE_OPUS}
                          onChange={(event) =>
                            patchRoute(route.id, {
                              payloadType: Number(event.target.value),
                            })
                          }
                          className="w-full rounded-md border border-border bg-input/40 px-2 py-1 text-right font-mono text-xs text-foreground"
                        />
                      </label>
                      <label className="space-y-1">
                        <span className="text-[10px] text-muted-foreground">
                          Hz
                        </span>
                        <input
                          type="number"
                          min={8000}
                          max={96000}
                          value={route.sampleRate ?? 48000}
                          onChange={(event) =>
                            patchRoute(route.id, {
                              sampleRate: Number(event.target.value),
                            })
                          }
                          className="w-full rounded-md border border-border bg-input/40 px-2 py-1 text-right font-mono text-xs text-foreground"
                        />
                      </label>
                      <label className="space-y-1">
                        <span className="text-[10px] text-muted-foreground">
                          Ch
                        </span>
                        <input
                          type="number"
                          min={1}
                          max={2}
                          value={route.channels ?? 1}
                          onChange={(event) =>
                            patchRoute(route.id, {
                              channels: Number(event.target.value),
                            })
                          }
                          className="w-full rounded-md border border-border bg-input/40 px-2 py-1 text-right font-mono text-xs text-foreground"
                        />
                      </label>
                    </div>
                    <div className="rounded-md border border-border bg-background/40 px-2 py-2">
                      <div className="mb-2 flex items-center justify-between">
                        <span className="text-[10px] text-muted-foreground">
                          Volume
                        </span>
                        <span className="font-mono text-[10px] text-muted-foreground">
                          {s.audioVolume}%
                        </span>
                      </div>
                      <div className="flex items-center gap-2">
                        {s.audioVolume === 0 ? (
                          <VolumeX className="h-4 w-4 text-muted-foreground" />
                        ) : (
                          <Volume2 className="h-4 w-4 text-muted-foreground" />
                        )}
                        <Slider
                          value={[s.audioVolume]}
                          min={0}
                          max={100}
                          step={1}
                          onValueChange={([audioVolume]) =>
                            actions.patchSettings({ audioVolume })
                          }
                          aria-label="Audio volume"
                        />
                      </div>
                    </div>
                  </div>
                )}

                {route.action === "udp" && (
                  <div className="mt-2 grid grid-cols-[1fr_72px] gap-2">
                    <label className="space-y-1">
                      <span className="text-[10px] text-muted-foreground">
                        UDP host
                      </span>
                      <input
                        value={route.udpHost ?? "127.0.0.1"}
                        disabled={!udpAvailable || state.receiving}
                        onChange={(event) =>
                          patchRoute(route.id, { udpHost: event.target.value })
                        }
                        className="w-full rounded-md border border-border bg-input/40 px-2 py-1 font-mono text-[11px] text-foreground disabled:opacity-50"
                      />
                    </label>
                    <label className="space-y-1">
                      <span className="text-[10px] text-muted-foreground">
                        Port
                      </span>
                      <input
                        type="number"
                        min={1}
                        max={65535}
                        value={route.udpPort ?? 5600}
                        disabled={!udpAvailable || state.receiving}
                        onChange={(event) =>
                          patchRoute(route.id, {
                            udpPort: Number(event.target.value),
                          })
                        }
                        className="w-full rounded-md border border-border bg-input/40 px-2 py-1 text-right font-mono text-xs text-foreground disabled:opacity-50"
                      />
                    </label>
                    <p className="col-span-2 text-[10px] text-muted-foreground">
                      {udpAvailable
                        ? "UDP forwarding runs in native/Tauri mode."
                        : "UDP forwarding is unavailable in browser/WebUSB mode."}
                    </p>
                  </div>
                )}

                <div className="mt-2 flex items-center justify-between gap-2 border-t border-border pt-2 font-mono text-[10px] text-muted-foreground">
                  <span>route {route.id}</span>
                  {udpUnavailable ? (
                    <span>native mode only</span>
                  ) : (
                    state.receiving && <span>restart to apply changes</span>
                  )}
                  <span>
                    {stats
                      ? `${stats.packets.toLocaleString()} pkt · ${stats.lastBytes} B`
                      : "idle"}
                  </span>
                </div>
              </div>
            );
          })}
          <button
            type="button"
            onClick={addRoute}
            disabled={state.receiving}
            className="inline-flex w-full items-center justify-center gap-1.5 rounded-md border border-border px-2.5 py-1.5 text-xs font-medium text-muted-foreground hover:text-foreground disabled:opacity-40"
          >
            <Plus className="h-3.5 w-3.5" /> Add route
          </button>
        </div>
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
        <FieldRow
          label="RTP reorder"
          hint={
            state.receiving ? "Restart required to apply" : "Off by default"
          }
        >
          <Toggle
            checked={s.rtpReorder}
            disabled={state.receiving}
            onChange={(v) => actions.patchSettings({ rtpReorder: v })}
            label="RTP reorder"
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
            {s.channelId} · {formatChannelIdHex(s.channelId)}
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
