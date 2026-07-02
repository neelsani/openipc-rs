import { CommandBar } from "@/components/station/command-bar";
import { Inspector } from "@/components/station/inspector";
import { NebulusBanner } from "@/components/station/nebulus-banner";
import { VideoPanel } from "@/components/station/video-panel";
import { useStation } from "@/lib/use-station";
import { isTauriRuntime } from "@/runtime/tauri";

export default function App() {
  if (import.meta.env.MODE === "desktop" && !isTauriRuntime()) {
    return (
      <main className="grid min-h-svh place-items-center bg-background p-6 text-foreground">
        <section className="max-w-md text-center">
          <h1 className="text-2xl font-semibold">OpenIPC Station Desktop</h1>
          <p className="mt-3 text-sm text-muted-foreground">
            This development server is reserved for the Tauri desktop window.
          </p>
        </section>
      </main>
    );
  }

  return <OpenIpcStation />;
}

function OpenIpcStation() {
  const api = useStation();

  return (
    <div className="flex min-h-svh flex-col bg-background text-foreground lg:h-svh lg:overflow-hidden">
      <CommandBar api={api} />
      {!isTauriRuntime() && <NebulusBanner />}

      <main className="flex min-h-0 flex-1 flex-col gap-3 p-3 lg:flex-row lg:items-start lg:overflow-hidden lg:p-4">
        <section className="flex min-w-0 flex-1 flex-col lg:min-h-0 lg:overflow-y-auto">
          <div id="video-region" className="bg-background">
            <VideoPanel api={api} />
          </div>
        </section>

        <Inspector
          api={api}
          className="w-full shrink-0 lg:max-h-[calc(100svh-5rem)] lg:w-[400px] lg:self-start"
        />
      </main>
    </div>
  );
}
