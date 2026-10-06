# Guía de RuView

Esta instalación usa el servidor Rust de RuView y un nodo ESP32-S3 por UDP.
La interfaz local queda en `http://127.0.0.1:3000`.

Esta carpeta contiene los ayudantes de hardware:

- `build-s3-firmware.sh 8mb --clean`: compila la imagen de la ESP32-S3 actual.
- `build-s3-firmware.sh 16mb --clean`: compila la imagen para S3 N16/N16R8.
- `flash-s3-8mb.sh`: flashea una S3 de 8 MB tras comprobar `flash_id`.
- `flash-s3-16mb.sh`: flashea una S3 de 16 MB tras comprobar `flash_id`.
- `provision-wifi.sh`: configura Wi‑Fi, destino UDP y `node-id` pidiendo la
  contraseña sin guardarla en ningún archivo.
- `start-ruview.sh`: arranca o comprueba RuView en modo ESP32 real; nunca activa
  el simulador.
- `build-appimage.sh`: crea `releases/RuView-ESP32-x86_64.AppImage` con el
  servidor Rust, UI, firmware y esta guía. El AppImage arranca siempre con
  `--source esp32`; sin CSI real muestra que no hay hardware, no genera datos.

Los scripts se ejecutan desde cualquier directorio y localizan automáticamente
la raíz del repositorio. No mezcles una tabla de particiones de 8 MB con una
placa de 16 MB, ni al revés.

> El modo `simulated` genera datos sintéticos para comprobar la aplicación. No
> representa personas, postura, respiración ni pulso reales.

## Encender y apagar

### Encender RuView

Desde la raíz del repositorio:

```bash
./guia/start-ruview.sh
```

El script reutiliza el contenedor y el volumen de datos existentes. Si todavía
no existe la imagen, muestra el comando de build necesario.

Abre estas páginas:

- Dashboard: <http://127.0.0.1:3000/ui/index.html>
- Observatory: <http://127.0.0.1:3000/ui/observatory.html>
- Pose Fusion: <http://127.0.0.1:3000/ui/pose-fusion.html>
- Estado: <http://127.0.0.1:3000/health>

### Encender RuView con el ESP32

1. Conecta el ESP32-S3 por USB y espera a que arranque.
2. Comprueba que tenga alimentación y conexión Wi‑Fi.
3. Arranca el contenedor configurado para recibir CSI real, como se explica en
   [Usar el ESP32](#usar-el-esp32).
4. Abre la interfaz en <http://127.0.0.1:3000/ui/index.html>.

El ESP32 no tiene un apagado lógico seguro en este flujo: para apagarlo,
detén primero RuView y después desconecta el USB.

### Apagar o reiniciar

```bash
./guia/start-ruview.sh stop
./guia/start-ruview.sh start
./guia/start-ruview.sh restart
./guia/start-ruview.sh status
```

Para seguir los logs:

```bash
docker logs -f --tail 100 ruview-sensing
```

Pulsa `Ctrl+C` para dejar de seguir los logs; eso no detiene el contenedor.

## Preparar el servidor

El primer build compila el servidor dentro de Docker y puede tardar varios
minutos. Desde `/home/krox/Documents/ruview`:

```bash
docker build -f docker/Dockerfile.rust -t ruview-local:latest .
```

Esta instalación no usa el modo sintético. No arranques el contenedor con
`CSI_SOURCE=simulated` ni con `--source simulated`: esos modos generan datos
falsos y no sirven para validar una ESP32. El arranque de producción está en
[Usar el ESP32](#usar-el-esp32) y fija `CSI_SOURCE=esp32`, con una allowlist UDP
de los nodos reales.

## AppImage portátil

Una vez construida la imagen Docker local, crea el paquete portable así:

```bash
./guia/build-appimage.sh
```

El resultado queda en:

```text
releases/RuView-ESP32-x86_64.AppImage
```

En otro Linux x86_64 se ejecuta con doble clic o:

```bash
chmod +x releases/RuView-ESP32-x86_64.AppImage
./releases/RuView-ESP32-x86_64.AppImage
```

No necesita Docker, Rust, Node ni una instalación de RuView. Guarda el estado
en `~/.local/share/ruview/data` y abre el navegador automáticamente. La
allowlist UDP se calcula con la primera interfaz IPv4 global; si la red usa
varias interfaces, se puede fijar antes de arrancar:

```bash
RUVIEW_UDP_ALLOW=192.168.1.0/24 ./releases/RuView-ESP32-x86_64.AppImage
```

El paquete incluye los ayudantes de flasheo/provisionado y los binarios S3,
pero el acceso USB sigue necesitando `esptool`/Python en el sistema destino.
La configuración Wi‑Fi es dinámica: se hace con `provision-wifi.sh` y no se
compila dentro del firmware. Para cambiar de red no hay que reflashear.

Verificación rápida:

```bash
docker ps --filter name=ruview-sensing
curl -s http://127.0.0.1:3000/health
```

## Flashear el ESP32-S3

No borres toda la flash: el procedimiento de abajo no toca la partición NVS,
que contiene la configuración Wi‑Fi y del nodo.

### 1. Preparar las herramientas

Una sola vez, desde la raíz:

```bash
python -m venv .venv-ruview
.venv-ruview/bin/python -m pip install --upgrade pip esptool pyserial
```

### 2. Encontrar el puerto y comprobar el chip

```bash
ls -l /dev/serial/by-id/* /dev/ttyACM* /dev/ttyUSB* 2>/dev/null
```

Usa el puerto estable de `/dev/serial/by-id/` cuando exista. Sustituye
`/dev/ttyUSB0` por el puerto real en estos comandos:

```bash
PORT=/dev/ttyUSB0
.venv-ruview/bin/python -m esptool --port "$PORT" chip_id
.venv-ruview/bin/python -m esptool --port "$PORT" flash_id
```

El chip debe ser `ESP32-S3`. Comprueba también el tamaño de flash: la imagen
`s3-adr110` de este repositorio es para 8 MB.

### 3. Flashear la imagen S3 de 8 MB

En esta instalación la placa es un ESP32-S3 sin pantalla y se validó con el
overlay `devkitc`. Ese overlay evita que una detección de pantalla incorrecta
deje el CSI en `0pps`. Si vas a regenerar el firmware, compílalo así desde la
raíz del repositorio:

```bash
MSYS_NO_PATHCONV=1 docker run --rm \
  -v "$PWD/firmware/esp32-csi-node:/project" \
  -w /project \
  espressif/idf:v5.4 bash -c \
  "rm -rf build sdkconfig && \
   idf.py -DSDKCONFIG_DEFAULTS='sdkconfig.defaults;sdkconfig.defaults.devkitc' set-target esp32s3 && \
   idf.py -DSDKCONFIG_DEFAULTS='sdkconfig.defaults;sdkconfig.defaults.devkitc' build"
```

Después usa los artefactos de `firmware/esp32-csi-node/build/`:

```bash
.venv-ruview/bin/python -m esptool --chip esp32s3 --port "$PORT" --baud 460800 \
  --before default_reset --after hard_reset write_flash \
  --flash-mode dio --flash-size 8MB --flash-freq 80m \
  0x0 firmware/esp32-csi-node/build/bootloader/bootloader.bin \
  0x8000 firmware/esp32-csi-node/build/partition_table/partition-table.bin \
  0xf000 firmware/esp32-csi-node/build/ota_data_initial.bin \
  0x20000 firmware/esp32-csi-node/build/esp32-csi-node.bin
```

Este flasheo no escribe NVS, por lo que conserva la Wi‑Fi provisionada. La
imagen `release_bins/s3-adr110` puede servir para otra variante compatible,
pero en esta placa produjo `0pps` aunque el Wi‑Fi estuviera conectado.

Como alternativa para otra placa compatible, y solo si `flash_id` confirma
ESP32-S3 y 8 MB, puede usarse la imagen precompilada:

```bash
.venv-ruview/bin/python -m esptool --chip esp32s3 --port "$PORT" --baud 460800 \
  write_flash --flash_mode dio --flash_size 8MB \
  0x0 firmware/esp32-csi-node/release_bins/s3-adr110/bootloader.bin \
  0x8000 firmware/esp32-csi-node/release_bins/s3-adr110/partition-table.bin \
  0xf000 firmware/esp32-csi-node/release_bins/ota_data_initial.bin \
  0x20000 firmware/esp32-csi-node/release_bins/s3-adr110/esp32-csi-node.bin
```

No uses esta imagen en esta placa validada si el objetivo es obtener CSI: usa
la variante `devkitc` anterior. Tampoco la uses si `flash_id` indica 4 MB; en
ese caso hay que elegir la variante S3 de 4 MB y su tabla de particiones
correspondiente, sin improvisar offsets.

Como alternativa a copiar los comandos manualmente, desde la raíz del
repositorio puedes usar los ayudantes de esta carpeta:

```bash
guia/build-s3-firmware.sh 8mb --clean
guia/flash-s3-8mb.sh /dev/serial/by-id/<PLACA> --confirm
```

Para una N16/N16R8 usa `16mb` en los dos scripts. El segundo script siempre
ejecuta `chip_id` y `flash_id`; detente si el tamaño detectado no coincide.

### 4. Ver el arranque real

```bash
.venv-ruview/bin/python -m serial.tools.miniterm "$PORT" 115200
```

Debe aparecer un arranque del ESP32-S3 y después mensajes de Wi‑Fi/CSI. Sal de
`miniterm` con `Ctrl+]`. Un build correcto no sustituye este log: la validación
del hardware requiere que el silicio arranque y transmita.

## Provisionar Wi‑Fi y el destino CSI

El destino es la IP LAN de este ordenador, no `127.0.0.1`, porque el ESP32 le
envía los paquetes por la red. Averigua la IP LAN con:

```bash
hostname -I
```

En el primer provisionado necesitas el SSID y la contraseña de la red. No los
guardes en este archivo ni los pegues en logs. Ejemplo con valores sustituidos
en una terminal privada:

La ESP32-S3 solo usa Wi‑Fi de 2,4 GHz. Si el router publica nombres separados,
elige el SSID de 2,4 GHz; una red de 5 GHz puede dejar el nodo en
`WiFi disconnected, reason=201` aunque la contraseña sea correcta.

```bash
.venv-ruview/bin/python firmware/esp32-csi-node/provision.py \
  --port "$PORT" \
  --chip esp32s3 \
  --ssid '<SSID_DE_TU_RED>' \
  --password '<CONTRASEÑA_DE_TU_RED>' \
  --target-ip '<IP_LAN_DE_ESTE_PC>' \
  --target-port 5005 \
  --node-id 1
```

También puedes usar el ayudante, que solicita la contraseña de forma
interactiva y no la escribe en `guia/`:

```bash
guia/provision-wifi.sh /dev/serial/by-id/<PLACA> '<SSID_DE_TU_RED>' \
  '<IP_LAN_DE_ESTE_PC>' 1
```

Para cambiar de red, repite exactamente el mismo comando con el nuevo SSID y
la nueva IP del PC. La contraseña nueva sustituye a la anterior. El ayudante
escribe únicamente NVS en `0x9000`; no vuelve a grabar bootloader, tabla de
particiones ni aplicación. Por tanto, cambiar de Wi‑Fi no requiere flashear el
firmware completo:

```bash
guia/provision-wifi.sh /dev/serial/by-id/<PLACA> '<NUEVA_WIFI>' \
  '<IP_ACTUAL_DEL_PC>' 1
```

`provision.py` mantiene además un pequeño estado por puerto serie en la
configuración del usuario para completar valores omitidos. Los valores que
escribas explícitamente siempre ganan. Si vas a reutilizar una placa y quieres
eliminar ese estado local antiguo antes de guardar el nuevo:

```bash
guia/provision-wifi.sh /dev/serial/by-id/<PLACA> '<NUEVA_WIFI>' \
  '<IP_ACTUAL_DEL_PC>' 1 --reset-state
```

`--reset-state` borra solo la caché local de provisionado de ese puerto antes
de crear la nueva NVS; no borra la aplicación ni los modelos del servidor.
Nunca pongas una contraseña real en un script, en esta guía o en el historial
de comandos. El helper la pide de forma oculta.

Si el nodo ya tenía Wi‑Fi configurado, el script conserva el estado previo al
no proporcionar credenciales nuevas. Reinicia el ESP32 después de provisionar.

## Usar el ESP32 con RuView

Recrea el contenedor para activar la recepción UDP. Sustituye
`<IP_DEL_ESP32>` por la IP que tenga el nodo en tu router:

```bash
docker rm -f ruview-sensing 2>/dev/null || true
docker volume create ruview-data 2>/dev/null || true
docker run -d \
  --name ruview-sensing \
  --restart unless-stopped \
  --security-opt=no-new-privileges:true \
  --cap-drop=ALL \
  --memory=1g \
  --cpus=2 \
  -e CSI_SOURCE=esp32 \
  -e RUVIEW_UDP_BIND=0.0.0.0 \
  -e RUVIEW_UDP_ALLOW='<IP_DEL_ESP32>' \
  -e RUVIEW_ALLOW_UNAUTHENTICATED=1 \
  -p 127.0.0.1:3000:3000 \
  -p 127.0.0.1:3001:3001 \
  -p 5005:5005/udp \
  -v ruview-data:/app/data \
  ruview-local:latest
```

La API sigue limitada a este ordenador. El puerto UDP 5005 sí entra desde la
LAN y queda limitado a la IP del nodo mediante `RUVIEW_UDP_ALLOW`; no lo abras
hacia Internet.

Comprueba la fuente y el tráfico:

```bash
curl -s http://127.0.0.1:3000/health
docker logs -f --tail 100 ruview-sensing
```

La fuente debe indicar `esp32` y los logs deben mostrar recepción de CSI. Una
interfaz que carga solo demuestra que el servidor está vivo, no que el nodo
esté transmitiendo.

### Interpretar una habitación vacía después de calibrar

Después de una calibración fresca pueden aparecer dos capas distintas en la
respuesta de `/api/v1/sensing/latest`:

- `calibrated_presence_evidence.person_count` y `.presence`: resultado ligado
  al modelo de habitación recién calibrado; es el valor que se debe usar para
  validar una habitación vacía.
- `classification`, `estimated_persons` y `persons`: salida pública para la
  interfaz. Con el servidor corregido queda gobernada por la evidencia
  calibrada: en una habitación vacía debe ser `presence: false`, `0` y `null`.
- `room_inference` y algunos campos de `/api/v1/nodes`: diagnóstico por nodo de
  la heurística de actividad. Puede seguir reflejando actividad RF del
  teléfono/router y no debe usarse para afirmar que hay una persona.

Revísalo así:

```bash
curl -s http://127.0.0.1:3000/api/v1/sensing/latest | \
  python -c 'import json,sys; x=json.load(sys.stdin); print(json.dumps({
    "legacy": {"estimated_persons": x.get("estimated_persons"), "classification": x.get("classification")},
    "calibrated": x.get("calibrated_presence_evidence")
  }, indent=2))'
```

En la instalación validada, la habitación vacía dio evidencia calibrada
`presence=false, person_count=0`; la salida pública de Observatory quedó en
`motion_level=absent`, sin personas y sin signos vitales. La etiqueta
`Through-wall` pertenece al escenario sintético `search_rescue`: si aparece
junto a `DEMO`, describe una animación de demostración y no una medición del
ESP32. Para hardware real el indicador debe mostrar `LIVE`.

Después de reiniciar o recrear el contenedor hay que repetir la calibración de
la habitación vacía: el modelo de campo se mantiene en memoria durante ese
proceso y no se considera persistido hasta que el servidor lo indique.

## Cachés, modelos y cambios de habitación

No mezcles estas tres cosas:

| Elemento | Dónde vive | Qué hacer al cambiar de Wi‑Fi o ubicación |
|---|---|---|
| Credenciales y destino UDP | NVS de la ESP32, escrito por `provision.py` | Actualizar con `provision-wifi.sh`; no reflashear firmware |
| Modelos entrenados `.rvf` | Volumen Docker `ruview-data`, en `/app/data/models` | Conservar, cargar, descargar o borrar según el espacio/escenario |
| Calibración de habitación | Estado activo del servidor | Repetir al mover ESP32/AP o reiniciar el servidor |

### Guardar y seleccionar modelos entrenados

Los modelos `.rvf` no son la calibración rápida de una habitación. Son
artefactos de entrenamiento y permanecen en `ruview-data` aunque se reinicie
el contenedor. Antes de cambiar una instalación puedes guardar una copia del
volumen completo:

```bash
mkdir -p backups
docker run --rm -v ruview-data:/data -v "$PWD/backups:/backup" \
  alpine tar -czf /backup/ruview-data-$(date +%Y%m%d-%H%M%S).tgz -C /data .
```

Lista los modelos disponibles y el modelo activo:

```bash
curl -fsS http://127.0.0.1:3000/api/v1/models | jq
curl -fsS http://127.0.0.1:3000/api/v1/models/active | jq
```

Selecciona uno para inferencia o descárgalo de memoria sin borrarlo:

```bash
curl -fsS -X POST http://127.0.0.1:3000/api/v1/models/load \
  -H 'content-type: application/json' \
  --data '{"model_id":"<ID_DEL_MODELO>"}'

curl -fsS -X POST http://127.0.0.1:3000/api/v1/models/unload \
  -H 'content-type: application/json' --data '{}'
```

Elimina un modelo solo cuando estés seguro de que la copia de seguridad es
válida:

```bash
curl -fsS -X DELETE \
  'http://127.0.0.1:3000/api/v1/models/<ID_DEL_MODELO>'
```

### Grabaciones usadas para entrenamiento

Las capturas CSI se guardan en `/app/data/recordings` dentro del mismo volumen.
Se pueden conservar como datasets, listar y eliminar por sesión:

```bash
curl -fsS http://127.0.0.1:3000/api/v1/recording/list | jq
curl -fsS -X DELETE \
  'http://127.0.0.1:3000/api/v1/recording/<ID_DE_SESION>'
```

No borres `ruview-data` para cambiar de Wi‑Fi: eso eliminaría modelos y
grabaciones guardados. Borra solo un modelo o una grabación identificados.

### Calibración por ubicación

La calibración de campo actual no se selecciona como un modelo `.rvf`: queda
activa en memoria y está ligada al conjunto de nodos, la rejilla CSI y la
posición de la habitación. Al mover la ESP32, mover el AP/móvil o cambiar
mucho la geometría, descarta la calibración activa con `/api/v1/calibration/reset`
y ejecuta la sección [Recalibrar una habitación vacía](#recalibrar-una-habitación-vacía).
Guarda el nombre de la ubicación y el `binding_digest` devuelto por `start`
en tus notas; si el servidor se reinicia, actualmente hay que volver a
capturar la referencia vacía en vez de seleccionar una calibración antigua.

## Cambiar la ESP32 de habitación o de posición

Hay tres casos distintos:

1. **Misma Wi‑Fi, nueva posición o habitación:** no flashees ni reprovisiones
   la placa. Colócala junto al router/AP en la posición definitiva, deja la
   habitación vacía y repite la calibración de abajo.
2. **Nueva Wi‑Fi o nuevo hotspot:** vuelve a ejecutar `provision.py` con el
   SSID, contraseña y la IP LAN actual del PC. Después reinicia la ESP32 y
   actualiza `RUVIEW_UDP_ALLOW` si la IP del nodo ha cambiado. No hace falta
   borrar la flash completa.
3. **Añadir más nodos:** asigna un `node-id` distinto a cada placa y calibra
   de nuevo con todos los nodos que vayan a cubrir esa habitación.

### Recalibrar una habitación vacía

Hazlo siempre después de mover la placa, el router/AP o cambiar mucho la
orientación. Espera al menos 10 segundos para que el servidor vea el CSI y
mantén la habitación vacía durante unos 10 minutos.

Desde la web, abre `http://127.0.0.1:3000/ui/index.html`, entra en **Sensing**
y usa el panel **ROOM CALIBRATION**. Escribe los `node-id` separados por
comas, pulsa **START EMPTY CAPTURE**, espera a que el contador alcance al menos
10 minutos y 1000 frames, y pulsa **FINALIZE CALIBRATION**. La habitación debe
permanecer vacía durante toda la captura. Si cambias la posición, pulsa
**RESET CALIBRATION** y repite el proceso; no reutilices una calibración de otra
habitación o de otro conjunto de nodos.

Con varios nodos, pulsa **Use all live nodes** para rellenar automáticamente
todos los IDs que estén transmitiendo y después inicia una única captura
conjunta. La finalización no se permite mientras falte CSI de cualquiera de
los nodos seleccionados.

Si el panel no aparece, abre la URL principal anterior (no
`observatory.html`) y fuerza una recarga con `Ctrl+Shift+R`. La aplicación
actualiza automáticamente la caché del navegador cuando cambia la UI.

La misma operación por API, útil para diagnóstico, es:

```bash
# Usa un identificador estable para esta habitación y este conjunto de nodos.
DIGEST=$(printf 'ruview-room-nodes-1' | sha256sum | awk '{print $1}')

curl -fsS -X POST \
  'http://127.0.0.1:3000/api/v1/calibration/start?source_node_id=1' \
  -H 'content-type: application/json' \
  --data "{\"binding_digest\":\"$DIGEST\",\"source_node_ids\":[1]}"

# Debe llegar a elapsed_s >= 600 y frame_count >= 1000.
watch -n 5 'curl -fsS http://127.0.0.1:3000/api/v1/calibration/status | \
  jq "{status,elapsed_s,frame_count,frames_per_second,missing_source_node_ids,sequence_fault_node_ids}"'
```

Cuando se cumplan ambos límites, usa en el siguiente comando los valores
`boot_epoch`, `session_id`, `binding_digest` y `source_node_ids` devueltos por
`start`:

```bash
curl -fsS -X POST http://127.0.0.1:3000/api/v1/calibration/stop \
  -H 'content-type: application/json' \
  --data '{
    "boot_epoch": "<BOOT_EPOCH_DEVUELTO>",
    "session_id": "<SESSION_ID_DEVUELTO>",
    "binding_digest": "<DIGEST_DEVUELTO>",
    "source_node_ids": [1]
  }'
```

Comprueba después que el estado es `fresh` y que, con la habitación todavía
vacía, Observatory muestra `LIVE`, `ABSENT` y cero personas. Si solo aparece
`DEMO`, recarga la página y revisa la conexión al WebSocket.

## Añadir 2–3 ESP32-S3 N16/N16R8

En estas placas, `N16` suele indicar **16 MB de flash** y `R8` **8 MB de
PSRAM**. No lo des por supuesto: cada unidad debe identificarse por USB antes
de flashearla. El tamaño que importa para el mapa de particiones es el de
flash, no el de PSRAM.

### Preparación de cada nodo

Repite estos pasos con una sola placa conectada cada vez:

```bash
PORT=/dev/ttyACM0                 # sustituir por el puerto que aparezca
.venv-ruview/bin/python -m esptool --port "$PORT" chip_id
.venv-ruview/bin/python -m esptool --port "$PORT" flash_id
```

Debe aparecer `ESP32-S3`. Anota la MAC y el tamaño de flash; la MAC es la
identidad física de la placa y nunca debe reutilizarse como `node-id` de otra.
Para una N16/N16R8 con 16 MB, compila con el overlay de 16 MB además del
overlay `devkitc` de placa sin pantalla:

```bash
MSYS_NO_PATHCONV=1 docker run --rm \
  -v "$PWD/firmware/esp32-csi-node:/project" \
  -w /project \
  espressif/idf:v5.4 bash -c \
  "rm -rf build sdkconfig && \
   idf.py -DSDKCONFIG_DEFAULTS='sdkconfig.defaults;sdkconfig.defaults.16mb;sdkconfig.defaults.devkitc' set-target esp32s3 && \
   idf.py -DSDKCONFIG_DEFAULTS='sdkconfig.defaults;sdkconfig.defaults.16mb;sdkconfig.defaults.devkitc' build"
```

Antes de flashear, confirma que el `flash_id` dice 16 MB y que la compilación
seleccionó `CONFIG_ESPTOOLPY_FLASHSIZE="16MB"` y
`CONFIG_PARTITION_TABLE_CUSTOM_FILENAME="partitions_16mb.csv"`:

```bash
rg '^CONFIG_(IDF_TARGET|ESPTOOLPY_FLASHSIZE|PARTITION_TABLE_CUSTOM_FILENAME)=' \
  firmware/esp32-csi-node/sdkconfig
```

Usa los cuatro offsets habituales, siempre con los artefactos recién
compilados:

```bash
.venv-ruview/bin/python -m esptool --chip esp32s3 --port "$PORT" --baud 460800 \
  --before default_reset --after hard_reset write_flash \
  --flash-mode dio --flash-size 16MB --flash-freq 80m \
  0x0 firmware/esp32-csi-node/build/bootloader/bootloader.bin \
  0x8000 firmware/esp32-csi-node/build/partition_table/partition-table.bin \
  0xf000 firmware/esp32-csi-node/build/ota_data_initial.bin \
  0x20000 firmware/esp32-csi-node/build/esp32-csi-node.bin
```

Si `flash_id` dice 8 MB, usa el build y el comando de 8 MB de la sección
anterior. Nunca mezcles una tabla de 8 MB o 16 MB por intuición: el mapa de
particiones debe coincidir con el tamaño detectado. Estos flasheos no escriben
NVS (`0x9000`), por lo que no sustituyen la provisionación de Wi‑Fi.

### IDs, Wi‑Fi y servidor compartido

Los nodos pueden usar la misma red Wi‑Fi y el mismo destino (`IP_LAN_DEL_PC`,
puerto UDP `5005`), pero cada uno necesita un `node-id` distinto. Como el nodo
antiguo se va a dedicar a otra cosa, para empezar de cero con tres placas
nuevas usa esta asignación:

| Placa | `node-id` | Ubicación sugerida | IP que hay que anotar |
|---|---:|---|---|
| Nueva 1 | 1 | primera zona | DHCP/reserva del router |
| Nueva 2 | 2 | segunda zona | DHCP/reserva del router |
| Nueva 3 | 3 | tercera zona | DHCP/reserva del router |

Provisiona cada placa con su propio `node-id`, manteniendo las credenciales
fuera de esta guía y de los logs:

```bash
.venv-ruview/bin/python firmware/esp32-csi-node/provision.py \
  --port "$PORT" --chip esp32s3 \
  --ssid '<SSID_DE_TU_RED>' --password '<CONTRASEÑA_DE_TU_RED>' \
  --target-ip '<IP_LAN_DEL_PC>' --target-port 5005 --node-id 2
```

Para los nodos siguientes cambia solo `--port` y `--node-id`; no repitas un
ID. Después de arrancar cada placa, localiza su IP por la tabla DHCP/ARP del
router y comprueba su MAC. El servidor es único: no se levanta un contenedor
por placa.

Recrea el contenedor con todas las IP de los nodos separadas por comas:

```bash
docker stop ruview-sensing
docker rm ruview-sensing
docker run -d \
  --name ruview-sensing --restart unless-stopped \
  -e CSI_SOURCE=esp32 -e RUVIEW_UDP_BIND=0.0.0.0 \
  -e RUVIEW_UDP_ALLOW='<IP_NODO_1>,<IP_NODO_2>,<IP_NODO_3>' \
  -e RUVIEW_ALLOW_UNAUTHENTICATED=1 \
  -p 127.0.0.1:3000:3000 -p 127.0.0.1:3001:3001 \
  -p 0.0.0.0:5005:5005/udp -v ruview-data:/app/data \
  ruview-local:latest
```

Si solo hay dos placas, elimina la entrada sobrante. Reserva sus
IPs en el router si es posible; si DHCP cambia una IP, el nodo seguirá
transmitiendo pero el servidor lo bloqueará hasta actualizar `RUVIEW_UDP_ALLOW`.

### Arranque limpio para los tres nodos nuevos

El servidor queda apagado al terminar una sesión. Mañana, antes de arrancar
los nodos nuevos, hay dos opciones:

- **Conservar modelos/grabaciones:** mantén el volumen `ruview-data` y elimina
  solo la calibración activa con `/api/v1/calibration/reset` cuando el servidor
  vuelva a estar encendido.
- **Empezar completamente de cero:** haz primero una copia de seguridad y,
  con el contenedor parado, elimina el volumen explícito. Esto borra modelos,
  grabaciones, secretos de sesión y cualquier bootstrap guardado:

  ```bash
  mkdir -p backups
  docker run --rm -v ruview-data:/data -v "$PWD/backups:/backup" \
    alpine tar -czf /backup/ruview-data-before-new-nodes.tgz -C /data .
  docker rm -f ruview-sensing 2>/dev/null || true
  docker volume rm ruview-data
  ```

No ejecutes `docker volume rm` si quieres conservar los modelos o las
grabaciones del nodo antiguo. El nodo antiguo puede seguir provisionado para
otro uso: simplemente no incluyas su IP en `RUVIEW_UDP_ALLOW`.

### Validar que están acoplados de verdad

Por cada placa, el monitor serie debe mostrar `yield` estable por encima de
20 pps, callbacks `CSI cb` y su `node`/`node_id`. En el servidor:

```bash
curl -s http://127.0.0.1:3000/health
docker logs --since 60s ruview-sensing | rg \
  'Data source|simulator=false|Dropped UDP|UDP listening|HEALTH|node'
```

Lo correcto es `source=esp32`, `simulator=false` y un `tick` que siga
aumentando. Un `yield=0pps`, `sendto failed`, `watchdog`, reinicios repetidos o
`Dropped UDP frame from disallowed source` indican un problema distinto; no se
deben tapar activando el simulador. La luz naranja solo indica alimentación y
no demuestra que haya CSI.

### Calibración conjunta de tres nodos

Después de comprobar que los tres nodos están activos, deja vacía la zona que
quieras monitorizar durante unos 10 minutos. Espera al menos 10 segundos para
que los tres nodos tengan evidencia CSI y comienza una única calibración con
los tres `node-id`:

```bash
DIGEST=$(printf 'ruview-new-room-nodes-1-2-3' | sha256sum | awk '{print $1}')

curl -fsS -X POST http://127.0.0.1:3000/api/v1/calibration/start \
  -H 'content-type: application/json' \
  --data "{\"binding_digest\":\"$DIGEST\",\"source_node_ids\":[1,2,3]}"

watch -n 5 'curl -fsS http://127.0.0.1:3000/api/v1/calibration/status | \
  jq "{status,elapsed_s,frame_count,frames_per_second,missing_source_node_ids,sequence_fault_node_ids}"'
```

No finalices si `missing_source_node_ids` contiene algún nodo. Cuando
`elapsed_s >= 600`, `frame_count >= 1000`, la lista de nodos ausentes está vacía
y no hay fallos de secuencia, finaliza usando los cuatro valores (`boot_epoch`,
`session_id`, `binding_digest` y `source_node_ids`) que devolvió `start`:

```bash
curl -fsS -X POST http://127.0.0.1:3000/api/v1/calibration/stop \
  -H 'content-type: application/json' \
  --data '{
    "boot_epoch": "<BOOT_EPOCH_DEVUELTO>",
    "session_id": "<SESSION_ID_DEVUELTO>",
    "binding_digest": "<DIGEST_DEVUELTO>",
    "source_node_ids": [1, 2, 3]
  }'
```

La calibración queda ligada a esos tres nodos y a esa geometría. Si cambias un
nodo de habitación, cambias el AP de sitio o reinicias el servidor, repite la
captura vacía; no cargues una calibración antigua como si fuera un modelo `.rvf`.

## Monitorización de RuView

Mientras se prueba una instalación nueva, deja una terminal con:

```bash
watch -n 5 'curl -fsS http://127.0.0.1:3000/health'
```

Y otra con:

```bash
docker stats ruview-sensing
docker logs -f --tail 100 ruview-sensing
```

Durante la primera prueba de varios nodos conviene observar al menos 10–15
minutos. Apunta hora, `node-id`, IP, `yield`, RSSI, `tick`, reinicios y errores
UDP. Primero prueba cada nodo solo; después enciéndelos juntos. Así se separa
un problema de Wi‑Fi/flash de un problema de interferencia o de configuración
multinodo. La métrica de paquetes confirma transporte real, pero no constituye
por sí sola una validación clínica de presencia, respiración, pulso o pose.

## Problemas habituales

- **No aparece ningún puerto serie / `error -71`:** desconecta la placa,
  prueba otro cable USB de datos y otro puerto USB directo del PC; evita hubs.
  Si la placa tiene botones `BOOT` y `RESET`, mantén `BOOT`, pulsa y suelta
  `RESET`, suelta `BOOT` y vuelve a listar los puertos.
- **`chip_id` falla:** cierra cualquier monitor serie, usa 115200 para el
  monitor y deja que `esptool` gestione el reset; confirma que el cable admite
  datos.
- **El servidor arranca pero no hay datos:** comprueba que el ESP32 y el PC
  están en la misma LAN, que `--target-ip` era la IP LAN correcta y que no
  cambió la IP del nodo. Revisa `docker logs` y el firewall UDP 5005.
- **Puerto 3000 ocupado:** identifica el proceso antes de detenerlo:

  ```bash
  docker ps -a
  ss -lntup | grep -E ':(3000|3001|5005)\b'
  ```

  El contenedor anterior `ruview-sensing` se puede detener y recrear con los
  comandos de esta guía; no borres el volumen de datos sin revisarlo.
