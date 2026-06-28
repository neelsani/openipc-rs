import {
  useRef,
  useState,
  type ChangeEvent,
  type Dispatch,
  type SetStateAction,
} from "react";
import {
  FileKey2,
  Film,
  Play,
  RotateCcw,
  Settings2,
  Square,
  Usb,
  Wifi,
} from "lucide-react";
import { Button, buttonVariants } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { NativeSelect } from "@/components/ui/native-select";
import { Slider } from "@/components/ui/slider";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { AVIATEUR_CHANNELS, AVIATEUR_CHANNEL_WIDTHS } from "@/lib/settings";
import type {
  AuthorizedUsbDevice,
  ChannelWidthMhz,
  DiagnosticsState,
  LogEntry,
  Metrics,
  Settings,
  UsbInfo,
  VideoStats,
  WebCodecsCapabilities,
} from "@/lib/types";
import { authorizedDeviceId, authorizedDeviceLabel } from "@/lib/usb";
import { cn } from "@/lib/utils";
import type { VideoCodecPreference } from "@/video";
import { DiagnosticsPanel } from "./diagnostics-panel";
import {
  CheckSetting,
  ControlBlock,
  FieldStack,
  SectionHeading,
} from "./ui-parts";

export function SettingsPanel({
  settings,
  setSettings,
  desktopRuntime,
  webUsbSupported,
  wasmReady,
  running,
  recording,
  canStart,
  authorizedDevices,
  selectedDeviceKnown,
  keyName,
  fullscreen,
  onConnect,
  onRefreshDevices,
  onLoadKey,
  onSetFullscreen,
  onCloseDecoder,
  onStart,
  onStop,
  onToggleRecording,
  diagnostics,
  logs,
  metrics,
  usbInfo,
  videoStats,
  webCodecsCapabilities,
}: {
  settings: Settings;
  setSettings: Dispatch<SetStateAction<Settings>>;
  desktopRuntime: boolean;
  webUsbSupported: boolean;
  wasmReady: boolean;
  running: boolean;
  recording: boolean;
  canStart: boolean;
  authorizedDevices: AuthorizedUsbDevice[];
  selectedDeviceKnown: boolean;
  keyName: string;
  fullscreen: boolean;
  onConnect: () => void;
  onRefreshDevices: () => void;
  onLoadKey: (event: ChangeEvent<HTMLInputElement>) => void;
  onSetFullscreen: (enabled: boolean) => void;
  onCloseDecoder: () => void;
  onStart: () => void;
  onStop: () => void;
  onToggleRecording: () => void;
  diagnostics: DiagnosticsState;
  logs: LogEntry[];
  metrics: Metrics;
  usbInfo: UsbInfo | null;
  videoStats: VideoStats;
  webCodecsCapabilities: WebCodecsCapabilities;
}) {
  const asideRef = useRef<HTMLElement | null>(null);
  const [activeTab, setActiveTab] = useState("wifi");
  const startReason = startBlockedReason({
    canStart,
    desktopRuntime,
    running,
    usbInfo,
    wasmReady,
    webUsbSupported,
  });

  function handleTabChange(value: string) {
    setActiveTab(value);
    window.requestAnimationFrame(() => {
      if (asideRef.current) {
        asideRef.current.scrollTop = 0;
      }
    });
  }

  return (
    <aside
      className="order-first min-w-0 rounded-lg border border-border bg-sidebar p-2 shadow-sm lg:order-none lg:h-full lg:min-h-0 lg:overflow-y-auto lg:overscroll-contain"
      ref={asideRef}
    >
      <Tabs
        className="min-w-0"
        onValueChange={handleTabChange}
        value={activeTab}
      >
        <section className="-mx-2 -mt-2 mb-3 space-y-2 border-b border-border bg-sidebar p-2 lg:sticky lg:top-0 lg:z-30">
          <div className="rounded-lg border border-border bg-card p-2.5 shadow-sm">
            <div className="mb-2 flex items-center justify-between gap-3">
              <div className="min-w-0">
                <h2 className="text-sm font-semibold text-foreground">
                  Receiver
                </h2>
                <p className="truncate text-xs text-muted-foreground">
                  {startReason}
                </p>
              </div>
              <span className="rounded-full border border-border bg-muted/35 px-2 py-1 text-xs text-muted-foreground">
                {running ? "Live" : usbInfo ? "Linked" : "Idle"}
              </span>
            </div>
            <div className="grid grid-cols-2 gap-2">
              <Button
                className="h-10"
                disabled={!wasmReady || running}
                onClick={onConnect}
                type="button"
                variant="outline"
              >
                <Usb />
                Connect
              </Button>
              <Button
                className="h-10"
                disabled={!canStart}
                onClick={onStart}
                type="button"
              >
                <Play />
                Start RX
              </Button>
              <Button
                disabled={!running}
                onClick={onStop}
                type="button"
                variant="destructive"
              >
                <Square />
                Stop
              </Button>
              <Button
                className={
                  recording
                    ? "border-red-500/50 bg-red-500/10 text-red-700 hover:bg-red-500/15 dark:text-red-300"
                    : undefined
                }
                disabled={!wasmReady}
                onClick={onToggleRecording}
                type="button"
                variant="outline"
              >
                {recording ? <Square /> : <Film />}
                {recording ? "Stop Rec" : "Record"}
              </Button>
            </div>
          </div>
          <TabsList>
            <TabsTrigger value="wifi">Wi-Fi</TabsTrigger>
            <TabsTrigger value="settings">Settings</TabsTrigger>
            <TabsTrigger value="diagnostics">Diagnostics</TabsTrigger>
          </TabsList>
        </section>

        <TabsContent className="space-y-4" value="wifi">
          <ControlBlock>
            <SectionHeading icon={<Wifi className="size-4" />} title="Wi-Fi" />
            <div className="grid gap-3">
              <FieldStack label="Device">
                <div className="grid grid-cols-[minmax(0,1fr)_2.5rem] gap-2">
                  <NativeSelect
                    disabled={!webUsbSupported || running || desktopRuntime}
                    onChange={(event) =>
                      setSettings((current) => ({
                        ...current,
                        wifiDevice: event.target.value,
                      }))
                    }
                    value={settings.wifiDevice}
                  >
                    <option value="">
                      {desktopRuntime
                        ? "First supported adapter"
                        : "Browser prompt"}
                    </option>
                    {!selectedDeviceKnown ? (
                      <option value={settings.wifiDevice}>
                        {settings.wifiDevice}
                      </option>
                    ) : null}
                    {authorizedDevices.map((device, index) => (
                      <option
                        value={authorizedDeviceId(device)}
                        key={`${authorizedDeviceId(device)}-${index}`}
                      >
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
                    setSettings((current) => ({
                      ...current,
                      rfChannel: Number(event.target.value),
                    }))
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
                      channelWidthMhz: Number(
                        event.target.value,
                      ) as ChannelWidthMhz,
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
                  <div className="flex h-10 min-w-0 items-center gap-2 rounded-md border border-input bg-background px-3 text-sm shadow-sm">
                    <FileKey2 className="size-4 shrink-0 text-muted-foreground" />
                    <span className="truncate">
                      {keyName === "No key" ? "Default" : keyName}
                    </span>
                  </div>
                  <input
                    className="sr-only"
                    id="openipc-keypair"
                    onChange={onLoadKey}
                    type="file"
                  />
                  <label
                    className={cn(
                      buttonVariants({ variant: "outline", size: "sm" }),
                      "h-10 cursor-pointer",
                    )}
                    htmlFor="openipc-keypair"
                  >
                    Open
                  </label>
                </div>
              </FieldStack>

              <div className="rounded-md border border-border bg-muted/20 p-2.5">
                <CheckSetting
                  checked={settings.adaptiveEnabled}
                  label="Adaptive link"
                  onCheckedChange={(checked) =>
                    setSettings((current) => ({
                      ...current,
                      adaptiveEnabled: checked,
                    }))
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
          </ControlBlock>
        </TabsContent>

        <TabsContent className="space-y-4" value="settings">
          <ControlBlock>
            <SectionHeading
              icon={<Settings2 className="size-4" />}
              title="Settings"
            />
            <div className="grid gap-3">
              <CheckSetting
                checked={fullscreen}
                label="Fullscreen"
                onCheckedChange={onSetFullscreen}
              />
              <CheckSetting
                checked={settings.darkMode}
                label="Dark mode"
                onCheckedChange={(checked) =>
                  setSettings((current) => ({ ...current, darkMode: checked }))
                }
              />
            </div>
          </ControlBlock>

          <details className="rounded-lg border border-border bg-card shadow-sm">
            <summary className="cursor-pointer px-3 py-2 text-sm font-medium text-foreground">
              Advanced
            </summary>
            <div className="grid gap-3 border-t p-3">
              <FieldStack label="Channel ID">
                <Input
                  onChange={(event) =>
                    setSettings((current) => ({
                      ...current,
                      channelId: event.target.value,
                    }))
                  }
                  value={settings.channelId}
                />
              </FieldStack>
              <FieldStack label="Minimum epoch">
                <Input
                  onChange={(event) =>
                    setSettings((current) => ({
                      ...current,
                      minimumEpoch: event.target.value,
                    }))
                  }
                  value={settings.minimumEpoch}
                />
              </FieldStack>
              <FieldStack label="Transfer size">
                <NativeSelect
                  onChange={(event) =>
                    setSettings((current) => ({
                      ...current,
                      transferSize: Number(event.target.value),
                    }))
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
                    setSettings((current) => ({
                      ...current,
                      channelOffset: Number(event.target.value),
                    }))
                  }
                  type="number"
                  value={settings.channelOffset}
                />
              </FieldStack>
            </div>
          </details>
        </TabsContent>

        <TabsContent className="space-y-4" value="diagnostics">
          <DiagnosticsPanel
            diagnostics={diagnostics}
            logs={logs}
            metrics={metrics}
            videoStats={videoStats}
            webCodecsCapabilities={webCodecsCapabilities}
          />
        </TabsContent>
      </Tabs>
    </aside>
  );
}

function startBlockedReason({
  canStart,
  desktopRuntime,
  running,
  usbInfo,
  wasmReady,
  webUsbSupported,
}: {
  canStart: boolean;
  desktopRuntime: boolean;
  running: boolean;
  usbInfo: UsbInfo | null;
  wasmReady: boolean;
  webUsbSupported: boolean;
}) {
  if (running) {
    return "Receiving video";
  }
  if (canStart) {
    return "Ready to start";
  }
  if (!wasmReady) {
    return "Runtime loading";
  }
  if (!webUsbSupported) {
    return desktopRuntime ? "Native USB unavailable" : "WebUSB unavailable";
  }
  if (!usbInfo) {
    return "Connect an adapter first";
  }
  return "Waiting for receiver setup";
}
