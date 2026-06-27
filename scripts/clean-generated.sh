#!/usr/bin/env sh
set -eu

ROOT_DIR=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)

TARGETS="
target
.cargo-tools
crates/openipc-web/pkg
apps/openipc-station/node_modules
apps/openipc-station/dist
apps/openipc-station/.wrangler
apps/openipc-station/src-tauri/gen
apps/openipc-station/src-tauri/target
docs/node_modules
docs/build
docs/.docusaurus
docs/.wrangler
docs/bun.lock
"

DRY_RUN=0
FOUND=0
case "${1:-}" in
  "")
    ;;
  "--dry-run")
    DRY_RUN=1
    ;;
  "--help"|"-h")
    cat <<'EOF'
Usage: scripts/clean-generated.sh [--dry-run]

Removes generated build artifacts, dependency installs, and local tool caches.
Does not remove source files, lockfiles, or the default demo key at
apps/openipc-station/public/gs.key.
EOF
    exit 0
    ;;
  *)
    printf 'unknown argument: %s\n' "$1" >&2
    printf 'usage: scripts/clean-generated.sh [--dry-run]\n' >&2
    exit 1
    ;;
esac

for target in $TARGETS; do
  path="$ROOT_DIR/$target"
  if [ "$DRY_RUN" -eq 1 ] && [ -e "$path" ]; then
    FOUND=1
    printf 'would remove %s\n' "$target"
  elif [ -e "$path" ]; then
    FOUND=1
    printf 'removing %s\n' "$target"
    rm -rf -- "$path"
  fi
done

for tarball in "$ROOT_DIR"/*.tgz; do
  [ -e "$tarball" ] || continue
  name=${tarball#"$ROOT_DIR"/}
  if [ "$DRY_RUN" -eq 1 ]; then
    FOUND=1
    printf 'would remove %s\n' "$name"
  else
    FOUND=1
    printf 'removing %s\n' "$name"
    rm -f -- "$tarball"
  fi
done

if [ "$DRY_RUN" -eq 1 ] && [ "$FOUND" -eq 0 ]; then
  printf 'nothing to remove\n'
fi
