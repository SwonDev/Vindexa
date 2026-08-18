import { describe, expect, it } from "vitest";
import cssFuente from "@/index.css?raw";

/**
 * `contain: size` sobre un contenedor de desplazamiento rompe la biblioteca en
 * WebKit —el motor de la aplicación de escritorio—: el área desplazable deja de
 * contar el lienzo virtual, el listado se queda en una franja bajo la barra y no
 * hay forma de bajar. Chromium no lo reproduce, así que ni las pruebas de
 * navegador ni las de unidad lo detectan: sólo se ve en la aplicación instalada.
 *
 * Este contrato existe para que no vuelva. `layout` y `paint` sí son seguros y
 * siguen aportando el aislamiento que buscaba la regla original.
 */

interface ReglaCss {
  selector: string;
  cuerpo: string;
}

function reglas(css: string): ReglaCss[] {
  // Los comentarios se retiran antes de partir en reglas: una declaración
  // precedida de un comentario no debe escaparse del contrato.
  const limpio = css.replace(/\/\*[\s\S]*?\*\//g, "");
  const encontradas: ReglaCss[] = [];
  const patron = /([^{}]+)\{([^{}]*)\}/g;
  let coincidencia = patron.exec(limpio);
  while (coincidencia) {
    const selector = (coincidencia[1] ?? "").trim();
    const cuerpo = coincidencia[2] ?? "";
    if (selector && !selector.startsWith("@")) encontradas.push({ selector, cuerpo });
    coincidencia = patron.exec(limpio);
  }
  return encontradas;
}

/** Valores de `contain` que implican containment de tamaño. */
function contieneTamano(valor: string): boolean {
  const partes = valor.trim().toLowerCase().split(/\s+/);
  return partes.some((parte) => parte === "strict" || parte === "size" || parte === "inline-size");
}

describe("contención de los contenedores de desplazamiento", () => {
  const todas = reglas(cssFuente);

  it("ninguna regla con desplazamiento propio aplica containment de tamaño", () => {
    const culpables = todas
      .filter((regla) => /overflow(-y)?\s*:\s*(auto|scroll)/.test(regla.cuerpo))
      .filter((regla) => {
        const contain = /(?:^|;)\s*contain\s*:\s*([^;]+)/.exec(regla.cuerpo);
        return contain ? contieneTamano(contain[1] ?? "") : false;
      })
      .map((regla) => regla.selector);

    expect(
      culpables,
      `Reglas con overflow y containment de tamaño: ${culpables.join(", ")}`,
    ).toEqual([]);
  });

  it("la biblioteca conserva el aislamiento que sí es seguro", () => {
    const biblioteca = todas.find((regla) => regla.selector === ".game-browser");
    expect(biblioteca, "la regla .game-browser debe existir").toBeDefined();
    const contain = /(?:^|;)\s*contain\s*:\s*([^;]+)/.exec(biblioteca?.cuerpo ?? "");
    expect(contain, ".game-browser debe declarar `contain`").not.toBeNull();
    const valor = (contain?.[1] ?? "").trim();
    expect(valor).toContain("layout");
    expect(valor).toContain("paint");
    expect(contieneTamano(valor)).toBe(false);
  });
});
