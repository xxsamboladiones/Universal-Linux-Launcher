#!/usr/bin/env bash
set -euo pipefail
binary=${1:-src-tauri/target/release/orbit-launcher}
test -x "$binary"
tmpdir=$(mktemp -d)
trap 'rm -rf "$tmpdir"' EXIT
export XDG_DATA_HOME="$tmpdir/data"
export XDG_CONFIG_HOME="$tmpdir/config"
export XDG_CACHE_HOME="$tmpdir/cache"
export XDG_RUNTIME_DIR="$tmpdir/runtime"
mkdir -m 700 -p "$XDG_RUNTIME_DIR"
if command -v xvfb-run >/dev/null; then
  timeout 15s xvfb-run -a "$binary" --hidden &
  pid=$!
  for _ in $(seq 1 50); do test -S "$XDG_RUNTIME_DIR/orbit-launcher.sock" && break; sleep .1; done
  test -S "$XDG_RUNTIME_DIR/orbit-launcher.sock"
  "$binary" --hidden
  kill "$pid" 2>/dev/null || true
fi
test -f "$XDG_DATA_HOME/io.orbit.launcher/orbit.db" || test ! -x "$(command -v xvfb-run || true)"
echo "Smoke test em perfil XDG limpo concluído"
