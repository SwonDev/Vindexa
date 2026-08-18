#!/usr/bin/env bash
# Sube la versión de Vindexa en los tres sitios que la declaran.
#
# La versión vive por triplicado —`package.json`, `src-tauri/Cargo.toml` y
# `src-tauri/tauri.conf.json`— porque cada herramienta lee la suya. Si se
# desincronizan, el instalador dice una cosa y la aplicación otra, así que se
# tocan siempre juntas y desde aquí.
#
# Criterio de versionado del proyecto: se avanza el tercer número hasta
# 0.1.10 y a partir de ahí se pasa a 0.2.0. Es decir 0.1.0 → 0.1.1 → … → 0.1.9
# → 0.1.10 → 0.2.0.
#
# Uso:
#   scripts/version.sh 0.1.1     fija esa versión
#   scripts/version.sh siguiente calcula la siguiente según el criterio
#   scripts/version.sh           muestra la versión actual
set -euo pipefail

cd "$(dirname "$0")/.."

actual="$(node -p "require('./package.json').version")"

if [ $# -eq 0 ]; then
  echo "$actual"
  exit 0
fi

if [ "$1" = "siguiente" ]; then
  IFS=. read -r mayor menor parche <<<"$actual"
  if [ "$parche" -ge 10 ]; then
    nueva="$mayor.$((menor + 1)).0"
  else
    nueva="$mayor.$menor.$((parche + 1))"
  fi
else
  nueva="$1"
fi

if ! [[ "$nueva" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
  echo "error: «$nueva» no es una versión con la forma mayor.menor.parche" >&2
  exit 1
fi

node - "$nueva" <<'JS'
const { readFileSync, writeFileSync } = require('node:fs');
const nueva = process.argv[2];

// Se sustituye sólo la línea de la versión, sin volver a serializar el fichero.
// Un JSON.parse/stringify reformatearía arrays y escaparía los acentos, y el
// diff de un cambio de versión pasaría a tocar medio fichero.
const ficheros = [
  ['package.json', /^(\s*"version":\s*")[^"]+(")/m],
  ['src-tauri/tauri.conf.json', /^(\s*"version":\s*")[^"]+(")/m],
  ['src-tauri/Cargo.toml', /^(version = ")[^"]+(")/m],
];

for (const [ruta, patron] of ficheros) {
  const antes = readFileSync(ruta, 'utf8');
  const despues = antes.replace(patron, `$1${nueva}$2`);
  if (antes === despues) {
    console.error(`error: no se encontró la versión en ${ruta}`);
    process.exit(1);
  }
  writeFileSync(ruta, despues);
}
JS

# Cargo.lock guarda la versión del propio paquete: sin esto el lockfile queda
# desfasado y la compilación con `--locked` falla en integración continua.
cargo update --manifest-path src-tauri/Cargo.toml --package vindexa --quiet 2>/dev/null || true

echo "$actual → $nueva"
echo
echo "Siguiente paso:"
echo "  1. Anota los cambios en CHANGELOG.md bajo [$nueva]."
echo "  2. git commit -am \"chore: versión $nueva\""
echo "  3. git tag v$nueva && git push origin main --follow-tags"
