import { readdirSync, readFileSync, statSync } from "node:fs";
import { join } from "node:path";
import { describe, expect, it } from "vitest";

/**
 * Los componentes de Radix marcan su estado con `data-state`, no con
 * `data-active`.
 *
 * # El fallo
 *
 * El planificador vestía su pestaña activa con
 * `[data-slot="tabs-trigger"][data-active]`. Radix nunca pone ese atributo, así
 * que la regla no pintaba nada y mandaba la utilidad de la librería, que dibuja
 * el subrayado cinco píxeles **por debajo** del disparador: quedaba flotando
 * dentro del contenido, blanco y separado de la pestaña que marcaba. No falló
 * nada; simplemente se veía mal, y el CSS parecía decir lo contrario.
 *
 * # Qué vigila
 *
 * Que ninguna hoja vista un `data-slot` de Radix por un estado que Radix no
 * emite. Los `data-active` de los componentes **propios** de Vindexa no entran
 * aquí: ésos los pone la propia aplicación y sí existen.
 */
const SLOTS_DE_RADIX = [
  "tabs-trigger",
  "tabs-list",
  "select-item",
  "accordion-item",
  "radio-group-item",
  "toggle-group-item",
];

function hojas(directorio: string): string[] {
  const salida: string[] = [];
  for (const entrada of readdirSync(directorio)) {
    const ruta = join(directorio, entrada);
    if (statSync(ruta).isDirectory()) salida.push(...hojas(ruta));
    else if (entrada.endsWith(".css")) salida.push(ruta);
  }
  return salida;
}

describe("estado de los componentes de Radix en CSS", () => {
  it("ninguna hoja viste un slot de Radix con un atributo que Radix no pone", () => {
    const infractores: string[] = [];
    for (const hoja of hojas("src")) {
      const contenido = readFileSync(hoja, "utf8");
      for (const slot of SLOTS_DE_RADIX) {
        const patron = new RegExp(`\\[data-slot="${slot}"\\][^{,]*\\[data-active[\\]=]`, "g");
        for (const encontrado of contenido.match(patron) ?? []) {
          infractores.push(`${hoja}: ${encontrado.trim()}`);
        }
      }
    }
    expect(infractores, "Radix marca el estado con `data-state`").toEqual([]);
  });
});
