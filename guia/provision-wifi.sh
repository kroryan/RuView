#!/usr/bin/env bash
set -euo pipefail

if [[ $# -lt 4 || $# -gt 5 || ( $# -eq 5 && "$5" != "--reset-state" ) ]]; then
  echo "Uso: $0 PUERTO SSID IP_DEL_PC NODE_ID [--reset-state]" >&2
  echo "Ejemplo: $0 /dev/serial/by-id/... 'MiWiFi' 192.168.1.20 2" >&2
  exit 2
fi

PORT="$1"
SSID="$2"
TARGET_IP="$3"
NODE_ID="$4"
RESET_STATE="${5:-}"
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
PYTHON_BIN="${PYTHON_BIN:-$REPO_ROOT/.venv-ruview/bin/python}"

[[ -x "$PYTHON_BIN" ]] || { echo "Falta $PYTHON_BIN; crea .venv-ruview o define PYTHON_BIN." >&2; exit 1; }
read -r -s -p "Contraseña Wi-Fi (no se guarda en la guía): " WIFI_PASSWORD
echo

RESET_ARGS=()
if [[ "$RESET_STATE" == "--reset-state" ]]; then
  RESET_ARGS+=(--reset)
fi

"$PYTHON_BIN" "$REPO_ROOT/firmware/esp32-csi-node/provision.py" \
  --port "$PORT" --chip esp32s3 \
  --ssid "$SSID" --password "$WIFI_PASSWORD" \
  --target-ip "$TARGET_IP" --target-port 5005 --node-id "$NODE_ID" \
  "${RESET_ARGS[@]}"

unset WIFI_PASSWORD
echo "Provisionado terminado. Se actualizó NVS, no el firmware completo."
echo "Espera el reinicio de la ESP32 y valida /health y simulator=false."
