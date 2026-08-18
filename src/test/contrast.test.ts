import { describe, expect, it } from "vitest";
import CSS from "@/index.css?raw";

/**
 * Puerta de accesibilidad del sistema de color.
 *
 * `DESIGN.md` se compromete por escrito con WCAG 2.2 AA (4,5:1 para texto
 * normal). Esta prueba lo hace verificable: recorre cada `color:` literal de la
 * hoja global y comprueba su contraste contra la superficie **más clara** del
 * proyecto, que es el peor caso realista para un texto secundario.
 */
/** `--v-surface-raised`: el fondo más claro sobre el que se escribe texto. */
const REFERENCE_BACKGROUND = "#2a3441";
const AA_NORMAL_TEXT = 4.5;

function channel(value: number): number {
  const c = value / 255;
  return c <= 0.04045 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4;
}

function luminance(hex: string): number {
  const value = hex.replace("#", "");
  const full = value.length === 3 ? [...value].map((c) => c + c).join("") : value;
  const [r, g, b] = [0, 2, 4].map((index) =>
    channel(Number.parseInt(full.slice(index, index + 2), 16)),
  ) as [number, number, number];
  return 0.2126 * r + 0.7152 * g + 0.0722 * b;
}

function contrastRatio(foreground: string, background: string): number {
  const a = luminance(foreground) + 0.05;
  const b = luminance(background) + 0.05;
  return Math.max(a, b) / Math.min(a, b);
}

/** Colores vivos: llevan su propia semántica y su contraste comprobado aparte. */
function isVividAccent(hex: string): boolean {
  const value = hex.replace("#", "");
  const [r, g, b] = [0, 2, 4].map((index) =>
    Number.parseInt(value.slice(index, index + 2), 16),
  ) as [number, number, number];
  const max = Math.max(r, g, b);
  return max > 0 && (max - Math.min(r, g, b)) / max > 0.28;
}

describe("contraste del sistema de color", () => {
  const declared = [...CSS.matchAll(/color:\s*(#[0-9a-fA-F]{3,6})\s*;/g)].map((match) =>
    (match[1] as string).toLowerCase(),
  );
  const greys = [...new Set(declared)].filter((hex) => !isVividAccent(hex));

  it("declara al menos un color literal que auditar", () => {
    expect(declared.length).toBeGreaterThan(0);
  });

  it.each(greys)("%s alcanza AA sobre la superficie más clara del proyecto", (hex) => {
    expect(contrastRatio(hex, REFERENCE_BACKGROUND)).toBeGreaterThanOrEqual(AA_NORMAL_TEXT);
  });

  it("los dos tokens de texto apagado cumplen AA con margen", () => {
    // Terciario y secundario. Si alguien los oscurece, esta prueba lo detiene.
    expect(contrastRatio("#93a1aa", REFERENCE_BACKGROUND)).toBeGreaterThanOrEqual(4.5);
    expect(contrastRatio("#abb7b5", REFERENCE_BACKGROUND)).toBeGreaterThanOrEqual(6);
    // Y conservan jerarquía entre sí: el terciario es más apagado.
    expect(contrastRatio("#93a1aa", REFERENCE_BACKGROUND)).toBeLessThan(
      contrastRatio("#abb7b5", REFERENCE_BACKGROUND),
    );
  });
});
