import { describe, expect, it } from "vitest";
import { describeFamilyStatus, describeFamilySync } from "@/features/settings/family-session";

/**
 * Reglas de honestidad del catálogo de Familia.
 *
 * Lo que se comprueba aquí no es la redacción, es qué se puede afirmar: nunca
 * dar por buena una cifra cuando el último intento ha fallado, nunca esconder
 * lo que no se ha podido importar, y nunca presentar «no tienes Familia» como
 * un error.
 */

describe("estado del vínculo con la sesión de Steam", () => {
  it("mientras se comprueba no afirma nada", () => {
    expect(describeFamilyStatus()).toContain("Comprobando");
  });

  it("sin vínculo explica por qué faltan juegos", () => {
    const frase = describeFamilyStatus({ linked: false });
    expect(frase).toContain("Sin sesión vinculada");
    expect(frase).toContain("biblioteca pública");
  });

  it("vinculado y sin lecturas todavía no inventa un recuento", () => {
    const frase = describeFamilyStatus({ linked: true });
    expect(frase).toContain("Todavía no se ha traído el catálogo");
    expect(frase).not.toMatch(/\d/);
  });

  it("un fallo reciente manda sobre el recuento anterior", () => {
    // Decir «última lectura: 3.000 juegos» cuando el último intento acaba de
    // fallar da por buena una cifra que puede estar caducada.
    const frase = describeFamilyStatus({
      linked: true,
      lastSyncAt: "2026-08-01T10:00:00Z",
      lastAppCount: 3000,
      lastErrorCode: "steam_family_session_expired",
    });
    expect(frase).toContain("El último intento falló");
    expect(frase).not.toContain("3.000");
  });

  it("con una lectura buena da el cuándo y el cuánto", () => {
    const frase = describeFamilyStatus({
      linked: true,
      lastSyncAt: "2026-08-01T10:00:00Z",
      lastAppCount: 3812,
    });
    // Cuatro cifras van sin separador de millares: es la convención del
    // español, y `toLocaleString("es-ES")` la aplica sola.
    expect(frase).toContain("3812 juegos");
    expect(frase).toContain("Última lectura");
  });

  it("a partir de cinco cifras aparece el separador de millares", () => {
    const frase = describeFamilyStatus({
      linked: true,
      lastSyncAt: "2026-08-01T10:00:00Z",
      lastAppCount: 12500,
    });
    expect(frase).toContain("12.500 juegos");
  });

  it("un solo juego se dice en singular", () => {
    const frase = describeFamilyStatus({
      linked: true,
      lastSyncAt: "2026-08-01T10:00:00Z",
      lastAppCount: 1,
    });
    expect(frase).toContain("1 juego en el catálogo");
  });
});

describe("resumen de una sincronización de Familia", () => {
  it("no pertenecer a ninguna Familia es una respuesta, no un fallo", () => {
    const frase = describeFamilySync({
      imported: 0,
      unusable: 0,
      withoutTitle: 0,
      noFamily: true,
    });
    expect(frase).toContain("no pertenece a ninguna Familia");
    expect(frase).not.toContain("error");
  });

  it("cuenta lo importado con separador de miles", () => {
    const frase = describeFamilySync({
      imported: 21985,
      unusable: 0,
      withoutTitle: 0,
      noFamily: false,
    });
    expect(frase).toBe("21.985 juegos en el catálogo de tu Familia.");
  });

  it("enseña los huecos en lugar de callarlos", () => {
    // Sin esto, la cifra parecería completa y nadie entendería por qué no cuadra
    // con la que enseña el cliente de Steam.
    const frase = describeFamilySync({
      imported: 1900,
      unusable: 4,
      withoutTitle: 7,
      noFamily: false,
    });
    expect(frase).toContain("1900 juegos");
    expect(frase).toContain("7 sin nombre publicado");
    expect(frase).toContain("4 entradas que Steam devolvió sin identificador");
  });

  it("un único hueco se dice en singular", () => {
    const frase = describeFamilySync({
      imported: 10,
      unusable: 0,
      withoutTitle: 1,
      noFamily: false,
    });
    expect(frase).toContain("1 sin nombre publicado, que no se ha guardado");
  });
});
