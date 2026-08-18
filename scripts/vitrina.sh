#!/usr/bin/env bash
# Genera el material visual del repositorio: capturas y vídeos de la aplicación
# real, enmarcados y optimizados, en `docs/media/`.
#
# Las capturas salen del escenario `showcase` de las pruebas de extremo a
# extremo: es la aplicación de verdad, con su arte oficial, pero sobre un
# catálogo de muestra. Es deliberado —la biblioteca real de quien desarrolla
# contiene su cuenta y sus juegos, y este repositorio es público— y además hace
# el material reproducible: cualquiera puede regenerarlo y obtener lo mismo.
#
# Uso:
#   scripts/vitrina.sh            genera todo
#   scripts/vitrina.sh imagenes   sólo las capturas
#   scripts/vitrina.sh videos     sólo los vídeos
set -euo pipefail

cd "$(dirname "$0")/.."

RAIZ_IMAGENES="artifacts/showcase"
DESTINO="docs/media"
RESULTADOS="${TMPDIR:-/tmp}/vindexa-playwright-results"
# Los vídeos duran lo que tarda el arte en cargar más la interacción. Sólo
# interesa el final: los últimos segundos son la parte en la que algo se mueve.
COLA_SEGUNDOS=9

mkdir -p "$DESTINO"

# Capturas que se publican, con el nombre final. El resto de la vitrina se
# queda en `artifacts/` para la revisión visual, sin engordar el repositorio.
#
# origen                                    destino
PUBLICABLES=(
  "biblioteca-rejilla-1920x1080.png         biblioteca-rejilla.webp"
  "biblioteca-lista-1440x900.png            biblioteca-lista.webp"
  "biblioteca-ultracompacta-1440x900.png    biblioteca-ultracompacta.webp"
  "biblioteca-agrupada-1440x900.png         biblioteca-agrupada.webp"
  "ficha-elden-ring-1440x900.png            ficha-juego.webp"
  "ficha-prioridad-1440x900.png             ficha-prioridad.webp"
  "ficha-dlc-1440x900.png                   ficha-contenido-adicional.webp"
  "deseados-1440x900.png                    deseados.webp"
  "planificador-1440x900.png                planificador.webp"
  "colecciones-1440x900.png                 colecciones.webp"
  "seguimiento-1440x900.png                 seguimiento.webp"
  "avisos-1440x900.png                      avisos.webp"
  "paleta-busqueda-1440x900.png             paleta-comandos.webp"
)

# Vídeos que se publican. La clave es el fragmento del directorio que Playwright
# crea a partir del nombre de la prueba.
PUBLICABLES_VIDEO=(
  "recorrido-por-la-biblioteca   recorrido-biblioteca.mp4"
  "densidades-y-agrupación       densidades.mp4"
  "ficha-de-un-juego             ficha-juego.mp4"
  "paleta-de-comandos            paleta-comandos.mp4"
)

generar_imagenes() {
  echo "▸ Capturando la vitrina"
  pnpm exec playwright test showcase.spec.ts

  echo "▸ Enmarcando y optimizando"
  local temporal
  temporal="$(mktemp -d)"
  trap 'rm -rf "$temporal"' RETURN

  for fila in "${PUBLICABLES[@]}"; do
    # shellcheck disable=SC2086
    set -- $fila
    local origen="$RAIZ_IMAGENES/$1" destino="$DESTINO/$2"
    if [ ! -f "$origen" ]; then
      echo "  ⚠ falta $origen, se omite" >&2
      continue
    fi
    scripts/marco.sh imagen "$origen" "$temporal/marco.png" 1280 >/dev/null
    # Calidad 88: por debajo se ven artefactos en el texto de 11 px de los
    # metadatos, que es justo lo que hay que poder leer en una captura.
    cwebp -quiet -q 88 -m 6 "$temporal/marco.png" -o "$destino"
    printf '  %-34s %s\n' "$2" "$(du -h "$destino" | cut -f1)"
  done
}

generar_videos() {
  echo "▸ Grabando los vídeos"
  pnpm exec playwright test showcase-video.spec.ts

  echo "▸ Enmarcando y convirtiendo"
  local piezas
  piezas="$(mktemp -d)"
  trap 'rm -rf "$piezas"' RETURN
  scripts/marco.sh piezas "$piezas" 1280 800 >/dev/null

  for fila in "${PUBLICABLES_VIDEO[@]}"; do
    # shellcheck disable=SC2086
    set -- $fila
    local clave="$1" destino="$DESTINO/$2"
    local origen
    origen="$(find "$RESULTADOS" -type d -name "*${clave}*" -exec find {} -name '*.webm' \; 2>/dev/null | head -1)"
    if [ -z "$origen" ]; then
      echo "  ⚠ no se encontró el vídeo de «$clave», se omite" >&2
      continue
    fi
    # `-sseof` recorta desde el final: el principio es el arte cargando.
    # `+faststart` mueve el índice al principio para que GitHub pueda empezar a
    # reproducir sin descargar el fichero entero.
    ffmpeg -y -v error \
      -loop 1 -i "$piezas/fondo.png" \
      -sseof "-$COLA_SEGUNDOS" -i "$origen" \
      -i "$piezas/mascara.png" \
      -loop 1 -i "$piezas/filo.png" \
      -filter_complex "\
[1:v]scale=1280:800,fps=30,format=rgba[vid];\
[2:v]format=gray[msk];\
[vid][msk]alphamerge[vr];\
[0:v][vr]overlay=80:64:shortest=1[base];\
[base][3:v]overlay=0:0:shortest=1,format=yuv420p[out]" \
      -map "[out]" -c:v libx264 -preset slow -crf 26 -movflags +faststart -an \
      "$destino"
    # Cartel del vídeo: GitHub lo pinta mientras no se pulsa reproducir, y si
    # `<video>` no llegara a renderizarse queda como enlace visible.
    local cartel="${destino%.mp4}-cartel.webp"
    ffmpeg -y -v error -ss 4 -i "$destino" -frames:v 1 -f image2 "$piezas/cartel.png"
    cwebp -quiet -q 82 -m 6 "$piezas/cartel.png" -o "$cartel"
    printf '  %-34s %s (+ cartel %s)\n' "$2" "$(du -h "$destino" | cut -f1)" \
      "$(du -h "$cartel" | cut -f1)"
  done
}

case "${1:-todo}" in
imagenes) generar_imagenes ;;
videos) generar_videos ;;
todo)
  generar_imagenes
  generar_videos
  ;;
*)
  echo "uso: $0 [todo|imagenes|videos]" >&2
  exit 1
  ;;
esac

echo
echo "▸ Total de docs/media: $(du -sh "$DESTINO" | cut -f1)"
