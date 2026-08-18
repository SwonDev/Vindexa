import { describe, expect, it } from "vitest";
import CSS from "@/features/collections/collections.css?raw";

/**
 * Puerta de diseño de la pantalla de Colecciones.
 *
 * `DESIGN.md` y la auditoría visual fijan cuatro compromisos que se pueden
 * comprobar leyendo la hoja: geometría casi ortogonal, contraste AA en todo el
 * texto, color con significado —el tipo de colección, nunca la identidad— y
 * movimiento corto sobre propiedades compuestas. Aquí se verifican para que no
 * se pierdan en la siguiente edición.
 */

const REFERENCE_BACKGROUND = "#2a3441";
const AA_NORMAL_TEXT = 4.5;
/** Tokens de texto permitidos, todos con AA comprobado en `contrast.test.ts`. */
const TEXT_TOKENS = new Set([
  "--foreground",
  "--v-muted",
  "--v-subtle",
  "--v-cyan",
  "--destructive",
]);

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

describe("contrato de diseño de la pantalla de colecciones", () => {
  it("no dibuja ni un solo círculo: todos los radios caben en 0–4 px", () => {
    const radii = [...CSS.matchAll(/border-radius:\s*([^;]+);/g)].map((match) =>
      (match[1] as string).trim(),
    );
    expect(radii.length).toBeGreaterThan(0);
    for (const radius of radii) {
      expect(radius).not.toMatch(/9999|999px|50%/);
      for (const value of radius.match(/(\d+(?:\.\d+)?)px/g) ?? []) {
        expect(Number.parseFloat(value)).toBeLessThanOrEqual(4);
      }
    }
  });

  it("todo el texto sale de tokens con AA garantizado", () => {
    // `(?<![-\w])` descarta `border-color`, `background-color` y compañía: aquí
    // solo se audita el color del texto.
    const declarations = [...CSS.matchAll(/(?<![-\w])color:\s*([^;]+);/g)].map((match) =>
      (match[1] as string).trim(),
    );
    expect(declarations.length).toBeGreaterThan(0);
    for (const declaration of declarations) {
      if (declaration === "inherit") continue;
      const literal = /^#[0-9a-fA-F]{3,6}$/.exec(declaration);
      if (literal) {
        expect(
          contrastRatio(declaration.toLowerCase(), REFERENCE_BACKGROUND),
        ).toBeGreaterThanOrEqual(AA_NORMAL_TEXT);
        continue;
      }
      const token = /^var\((--[a-z0-9-]+)\)$/.exec(declaration);
      expect(token, `color no tokenizado: ${declaration}`).not.toBeNull();
      expect(TEXT_TOKENS.has(token?.[1] ?? ""), `token de texto sin AA: ${declaration}`).toBe(true);
    }
  });

  it("el color codifica el tipo de colección, no la identidad de cada una", () => {
    // Ningún acento se pinta con el color libre elegido por la persona: eso
    // colisionaba con las familias semánticas del sistema y llegaba a repetirse.
    expect(CSS).not.toContain("--tile-accent");
    // Y el lima, reservado por `DESIGN.md` a progreso y confirmación, no decora.
    expect(CSS).not.toContain("--v-lime");
    expect(CSS).toContain('.collection-tile[data-kind="smart"] .collection-tile__accent');
  });

  it("el movimiento dura 120–260 ms, usa las curvas del sistema y solo compone", () => {
    const transitions = [...CSS.matchAll(/transition:\s*([^;]+);/g)].map((match) =>
      (match[1] as string).replace(/\s+/g, " ").trim(),
    );
    expect(transitions.length).toBeGreaterThan(0);
    for (const transition of transitions) {
      if (transition === "none") continue;
      expect(transition).toMatch(/^(opacity|transform)\b/);
      expect(transition).toContain("var(--ease-out)");
      for (const value of transition.match(/(\d+)ms/g) ?? []) {
        const duration = Number.parseInt(value, 10);
        expect(duration).toBeGreaterThanOrEqual(120);
        expect(duration).toBeLessThanOrEqual(260);
      }
    }
  });

  it("respeta la preferencia de movimiento reducido", () => {
    expect(CSS).toContain("@media (prefers-reduced-motion: reduce)");
  });
});
