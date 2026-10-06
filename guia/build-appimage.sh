#!/usr/bin/env bash
set -euo pipefail

# Build a portable x86_64 AppImage from the already-built local Docker image.
# The AppImage contains the Rust server, UI, firmware release files and guide;
# it never enables simulated/auto sources.

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
IMAGE="${RUVIEW_IMAGE:-ruview-local:latest}"
OUT_DIR="${RUVIEW_APPIMAGE_OUT_DIR:-$ROOT_DIR/releases}"
BUILD_DIR="$(mktemp -d /tmp/ruview-appimage.XXXXXX)"
APPDIR="$BUILD_DIR/RuView.AppDir"
TOOL="${APPIMAGETOOL:-$BUILD_DIR/appimagetool-x86_64.AppImage}"
CONTAINER="ruview-appimage-copy-$$"
cleanup() {
  docker rm "$CONTAINER" >/dev/null 2>&1 || true
  rm -rf "$BUILD_DIR"
}
trap cleanup EXIT

cd "$ROOT_DIR"
command -v docker >/dev/null || { echo "Falta Docker." >&2; exit 1; }
docker image inspect "$IMAGE" >/dev/null || {
  echo "Falta $IMAGE. Construye primero: docker build -f docker/Dockerfile.rust -t $IMAGE ." >&2
  exit 1
}

mkdir -p "$APPDIR/usr/lib/ruview" "$APPDIR/usr/share/ruview" "$APPDIR/usr/share/applications"
cp guia/appimage/AppRun "$APPDIR/AppRun"
cp guia/appimage/RuView.desktop "$APPDIR/usr/share/applications/RuView.desktop"
cp guia/appimage/RuView.desktop "$APPDIR/RuView.desktop"
cp guia/appimage/ruview.svg "$APPDIR/ruview.svg"
chmod +x "$APPDIR/AppRun"

docker create --name "$CONTAINER" "$IMAGE" >/dev/null
docker cp "$CONTAINER:/app/sensing-server" "$APPDIR/usr/lib/ruview/sensing-server"
docker cp "$CONTAINER:/app/ui" "$APPDIR/usr/share/ruview/ui"
chmod +x "$APPDIR/usr/lib/ruview/sensing-server"

# Keep the supported flash/provision helpers and release firmware beside the UI.
cp -a guia "$APPDIR/usr/share/ruview/guia"
rm -rf "$APPDIR/usr/share/ruview/guia/appimage"
mkdir -p "$APPDIR/usr/share/ruview/firmware"
cp -a firmware/esp32-csi-node/release_bins "$APPDIR/usr/share/ruview/firmware/"
cp -a firmware/esp32-csi-node/partitions_*.csv "$APPDIR/usr/share/ruview/firmware/"

if [[ ! -x "$TOOL" ]]; then
  command -v curl >/dev/null || { echo "Falta curl para descargar appimagetool." >&2; exit 1; }
  curl -fsSL \
    "https://github.com/AppImage/appimagetool/releases/download/continuous/appimagetool-x86_64.AppImage" \
    -o "$TOOL"
  chmod +x "$TOOL"
fi

mkdir -p "$OUT_DIR"
OUTPUT="$OUT_DIR/RuView-ESP32-x86_64.AppImage"
ARCH=x86_64 "$TOOL" "$APPDIR" "$OUTPUT"
chmod +x "$OUTPUT"
echo "AppImage creada: $OUTPUT"
