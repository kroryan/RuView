#!/usr/bin/env bash
set -euo pipefail

# Starts the local RuView server in hardware-only ESP32 mode.
# It never enables the simulator and preserves the ruview-data volume.

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
CONTAINER="ruview-sensing"
IMAGE="ruview-local:latest"
VOLUME="ruview-data"
ACTION="${1:-start}"

cd "$ROOT_DIR"

case "$ACTION" in
  stop)
    docker stop "$CONTAINER"
    exit 0
    ;;
  restart)
    if docker container inspect "$CONTAINER" >/dev/null 2>&1; then
      docker restart "$CONTAINER" >/dev/null
    else
      ACTION=start
    fi
    ;;
  status)
    curl -fsS http://127.0.0.1:3000/health
    printf '\n'
    curl -fsS http://127.0.0.1:3000/api/v1/status
    printf '\n'
    exit 0
    ;;
  start)
    if docker container inspect "$CONTAINER" >/dev/null 2>&1; then
      if [[ "$(docker inspect -f '{{.State.Running}}' "$CONTAINER")" != "true" ]]; then
        docker start "$CONTAINER" >/dev/null
      fi
    else
      docker image inspect "$IMAGE" >/dev/null 2>&1 || {
        echo "Falta la imagen $IMAGE. Ejecuta:" >&2
        echo "  docker build -f docker/Dockerfile.rust -t $IMAGE ." >&2
        exit 1
      }
      docker volume inspect "$VOLUME" >/dev/null 2>&1 || docker volume create "$VOLUME" >/dev/null
      docker run -d \
        --name "$CONTAINER" \
        --restart unless-stopped \
        --security-opt=no-new-privileges:true \
        --cap-drop=ALL \
        --memory=1g \
        --cpus=2 \
        -e CSI_SOURCE=esp32 \
        -e RUVIEW_UDP_BIND=0.0.0.0 \
        -e RUVIEW_UDP_ALLOW=192.168.1.0/24 \
        -e RUVIEW_ALLOW_UNAUTHENTICATED=1 \
        -p 127.0.0.1:3000:3000 \
        -p 127.0.0.1:3001:3001 \
        -p 5005:5005/udp \
        -v "$VOLUME:/app/data" \
        "$IMAGE" >/dev/null
    fi
    ;;
  *)
    echo "Uso: $0 [start|restart|stop|status]" >&2
    exit 2
    ;;
esac

for _ in {1..30}; do
  if curl -fsS http://127.0.0.1:3000/health >/dev/null 2>&1; then
    echo "RuView activo en modo ESP32 real: http://127.0.0.1:3000/ui/index.html"
    curl -fsS http://127.0.0.1:3000/health
    printf '\n'
    exit 0
  fi
  sleep 1
done

echo "El contenedor arrancó pero el HTTP aún no responde." >&2
echo "Revisa: docker logs --tail 100 $CONTAINER" >&2
exit 1
