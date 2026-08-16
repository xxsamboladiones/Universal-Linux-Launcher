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
export LIBGL_DRI3_DISABLE=1
export MESA_LOADER_DRIVER_OVERRIDE=llvmpipe
export WEBKIT_DISABLE_DMABUF_RENDERER=1
export WEBKIT_DISABLE_COMPOSITING_MODE=1
export GDK_BACKEND=x11
mkdir -m 700 -p "$XDG_RUNTIME_DIR"

if ! command -v xvfb-run >/dev/null 2>&1; then
  echo "Smoke test ignorado: xvfb-run não está instalado"
  exit 0
fi

# Desktop cache refresh is optional integration work and is not part of this
# clean-profile test. On a headless runner, kbuildsycoca6 can wait for a KDE
# session and block Orbit startup. Stub both commands so we test Orbit itself.
stub_bin="$tmpdir/bin"
mkdir -p "$stub_bin"
cat >"$stub_bin/gtk-update-icon-cache" <<'EOF'
#!/usr/bin/env bash
exit 0
EOF
cat >"$stub_bin/kbuildsycoca6" <<'EOF'
#!/usr/bin/env bash
exit 0
EOF
chmod +x "$stub_bin/gtk-update-icon-cache" "$stub_bin/kbuildsycoca6"
export PATH="$stub_bin:$PATH"

run_sandboxed() {
  dbus-run-session -- xvfb-run -a -s "-screen 0 1280x720x24 -nolisten tcp" bash -c '
    set -euo pipefail
    binary=$1
    logfile=$2

    timeout --kill-after=5s 30s "$binary" --hidden >"$logfile" 2>&1 &
    first_pid=$!

    for _ in $(seq 1 100); do
      if [[ -f "$XDG_DATA_HOME/io.orbit.launcher/orbit.db" ]]; then
        break
      fi
      if ! kill -0 "$first_pid" 2>/dev/null; then
        echo "Orbit encerrou antes de inicializar" >&2
        wait "$first_pid" || true
        exit 1
      fi
      sleep 0.1
    done

    test -f "$XDG_DATA_HOME/io.orbit.launcher/orbit.db"

    socket="$XDG_RUNTIME_DIR/orbit-launcher.sock"
    test -S "$socket"

    before_count=$(pgrep -x orbit-launcher | wc -l)
    test "$before_count" -ge 1

    set +e
    timeout --kill-after=5s 5s "$binary" --hidden >>"$logfile" 2>&1
    second_status=$?
    set -e

    after_count=$(pgrep -x orbit-launcher | wc -l)
    test "$after_count" -eq "$before_count"

    if [[ "$second_status" -ne 0 && "$second_status" -ne 124 ]]; then
      echo "Segunda inicialização retornou código inesperado: $second_status" >&2
      exit 1
    fi

    kill "$first_pid" 2>/dev/null || true
    wait "$first_pid" 2>/dev/null || true
  ' _ "$binary" "$logfile"
}

run_sandboxed &
pid=$!
wait "$pid"
pid=""

echo "Smoke test em perfil XDG limpo concluído"
