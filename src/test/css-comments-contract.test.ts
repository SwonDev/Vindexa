import { readdirSync, readFileSync } from "node:fs";
import { join } from "node:path";
import { describe, expect, it } from "vitest";

/**
 * Trinquete de comentarios en las hojas de estilo.
 *
 * Un `/*` sin su `*` `/` no rompe la compilación: el navegador se come todo lo
 * que viene detrás hasta el siguiente cierre, así que desaparecen reglas sin
 * un solo error. Pasó al insertar un bloque nuevo en la hoja de Deseados y el
 * síntoma apareció a tres pantallas de distancia —el texto de las tarjetas
 * partiéndose palabra a palabra—, no donde estaba la causa.
 *
 * Biome no lo detecta porque el archivo sigue siendo CSS válido. De ahí esta
 * prueba: cuenta los delimitadores de todas las hojas del proyecto y falla si
 * alguno queda suelto.
 */

/** Recorre `src/` y devuelve la ruta de todas las hojas de estilo. */
function hojasDeEstilo(directorio: string): string[] {
  const encontradas: string[] = [];
  for (const entrada of readdirSync(directorio, { withFileTypes: true })) {
    const ruta = join(directorio, entrada.name);
    if (entrada.isDirectory()) {
      encontradas.push(...hojasDeEstilo(ruta));
    } else if (entrada.name.endsWith(".css")) {
      encontradas.push(ruta);
    }
  }
  return encontradas.sort();
}

const HOJAS = hojasDeEstilo("src");

describe("comentarios de las hojas de estilo", () => {
  it("encuentra las hojas del proyecto", () => {
    // Si el recorrido dejara de encontrarlas, la prueba pasaría en vacío y no
    // protegería nada.
    expect(HOJAS.length).toBeGreaterThanOrEqual(5);
    expect(HOJAS).toContain(join("src", "index.css"));
  });

  it.each(HOJAS)("%s cierra todos sus comentarios", (ruta) => {
    const contenido = readFileSync(ruta, "utf8");

    // Se recorre el archivo en lugar de contar apariciones: `/*` dentro de un
    // comentario ya abierto no abre nada, y contar daría un falso positivo.
    let posicion = 0;
    let abiertoEn: number | undefined;
    while (posicion < contenido.length) {
      if (abiertoEn === undefined) {
        const apertura = contenido.indexOf("/*", posicion);
        if (apertura === -1) break;
        abiertoEn = apertura;
        posicion = apertura + 2;
        continue;
      }
      const cierre = contenido.indexOf("*/", posicion);
      if (cierre === -1) {
        const linea = contenido.slice(0, abiertoEn).split("\n").length;
        expect.fail(`${ruta}:${linea} abre un comentario que nunca se cierra`);
      }
      abiertoEn = undefined;
      posicion = cierre + 2;
    }

    expect(abiertoEn, `${ruta} termina con un comentario abierto`).toBeUndefined();
  });
});
