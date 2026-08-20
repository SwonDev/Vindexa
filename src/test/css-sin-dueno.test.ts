import { readdirSync, readFileSync, statSync } from "node:fs";
import { join } from "node:path";
import { describe, expect, it } from "vitest";

/**
 * Ninguna hoja viste un elemento que no existe.
 *
 * # Por qué se vigila
 *
 * Una regla escrita para un elemento que nadie pinta es de las dos cosas, y
 * ninguna buena:
 *
 * - **Código muerto**: dice que existe algo que no existe, y quien lo lee sale
 *   a buscar un elemento que nunca aparece.
 * - **Un fallo callado**: el elemento se renombró o se sustituyó y el estilo se
 *   quedó atrás. Pasó con la evidencia del DRM: había una regla para su lista y
 *   la lista había desaparecido dentro de un emergente, con el nombre interno
 *   del campo a la vista. No falló nada; simplemente dejó de verse.
 *
 * # Qué se admite
 *
 * Las clases que se **componen** en tiempo de ejecución no aparecen literales
 * en el código. Son pocas y se enumeran aquí con su motivo: si se añade otra,
 * se añade también aquí, que es donde se ve de un vistazo cuántas hay.
 */
const COMPUESTAS_EN_EJECUCION: Record<string, string> = {
  "artwork--cover": "la compone Artwork.tsx con el tipo de arte",
  "artwork--header": "la compone Artwork.tsx con el tipo de arte",
  "artwork--hero": "la compone Artwork.tsx con el tipo de arte",
  "artwork--icon": "la compone Artwork.tsx con el tipo de arte",
};

/**
 * Esta misma prueba se excluye del corpus.
 *
 * La lista de excepciones nombra las clases, así que sin excluirla toda clase
 * enumerada aquí parecería usada… por esta prueba.
 */
const YO = join("src", "test", "css-sin-dueno.test.ts");

function archivos(directorio: string, extension: string): string[] {
  const salida: string[] = [];
  for (const entrada of readdirSync(directorio)) {
    const ruta = join(directorio, entrada);
    if (statSync(ruta).isDirectory()) salida.push(...archivos(ruta, extension));
    else if (entrada.endsWith(extension) && ruta !== YO) salida.push(ruta);
  }
  return salida;
}

describe("hojas de estilo sin elemento", () => {
  it("no declara ninguna clase que nadie use", () => {
    const codigo = [...archivos("src", ".tsx"), ...archivos("src", ".ts")]
      .map((ruta) => readFileSync(ruta, "utf8"))
      .join("\n");

    const huerfanas = new Map<string, string>();
    for (const hoja of archivos("src", ".css")) {
      const contenido = readFileSync(hoja, "utf8");
      for (const encontrado of contenido.matchAll(/\.([a-zA-Z][a-zA-Z0-9_-]{2,})/g)) {
        const clase = encontrado[1] as string;
        if (clase in COMPUESTAS_EN_EJECUCION) continue;
        if (codigo.includes(clase)) continue;
        huerfanas.set(clase, hoja);
      }
    }

    expect(
      [...huerfanas].map(([clase, hoja]) => `${hoja}: .${clase}`).sort(),
      "o el elemento se perdió por el camino, o la regla sobra",
    ).toEqual([]);
  });

  it("no guarda excepciones que ya no hacen falta", () => {
    // Una excepción que sobrevive a su motivo es una puerta abierta sin puerta.
    const codigo = [...archivos("src", ".tsx"), ...archivos("src", ".ts")]
      .map((ruta) => readFileSync(ruta, "utf8"))
      .join("\n");
    const hojas = archivos("src", ".css")
      .map((ruta) => readFileSync(ruta, "utf8"))
      .join("\n");

    for (const [clase, motivo] of Object.entries(COMPUESTAS_EN_EJECUCION)) {
      expect(hojas, `.${clase} ya no está en ninguna hoja (${motivo})`).toContain(`.${clase}`);
      expect(codigo.includes(clase), `.${clase} ya aparece literal: sobra la excepción`).toBe(
        false,
      );
    }
  });
});
