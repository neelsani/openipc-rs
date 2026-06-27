import type { ChangeEvent, Dispatch, SetStateAction } from "react";
import { FileKey2, RotateCcw, Settings2, Wifi } from "lucide-react";
import { Button, buttonVariants } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { NativeSelect } from "@/components/ui/native-select";
import { Slider } from "@/components/ui/slider";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import {
  AVIATEUR_CHANNELS,
  AVIATEUR_CHANNEL_WIDTHS,
} from "@/lib/settings";
import type {
  AuthorizedUsbDevice,
  ChannelWidthMhz,
  DiagnosticsState,
  LogEntry,
  Metrics,
  Settings,
  VideoStats,
} from "@/lib/types";
import { authorizedDeviceId, authorizedDeviceLabel } from "@/lib/usb";
import { cn } from "@/lib/utils";
import type { VideoCodecPreference } from "@/video";
import { DiagnosticsPanel } from "./diagnostics-panel";
import { CheckSetting, FieldStack, SectionHeading } from "./ui-parts";

export function SettingsPanel({
  settings,
  setSettings,
  desktopRuntime,
  webUsbSupported,
  wasmReady,
  running,
  authorizedDevices,
  selectedDeviceKnown,
  keyName,
  fullscreen,
  onRefreshDevices,
  onLoadKey,
  onSetFullscreen,
  onCloseDecoder,
  diagnostics,
  logs,
  metrics,
  videoStats,
}: {
  settings: Settings;
  setSettings: Dispatch<SetStateAction<Settings>>;
  desktopRuntime: boolean;
  webUsbSupported: boolean;
  wasmReady: boolean;
  running: boolean;
  authorizedDevices: AuthorizedUsbDevice[];
  selectedDeviceKnown: boolean;
  keyName: string;
  fullscreen: boolean;
  onRefreshDevices: () => void;
  onLoadKey: (event: ChangeEvent<HTMLInputElement>) => void;
  onSetFullscreen: (enabled: boolean) => void;
  onCloseDecoder: () => void;
  diagnostics: DiagnosticsState;
  logs: LogEntry[];
  metrics: Metrics;
  videoStats: VideoStats;
}) {
  return (
    <aside className="min-w-0 overflow-auto border-l bg-sidebar p-3">
      <Tabs defaultValue="wifi">
        <TabsList>
          <TabsTrigger value="wifi">Wi-Fi</TabsTrigger>
          <TabsTrigger value="settings">Settings</TabsTrigger>
          <TabsTrigger value="diagnostics">Diagnostics</TabsTrigger>
        </TabsList>

        <TabsContent className="space-y-4" value="wifi">
          <section>
            <SectionHeading icon={<Wifi className="size-4" />} title="Wi-Fi" />
            <div className="grid gap-3">
              <FieldStack label="Device">
                <div className="grid grid-cols-[minmax(0,1fr)_2.25rem] gap-2">
                  <NativeSelect
                    disabled={!webUsbSupported || running || desktopRuntime}
                    onChange={(event) =>
                      setSettings((current) => ({ ...current, wifiDevice: event.target.value }))
                    }
                    value={settings.wifiDevice}
                  >
                    <option value="">{desktopRuntime ? "First supported adapter" : "Browser prompt"}</option>
                    {!selectedDeviceKnown ? <option value={settings.wifiDevice}>{settings.wifiDevice}</option> : null}
                    {authorizedDevices.map((device, index) => (
                      <option value={authorizedDeviceId(device)} key={`${authorizedDeviceId(device)}-${index}`}>
                        {authorizedDeviceLabel(device)}
                      </option>
                    ))}
                  </NativeSelect>
                  <Button
                    disabled={!wasmReady || running}
                    onClick={onRefreshDevices}
                    size="icon"
                    title="Refresh devices"
                    type="button"
                    variant="outline"
                  >
                    <RotateCcw className="size-4" />
                  </Button>
                </div>
              </FieldStack>

              <FieldStack label="Channel">
                <NativeSelect
                  disabled={running}
                  onChange={(event) =>
                    setSettings((current) => ({ ...current, rfChannel: Number(event.target.value) }))
                  }
                  value={settings.rfChannel}
                >
                  {AVIATEUR_CHANNELS.map(([channel, label]) => (
                    <option value={channel} key={channel}>
                      {label}
                    </option>
                  ))}
                </NativeSelect>
              </FieldStack>

              <FieldStack label="Channel width">
                <NativeSelect
                  disabled={running}
                  onChange={(event) =>
                    setSettings((current) => ({
                      ...current,
                      channelWidthMhz: Number(event.target.value) as ChannelWidthMhz,
                    }))
                  }
                  value={settings.channelWidthMhz}
                >
                  {AVIATEUR_CHANNEL_WIDTHS.map(([width, label]) => (
                    <option value={width} key={width}>
                      {label}
                    </option>
                  ))}
                </NativeSelect>
              </FieldStack>

              <FieldStack label="Key">
                <div className="grid grid-cols-[minmax(0,1fr)_auto] gap-2">
                  <div className="flex h-9 min-w-0 items-center gap-2 rounded-md border bg-background px-3 text-sm shadow-sm">
                    <FileKey2 className="size-4 shrink-0 text-muted-foreground" />
                    <span className="truncate">{keyName === "No key" ? "Default" : keyName}</span>
                  </div>
                  <input className="sr-only" id="openipc-keypair" onChange={onLoadKey} type="file" />
                  <label
                    className={cn(buttonVariants({ variant: "outline", size: "sm" }), "h-9 cursor-pointer")}
                    htmlFor="openipc-keypair"
                  >
                    Open
                  </label>
                </div>
              </FieldStack>

              <div className="rounded-md border p-2">
                <CheckSetting
                  checked={settings.adaptiveEnabled}
                  label="Adaptive link"
                  onCheckedChange={(checked) =>
                    setSettings((current) => ({ ...current, adaptiveEnabled: checked }))
                  }
                />
                {settings.adaptiveEnabled ? (
                  <FieldStack className="mt-2" label="Uplink TX power">
                    <div className="grid grid-cols-[minmax(0,1fr)_4.5rem] items-center gap-3">
                      <Slider
                        max={40}
                        min={1}
                        onValueChange={([value]) =>
                          setSettings((current) => ({
                            ...current,
                            alinkTxPower: value ?? current.alinkTxPower,
                          }))
                        }
                        step={1}
                        value={[settings.alinkTxPower]}
                      />
                      <strong className="text-right font-mono text-xs text-foreground">
                        {settings.alinkTxPower} mW
                      </strong>
                    </div>
                  </FieldStack>
                ) : null}
              </div>
            </div>
          </section>
        </TabsContent>

        <TabsContent className="space-y-4" value="settings">
          <section>
            <SectionHeading icon={<Settings2 className="size-4" />} title="Settings" />
            <div className="grid gap-3">
              <CheckSetting checked={fullscreen} label="Fullscreen" onCheckedChange={onSetFullscreen} />
              <CheckSetting
                checked={settings.darkMode}
                label="Dark mode"
                onCheckedChange={(checked) =>
                  setSettings((current) => ({ ...current, darkMode: checked }))
                }
              />
            </div>
          </section>

          <details className="rounded-md border bg-card">
            <summary className="cursor-pointer px-3 py-2 text-sm font-medium">Advanced</summary>
            <div className="grid gap-3 border-t p-3">
              <FieldStack label="Channel ID">
                <Input
                  onChange={(event) =>
                    setSettings((current) => ({ ...current, channelId: event.target.value }))
                  }
                  value={settings.channelId}
                />
              </FieldStack>
              <FieldStack label="Minimum epoch">
                <Input
                  onChange={(event) =>
                    setSettings((current) => ({ ...current, minimumEpoch: event.target.value }))
                  }
                  value={settings.minimumEpoch}
                />
              </FieldStack>
              <FieldStack label="Transfer size">
                <NativeSelect
                  onChange={(event) =>
                    setSettings((current) => ({ ...current, transferSize: Number(event.target.value) }))
                  }
                  value={settings.transferSize}
                >
                  <option value={16 * 1024}>16 KiB</option>
                  <option value={32 * 1024}>32 KiB</option>
                  <option value={64 * 1024}>64 KiB</option>
                </NativeSelect>
              </FieldStack>
              <FieldStack label="Video codec">
                <NativeSelect
                  onChange={(event) => {
                    onCloseDecoder();
                    setSettings((current) => ({
                      ...current,
                      videoCodec: event.target.value as VideoCodecPreference,
                    }));
                  }}
                  value={settings.videoCodec}
                >
                  <option value="auto">Auto</option>
                  <option value="h264">H.264</option>
                  <option value="h265">H.265</option>
                </NativeSelect>
              </FieldStack>
              <FieldStack label="Channel offset">
                <Input
                  disabled={running}
                  max={3}
                  min={0}
                  onChange={(event) =>
                    setSettings((current) => ({ ...current, channelOffset: Number(event.target.value) }))
                  }
                  type="number"
                  value={settings.channelOffset}
                />
              </FieldStack>
            </div>
          </details>
        </TabsContent>

        <TabsContent className="space-y-4" value="diagnostics">
          <DiagnosticsPanel diagnostics={diagnostics} logs={logs} metrics={metrics} videoStats={videoStats} />
        </TabsContent>
      </Tabs>
    </aside>
  );
}
