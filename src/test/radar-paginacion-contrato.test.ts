import { readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";

/**
 * La segunda tanda del radar empieza donde acaba la primera.
 *
 * # Por qué se vigila
 *
 * El panorama trae doce de cada lista y el botón «Ver más» pide desde el doce.
 * Son dos números escritos en dos idiomas distintos —`DISCOVERY_LIST_LIMIT` en
 * Rust, `RADAR_FIRST_PAGE` en TypeScript— y nada obliga a que coincidan. Si en
 * Rust se sube a veinte y aquí no, la segunda tanda repite ocho juegos que ya
 * estaban en pantalla; si baja, se saltan.
 *
 * No es hipotético: el mismo desajuste entre los dos lados ya se coló en los
 * atajos, donde el nombre de un campo cambió en Rust y la interfaz siguió
 * pidiendo el viejo.
 */
describe("contrato de paginación del radar", () => {
  it("la primera tanda mide lo mismo a los dos lados", () => {
    const rust = readFileSync("src-tauri/src/db/discovery.rs", "utf8");
    const limite = rust.match(/const DISCOVERY_LIST_LIMIT: usize = (\d+);/);
    expect(limite?.[1], "DISCOVERY_LIST_LIMIT dejó de existir o cambió de forma").toBeDefined();

    const ts = readFileSync("src/features/discovery/DiscoveryScreen.tsx", "utf8");
    const primera = ts.match(/const RADAR_FIRST_PAGE = (\d+);/);
    expect(primera?.[1], "RADAR_FIRST_PAGE dejó de existir o cambió de forma").toBeDefined();

    expect(
      Number(primera?.[1]),
      "la segunda tanda empezaría en otro sitio: se repetirían o se saltarían juegos",
    ).toBe(Number(limite?.[1]));
  });

  it("las vistas que aceptan más tandas son las que el radar ofrece", () => {
    const rust = readFileSync("src-tauri/src/db/discovery.rs", "utf8");
    // `radar_page` sólo conoce estas tres; pedirle otra es un error de
    // validación, así que la interfaz no puede ofrecer un botón que falle.
    for (const vista of ['"forgotten"', '"almost"', '"upcoming"']) {
      expect(rust, `radar_page dejó de aceptar ${vista}`).toContain(`${vista} => &`);
    }
    const ts = readFileSync("src/features/discovery/DiscoveryScreen.tsx", "utf8");
    expect(ts).toContain('radarView === "forgotten" || radarView === "almost"');
  });
});
