import { ArrowUpRight, Sparkles } from "lucide-react";

const NEBULUS_URL = "https://nebulus.openipc-rs.neels.dev";

export function NebulusBanner() {
  return (
    <aside className="border-b border-primary/20 bg-primary/5 px-3 py-2 text-foreground">
      <div className="mx-auto flex max-w-5xl flex-wrap items-center justify-center gap-x-3 gap-y-1 text-center text-xs sm:text-left">
        <Sparkles className="h-3.5 w-3.5 shrink-0 text-primary" />
        <span>Nebulus is the new Rust-native OpenIPC ground station.</span>
        <a
          href={NEBULUS_URL}
          target="_blank"
          rel="noreferrer"
          className="inline-flex items-center gap-1 font-medium text-primary underline-offset-4 hover:underline"
        >
          Try Nebulus
          <ArrowUpRight className="h-3.5 w-3.5" />
        </a>
      </div>
    </aside>
  );
}
