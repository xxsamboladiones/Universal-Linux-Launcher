#!/usr/bin/env bash
set -euo pipefail

binary=${1:-src-tauri/target/release/orbit-launcher}
test -x "$binary"

tmpdir=$(mktemp -d)
logfile="$tmpdir/orbit.log"
pid=""
cleanup() {
  if [[ -n "$pid" ]]; then
    kill "$pid" 2>/dev/null || true
    wait "$pid" 2>/dev/null || true
  fi
  if [[ -s "$logfile" ]]; then
    echo "=== Orbit smoke log ===" >&2
    cat "$logfile" >&2 || true
  fi
  rm -rf "$tmpdir"
}
trap cleanup EXIT

export XDG_DATA_HOME="$tmpdir/data"
export XDG_CONFIG_HOME="$tmpdir/config"
export XDG_CACHE_HOME="$tmpdir/cache"
export XDG_RUNTIME_DIR="$tmpdir/runtime"
export LIBGL_ALWAYS_SOFTWARE=1
export WEBKIT_DISABLE_DMABUF_RENDERER=1
export WEBKIT_DISABLE_COMPOSITING_MODE=1
mkdir -m 700 -p "$XDG_RUNTIME_DIR"

run_sandboxed() {
  if command -v dbus-run-session >/dev/null 2>&1; then
    dbus-run-session -- xvfb-run -a "$binary" --hidden
  else
    xvfb-run -a "$binary" --hidden
  fi
}

if ! command -v xvfb-run >/dev/null 2>&1; then
  echo "Smoke test ignorado: xvfb-run não está instalado"
  exit 0
fi

run_sandboxed >"$logfile" 2>&1 &
pid=$!

socket="$XDG_RUNTIME_DIR/orbit-launcher.sock"
for _ in $(seq 1 100); do
  if [[ -S "$socket" ]]; then
    break
  fi
  if ! kill -0 "$pid" 2>/dev/null; then
    echo "Orbit encerrou antes de criar o socket" >&2
    wait "$pid" || true
    exit 1
  fi
  sleep 0.1
done

test -S "$socket"
test -f "$XDG_DATA_HOME/io.orbit.launcher/orbit.db"

# A segunda inicialização deve falar com a instância existente e sair limpa.
if ! "$binary" --hidden >>"$logfile" 2>&1; then
  echo "Segunda inicialização do Orbit falhou" >&2
  exit 1
fi

echo "Smoke test em perfil XDG limpo concluído"
