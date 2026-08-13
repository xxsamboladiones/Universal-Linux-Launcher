#!/usr/bin/env bash
set -euo pipefail
appimage=${1:?Uso: validate-appimage.sh arquivo.AppImage}
test -f "$appimage"
test -x "$appimage"
appimage=$(realpath "$appimage")
tmpdir=$(mktemp -d)
trap 'rm -rf "$tmpdir"' EXIT
(cd "$tmpdir" && "$appimage" --appimage-extract >/dev/null)
root="$tmpdir/squashfs-root"
test -x "$root/usr/bin/orbit-launcher" || test -x "$root/AppRun"
find "$root" -name 'io.orbit.launcher.desktop' -o -name '*.desktop' | grep -q .
find "$root" -type f \( -name '*.png' -o -name '*.svg' \) | grep -q .
echo "AppImage validado: $appimage"
