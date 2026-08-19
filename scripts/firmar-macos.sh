#!/usr/bin/env bash
# Firma el paquete de macOS con una identidad estable.
#
# # Por qué hace falta
#
# Sin certificado, Tauri firma «ad-hoc». Una firma ad-hoc no identifica a nadie,
# así que el llavero de macOS ata el permiso al **hash del binario**: en cuanto
# se recompila, el hash cambia, el sistema ve un programa distinto y vuelve a
# pedir la contraseña del llavero. «Permitir siempre» no aguanta ni una versión.
#
# Con un certificado de desarrollo, el permiso queda atado al certificado y al
# identificador del paquete, que no cambian de una compilación a otra: se
# autoriza una vez y ya está.
#
# # Qué NO hace
#
# No notariza ni habilita la distribución fuera de este equipo: un certificado
# «Apple Development» sirve para desarrollo local. Para publicar sin avisos hace
# falta un «Developer ID Application», que es otra cosa y cuesta dinero.
#
# # Por qué no está en `tauri.conf.json`
#
# Porque la integración continua no tiene este certificado —ni ninguno— y una
# identidad fija en la configuración haría fallar la compilación de cada
# publicación. Aquí se busca, y si no aparece se sigue sin firmar.

set -euo pipefail

paquete="${1:-src-tauri/target/release/bundle/macos/Vindexa.app}"
identificador="io.vindexa.desktop"

if [[ "$(uname -s)" != "Darwin" ]]; then
  echo "Este script sólo tiene sentido en macOS."
  exit 0
fi

if [[ ! -d "$paquete" ]]; then
  echo "No existe el paquete: $paquete" >&2
  exit 1
fi

# Se prefiere «Developer ID» —el que sirve para distribuir— y si no lo hay se
# usa el de desarrollo. Se coge la huella, no el nombre: el nombre lleva el
# correo de la cuenta y no tiene por qué acabar en un registro.
huella="$(security find-identity -v -p codesigning 2>/dev/null \
  | awk '/Developer ID Application/ {print $2; exit}')"
if [[ -z "$huella" ]]; then
  huella="$(security find-identity -v -p codesigning 2>/dev/null \
    | awk '/Apple Development/ {print $2; exit}')"
fi

if [[ -z "$huella" ]]; then
  echo "Sin certificado de firma en este equipo: el paquete se queda con la firma ad-hoc."
  echo "Funciona igual, pero macOS volverá a pedir la contraseña del llavero en cada versión."
  exit 0
fi

codesign --force --sign "$huella" --identifier "$identificador" "$paquete"
echo "Paquete firmado con una identidad estable."
codesign -dv --verbose=2 "$paquete" 2>&1 | grep -E "^Identifier|^TeamIdentifier"
