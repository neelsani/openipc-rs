export type OpenIpcBuildInfo = {
  commit: string | null;
  shortCommit: string | null;
  tag: string | null;
  dirty: boolean;
  repoUrl: string;
  commitUrl: string;
};

const REPO_URL = "https://github.com/neelsani/openipc-rs";

function clean(value: unknown): string | null {
  if (typeof value !== "string") {
    return null;
  }
  const trimmed = value.trim();
  return trimmed.length > 0 ? trimmed : null;
}

function enabled(): boolean {
  return clean(import.meta.env.VITE_OPENIPC_BUILD_METADATA) === "true";
}

function currentBuildInfo(): OpenIpcBuildInfo {
  const commit = enabled()
    ? (clean(import.meta.env.VITE_OPENIPC_GIT_COMMIT)?.slice(0, 40) ?? null)
    : null;
  const shortCommit =
    commit === null
      ? null
      : (clean(import.meta.env.VITE_OPENIPC_GIT_SHORT_COMMIT) ??
        commit.slice(0, 7));

  return {
    commit,
    shortCommit,
    tag: commit === null ? null : clean(import.meta.env.VITE_OPENIPC_GIT_TAG),
    dirty: false,
    repoUrl: REPO_URL,
    commitUrl: commit ? `${REPO_URL}/commit/${commit}` : REPO_URL,
  };
}

export const buildInfo: OpenIpcBuildInfo = currentBuildInfo();

export function buildInfoTitle(info: OpenIpcBuildInfo): string {
  const parts = [];
  if (info.commit) {
    parts.push(`commit ${info.commit}`);
  }
  if (info.tag) {
    parts.push(`tag ${info.tag}`);
  }
  if (info.dirty) {
    parts.push("dirty working tree");
  }
  return parts.length > 0 ? parts.join(" · ") : "build metadata unavailable";
}
