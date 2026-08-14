#!/usr/bin/env bash
set -Eeuo pipefail

binary=${1:-src-tauri/target/release/orbit-launcher}

if [[ ! -x "$binary" ]]; then
  echo "::error::Binário não encontrado ou não executável: $binary"
  exit 1
fi

if ! command -v xvfb-run >/dev/null 2>&1; then
  echo "::error::xvfb-run não está instalado"
  exit 1
fi

tmpdir=$(mktemp -d)
logfile="$tmpdir/orbit.log"
pid=""

cleanup() {
  if [[ -n "$pid" ]] && kill -0 "$pid" 2>/dev/null; then
    kill "$pid" 2>/dev/null || true
    wait "$pid" 2>/dev/null || true
  fi

  if [[ -f "$logfile" ]]; then
    echo
    echo "===== Orbit smoke log ====="
    cat "$logfile"
    echo "==========================="
  fi

  rm -rf "$tmpdir"
}
trap cleanup EXIT

export XDG_DATA_HOME="$tmpdir/data"
export XDG_CONFIG_HOME="$tmpdir/config"
export XDG_CACHE_HOME="$tmpdir/cache"
export XDG_RUNTIME_DIR="$tmpdir/runtime"

# Xvfb em CI não possui um dispositivo DRM/DRI3 real. O smoke test valida
# startup, persistência e single-instance; ele não mede performance gráfica.
export LIBGL_ALWAYS_SOFTWARE="${LIBGL_ALWAYS_SOFTWARE:-true}"
export WEBKIT_DISABLE_DMABUF_RENDERER="${WEBKIT_DISABLE_DMABUF_RENDERER:-1}"
export WEBKIT_DISABLE_COMPOSITING_MODE="${WEBKIT_DISABLE_COMPOSITING_MODE:-1}"
export RUST_BACKTRACE="${RUST_BACKTRACE:-1}"

mkdir -m 700 -p "$XDG_RUNTIME_DIR"

socket="$XDG_RUNTIME_DIR/orbit-launcher.sock"
database="$XDG_DATA_HOME/io.orbit.launcher/orbit.db"

echo "Iniciando Orbit em perfil XDG limpo"
echo "Binário: $binary"
echo "XDG_DATA_HOME: $XDG_DATA_HOME"
echo "XDG_RUNTIME_DIR: $XDG_RUNTIME_DIR"

timeout 20s xvfb-run -a \
  --server-args="-screen 0 1280x800x24" \
  "$binary" --hidden \
  >"$logfile" 2>&1 &
pid=$!

for _ in $(seq 1 100); do
  if [[ -S "$socket" ]]; then
    break
  fi

  if ! kill -0 "$pid" 2>/dev/null; then
    echo "::error::Orbit encerrou antes de criar o socket single-instance"
    exit 1
  fi

  sleep 0.1
done

if [[ ! -S "$socket" ]]; then
  echo "::error::Socket single-instance não foi criado: $socket"
  exit 1
fi

echo "✓ Socket single-instance criado"

# A segunda instância deve detectar a primeira pelo socket e encerrar sem
# inicializar outra WebView.
if ! timeout 5s "$binary" --hidden >>"$logfile" 2>&1; then
  echo "::error::Segunda instância não encerrou corretamente"
  exit 1
fi

echo "✓ Single-instance validado"

for _ in $(seq 1 100); do
  if [[ -f "$database" ]]; then
    break
  fi

  if ! kill -0 "$pid" 2>/dev/null; then
    echo "::error::Orbit encerrou antes de criar o banco SQLite"
    exit 1
  fi

  sleep 0.1
done

if [[ ! -f "$database" ]]; then
  echo "::error::Banco Orbit não foi criado: $database"
  echo "Conteúdo do perfil temporário:"
  find "$tmpdir" -maxdepth 6 -print || true
  exit 1
fi

echo "✓ Banco SQLite criado"

kill "$pid" 2>/dev/null || true
wait "$pid" 2>/dev/null || true
pid=""

echo "Smoke test em perfil XDG limpo concluído com sucesso"
