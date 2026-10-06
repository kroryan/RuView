#!/usr/bin/env bash
set -euo pipefail

if [[ $# -ne 2 || "$2" != "--confirm" ]]; then
  echo "Uso: $0 /dev/serial/by-id/... --confirm" >&2
  echo "Ejecuta primero build-s3-firmware.sh 8mb --clean y flash_id." >&2
  exit 2
fi

PORT="$1"
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
FIRMWARE_DIR="$REPO_ROOT/firmware/esp32-csi-node"
PYTHON_BIN="${PYTHON_BIN:-$REPO_ROOT/.venv-ruview/bin/python}"

[[ -x "$PYTHON_BIN" ]] || { echo "Falta $PYTHON_BIN; crea .venv-ruview o define PYTHON_BIN." >&2; exit 1; }
[[ -e "$PORT" ]] || { echo "No existe el puerto: $PORT" >&2; exit 1; }
for artifact in bootloader/bootloader.bin partition_table/partition-table.bin ota_data_initial.bin esp32-csi-node.bin; do
  [[ -f "$FIRMWARE_DIR/build/$artifact" ]] || { echo "Falta build/$artifact; compila primero." >&2; exit 1; }
done

"$PYTHON_BIN" -m esptool --chip esp32s3 --port "$PORT" chip_id
"$PYTHON_BIN" -m esptool --chip esp32s3 --port "$PORT" flash_id
echo "Revisa que flash_id indique 8 MB. El siguiente paso sobrescribe el firmware, no NVS."

"$PYTHON_BIN" -m esptool --chip esp32s3 --port "$PORT" --baud 460800 \
  --before default_reset --after hard_reset write_flash \
  --flash-mode dio --flash-size 8MB --flash-freq 80m \
  0x0 "$FIRMWARE_DIR/build/bootloader/bootloader.bin" \
  0x8000 "$FIRMWARE_DIR/build/partition_table/partition-table.bin" \
  0xf000 "$FIRMWARE_DIR/build/ota_data_initial.bin" \
  0x20000 "$FIRMWARE_DIR/build/esp32-csi-node.bin"
