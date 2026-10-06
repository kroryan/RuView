#!/usr/bin/env bash
set -euo pipefail

usage() {
  echo "Uso: $0 8mb|16mb [--clean]"
  echo "  8mb  ESP32-S3 actual / DevKitC sin pantalla"
  echo "  16mb ESP32-S3 N16/N16R8 / DevKitC sin pantalla"
}

[[ $# -ge 1 && $# -le 2 ]] || { usage >&2; exit 2; }
SIZE="$1"
CLEAN="${2:-}"
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
FIRMWARE_DIR="$REPO_ROOT/firmware/esp32-csi-node"

case "$SIZE" in
  8mb)
    DEFAULTS="sdkconfig.defaults;sdkconfig.defaults.devkitc"
    ;;
  16mb)
    DEFAULTS="sdkconfig.defaults;sdkconfig.defaults.16mb;sdkconfig.defaults.devkitc"
    ;;
  *)
    usage >&2
    exit 2
    ;;
esac

if [[ "$CLEAN" == "--clean" ]]; then
  rm -rf "$FIRMWARE_DIR/build" "$FIRMWARE_DIR/sdkconfig"
elif [[ -e "$FIRMWARE_DIR/build" || -e "$FIRMWARE_DIR/sdkconfig" ]]; then
  echo "Ya existe build/sdkconfig. Usa --clean para reconstruir con el mapa $SIZE." >&2
  exit 1
fi

command -v docker >/dev/null || { echo "Falta docker." >&2; exit 1; }

MSYS_NO_PATHCONV=1 docker run --rm \
  -v "$FIRMWARE_DIR:/project" \
  -w /project \
  espressif/idf:v5.4 bash -c \
  "idf.py -DSDKCONFIG_DEFAULTS='$DEFAULTS' set-target esp32s3 && \
   idf.py -DSDKCONFIG_DEFAULTS='$DEFAULTS' build"

echo
echo "Firmware $SIZE compilado. Antes de flashear, confirma:"
if [[ "$SIZE" == "8mb" ]]; then
  echo '  CONFIG_ESPTOOLPY_FLASHSIZE="8MB"'
  echo '  CONFIG_PARTITION_TABLE_CUSTOM_FILENAME="partitions_display.csv"'
else
  echo '  CONFIG_ESPTOOLPY_FLASHSIZE="16MB"'
  echo '  CONFIG_PARTITION_TABLE_CUSTOM_FILENAME="partitions_16mb.csv"'
fi
