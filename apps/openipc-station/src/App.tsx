import { AppHeader } from "@/components/openipc/app-header";
import { SettingsPanel } from "@/components/openipc/settings-panel";
import { VideoPanel } from "@/components/openipc/video-panel";
import { useOpenIpcRuntime } from "@/hooks/use-openipc-runtime";
import { isTauriRuntime } from "@/runtime/tauri";

export default function App() {
  if (import.meta.env.MODE === "desktop" && !isTauriRuntime()) {
    return (
      <main className="grid min-h-svh place-items-center bg-background p-6 text-foreground">
        <section className="max-w-md text-center">
          <h1 className="text-2xl font-semibold">OpenIPC RS Desktop</h1>
          <p className="mt-3 text-sm text-muted-foreground">
            This development server is reserved for the Tauri desktop window.
          </p>
        </section>
      </main>
    );
  }

  return <OpenIpcApp />;
}

function OpenIpcApp() {
  const openipc = useOpenIpcRuntime();

  return (
    <main className="grid min-h-svh grid-rows-[auto_minmax(0,1fr)] bg-background text-foreground">
      <AppHeader
        canStart={openipc.canStart}
        onConnect={openipc.actions.connectUsb}
        onStart={openipc.actions.startRx}
        onStop={openipc.actions.stopRx}
        onToggleRecording={openipc.actions.toggleRecording}
        recording={openipc.recording}
        running={openipc.running}
        runtime={openipc.runtime}
        statusLabel={openipc.statusLabel}
        wasmReady={openipc.wasmReady}
      />

      <section className="grid min-h-0 grid-cols-1 lg:grid-cols-[minmax(420px,1fr)_300px] xl:grid-cols-[minmax(560px,1fr)_320px]">
        <VideoPanel
          activeResolution={openipc.activeResolution}
          canvasRef={openipc.canvasRef}
          diagnostics={openipc.diagnostics}
          fecRecovered={openipc.fecRecovered}
          linkQuality={openipc.linkQuality}
          metrics={openipc.metrics}
          onResetCounters={openipc.actions.resetCounters}
          packetLoss={openipc.packetLoss}
          recording={openipc.recording}
          videoStats={openipc.videoStats}
          webCodecsSupported={openipc.webCodecsSupported}
        />

        <SettingsPanel
          authorizedDevices={openipc.authorizedDevices}
          desktopRuntime={openipc.desktopRuntime}
          diagnostics={openipc.diagnostics}
          fullscreen={openipc.fullscreen}
          keyName={openipc.keyName}
          logs={openipc.logs}
          metrics={openipc.metrics}
          onCloseDecoder={openipc.actions.closeDecoder}
          onLoadKey={openipc.actions.loadKey}
          onRefreshDevices={openipc.actions.refreshAuthorizedDevices}
          onSetFullscreen={openipc.actions.setFullscreen}
          running={openipc.running}
          selectedDeviceKnown={openipc.selectedDeviceKnown}
          setSettings={openipc.setSettings}
          settings={openipc.settings}
          videoStats={openipc.videoStats}
          wasmReady={openipc.wasmReady}
          webUsbSupported={openipc.webUsbSupported}
        />
      </section>
    </main>
  );
}
