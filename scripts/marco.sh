#!/usr/bin/env bash
# Marco de ventana para el material visual del repositorio.
#
# El material no puede ser un recorte crudo del navegador: se presenta como
# producto. El marco es sobrio a propósito —esquinas de 12 px, filo de un píxel,
# sombra contenida— porque la identidad de la aplicación es «técnica y casi
# ortogonal» (DESIGN.md) y un marco vistoso competiría con ella.
#
# Dos modos, misma geometría, para que una captura y un vídeo enmarcados encajen
# uno al lado del otro:
#
#   scripts/marco.sh imagen  entrada.png salida.png [ancho]
#   scripts/marco.sh piezas  directorio  [ancho] [alto]
#
# El modo `piezas` deja en el directorio el fondo con su sombra y el filo
# suelto, e imprime `ANCHO ALTO X Y` del hueco de la ventana. Sirve para
# enmarcar vídeo con ffmpeg, que no sabe redondear esquinas por su cuenta.
set -euo pipefail

# Paleta de DESIGN.md. El fondo desciende desde `Background` hacia algo más
# oscuro para dar aire sin introducir un color nuevo.
FONDO_ALTO='#171D25'
FONDO_BAJO='#0B0F14'
FILO='#39434F'
BRILLO='#5CAAC1'

MARGEN_X=80
MARGEN_ARRIBA=64
MARGEN_ABAJO=72
RADIO=12
SOMBRA='60x22+0+14'
# El brillo cian va al 12 %: es el único guiño de color y por encima de ahí
# empezaría a teñir los grises de la aplicación.
BRILLO_MEZCLA=12

# Dibuja el lienzo de fondo con su degradado y el brillo superior.
lienzo() {
  local ancho="$1" alto="$2" destino="$3" temporal="$4"
  magick -size "${ancho}x${alto}" "gradient:${FONDO_ALTO}-${FONDO_BAJO}" "$temporal/degradado.png"
  # Media elipse: se genera al doble de alto y se recorta la mitad superior,
  # para que el brillo nazca detrás de la ventana y no forme un halo centrado.
  magick -size "${ancho}x${alto}" radial-gradient:"${BRILLO}-none" \
    -resize "${ancho}x$((alto * 2))!" -gravity north -crop "${ancho}x${alto}+0+0" +repage \
    "$temporal/brillo.png"
  magick "$temporal/degradado.png" "$temporal/brillo.png" \
    -compose blend -define compose:args="$BRILLO_MEZCLA" -composite "$destino"
}

# Sombra de la ventana en su posición dentro del lienzo.
sombra_en_lienzo() {
  local ancho="$1" alto="$2" x="$3" y="$4" w="$5" h="$6" destino="$7" temporal="$8"
  magick -size "${ancho}x${alto}" xc:none -fill white \
    -draw "roundrectangle $x,$y,$((x + w - 1)),$((y + h - 1)),$RADIO,$RADIO" \
    "$temporal/silueta.png"
  # `-shadow` agranda el lienzo; se recorta a la medida original para que la
  # sombra siga alineada con el hueco.
  magick "$temporal/silueta.png" -background black -shadow "$SOMBRA" \
    -gravity center -background none -extent "${ancho}x${alto}" +repage "$destino"
}

# Filo de un píxel por dentro del borde del hueco.
filo_en_lienzo() {
  local ancho="$1" alto="$2" x="$3" y="$4" w="$5" h="$6" destino="$7"
  magick -size "${ancho}x${alto}" xc:none \
    -stroke "$FILO" -strokewidth 1 -fill none \
    -draw "roundrectangle $((x)).5,$((y)).5,$(bc <<<"$x + $w - 1.5"),$(bc <<<"$y + $h - 1.5"),$RADIO,$RADIO" \
    "$destino"
}

modo="${1:?falta el modo: imagen o piezas}"
shift

case "$modo" in
imagen)
  ENTRADA="${1:?falta la imagen de entrada}"
  SALIDA="${2:?falta la imagen de salida}"
  ANCHO_PEDIDO="${3:-1280}"
  TEMPORAL="$(mktemp -d)"
  trap 'rm -rf "$TEMPORAL"' EXIT

  magick "$ENTRADA" -resize "${ANCHO_PEDIDO}x" "$TEMPORAL/contenido.png"
  # `identify` no cierra con salto de línea, así que se lee campo a campo: un
  # `read` que topa con EOF devuelve 1 y con `set -e` mataría el guion.
  W="$(magick identify -format '%w' "$TEMPORAL/contenido.png")"
  H="$(magick identify -format '%h' "$TEMPORAL/contenido.png")"
  LIENZO_W=$((W + MARGEN_X * 2))
  LIENZO_H=$((H + MARGEN_ARRIBA + MARGEN_ABAJO))

  # La máscara va en un fichero aparte: la variante inline de `magick` deja la
  # máscara en blanco y el resultado sale plano.
  magick -size "${W}x${H}" xc:none -fill white \
    -draw "roundrectangle 0,0,$((W - 1)),$((H - 1)),$RADIO,$RADIO" "$TEMPORAL/mascara.png"
  magick "$TEMPORAL/contenido.png" "$TEMPORAL/mascara.png" \
    -alpha set -compose DstIn -composite "$TEMPORAL/redondeado.png"

  lienzo "$LIENZO_W" "$LIENZO_H" "$TEMPORAL/lienzo.png" "$TEMPORAL"
  sombra_en_lienzo "$LIENZO_W" "$LIENZO_H" "$MARGEN_X" "$MARGEN_ARRIBA" "$W" "$H" \
    "$TEMPORAL/sombra.png" "$TEMPORAL"
  filo_en_lienzo "$LIENZO_W" "$LIENZO_H" "$MARGEN_X" "$MARGEN_ARRIBA" "$W" "$H" \
    "$TEMPORAL/filo.png"

  magick "$TEMPORAL/lienzo.png" \
    "$TEMPORAL/sombra.png" -compose over -composite \
    "$TEMPORAL/redondeado.png" -geometry "+${MARGEN_X}+${MARGEN_ARRIBA}" -compose over -composite \
    "$TEMPORAL/filo.png" -geometry '+0+0' -compose over -composite \
    "$SALIDA"
  magick identify -format 'marco %f · %wx%h\n' "$SALIDA"
  ;;

piezas)
  DESTINO="${1:?falta el directorio de salida}"
  W="${2:-1280}"
  H="${3:-800}"
  mkdir -p "$DESTINO"
  LIENZO_W=$((W + MARGEN_X * 2))
  LIENZO_H=$((H + MARGEN_ARRIBA + MARGEN_ABAJO))

  lienzo "$LIENZO_W" "$LIENZO_H" "$DESTINO/lienzo.png" "$DESTINO"
  sombra_en_lienzo "$LIENZO_W" "$LIENZO_H" "$MARGEN_X" "$MARGEN_ARRIBA" "$W" "$H" \
    "$DESTINO/sombra.png" "$DESTINO"
  magick "$DESTINO/lienzo.png" "$DESTINO/sombra.png" -compose over -composite "$DESTINO/fondo.png"
  filo_en_lienzo "$LIENZO_W" "$LIENZO_H" "$MARGEN_X" "$MARGEN_ARRIBA" "$W" "$H" "$DESTINO/filo.png"
  # Máscara del hueco, a tamaño del contenido: ffmpeg la usa con `alphamerge`.
  magick -size "${W}x${H}" xc:black -fill white \
    -draw "roundrectangle 0,0,$((W - 1)),$((H - 1)),$RADIO,$RADIO" "$DESTINO/mascara.png"
  rm -f "$DESTINO/degradado.png" "$DESTINO/brillo.png" "$DESTINO/silueta.png" "$DESTINO/lienzo.png" "$DESTINO/sombra.png"
  echo "$W $H $MARGEN_X $MARGEN_ARRIBA"
  ;;

*)
  echo "modo desconocido: $modo (usa «imagen» o «piezas»)" >&2
  exit 1
  ;;
esac
