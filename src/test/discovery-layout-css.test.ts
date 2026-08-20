import { describe, expect, it } from "vitest";
import discoveryCss from "@/features/discovery/discovery.css?raw";

/**
 * Devuelve el cuerpo de la primera regla cuyo selector coincide exactamente.
 * Basta con un recorte por llaves: el archivo no anida reglas dentro de reglas.
 */
function ruleBody(css: string, selector: string): string {
  const pattern = new RegExp(
    `(^|[},])\\s*${selector.replace(/[.*+?^${}()|[\]\\]/g, "\\$&")}\\s*\\{([^}]*)\\}`,
    "m",
  );
  const match = css.match(pattern);
  if (!match?.[2]) throw new Error(`No existe la regla «${selector}» en discovery.css`);
  return match[2];
}

describe("geometría de la pantalla de seguimiento", () => {
  it("resuelve la altura de la pantalla para que la zona de scroll pueda desbordar", () => {
    // `.app-content` es `overflow: hidden` y no crece. Si `.discovery-screen`
    // no declara una altura resuelta, la fila `minmax(0, 1fr)` queda sin acotar,
    // el contenido se recorta y ninguna zona interior llega nunca a desbordar:
    // ese fue exactamente el fallo de scroll de esta pantalla.
    const screen = ruleBody(discoveryCss, ".discovery-screen");
    expect(screen).toMatch(/height:\s*100%/);
    expect(screen).toMatch(/overflow:\s*hidden/);
    expect(screen).toMatch(/grid-template-rows:\s*auto\s+minmax\(0,\s*1fr\)/);
  });

  it("acota cada contenedor intermedio hasta la zona desplazable", () => {
    // `.discovery-signals` entró en esta lista cuando dejó de desplazarse ella
    // misma: ahora sostiene su navegación arriba y deja que se desplace sólo el
    // panel del grupo elegido.
    for (const selector of [
      ".discovery-body",
      ".discovery-main",
      ".radar-panel",
      ".discovery-signals",
    ]) {
      const body = ruleBody(discoveryCss, selector);
      expect(body, `${selector} debe poder encogerse`).toMatch(/min-height:\s*0/);
      expect(body, `${selector} no debe desplazarse por su cuenta`).toMatch(/overflow:\s*hidden/);
    }
  });

  it("declara zonas de desplazamiento propias y hermanas, nunca anidadas", () => {
    for (const selector of [".radar-scroll", ".discovery-signals__panel", ".discovery-rail"]) {
      const body = ruleBody(discoveryCss, selector);
      expect(body, `${selector} debe desplazarse en vertical`).toMatch(/overflow:\s*hidden\s+auto/);
      expect(body, `${selector} debe poder encogerse`).toMatch(/min-height:\s*0/);
    }
  });

  it("mantiene la geometría técnica: ningún radio redondo ni mayor de 4 px", () => {
    const radii = discoveryCss.match(/border-radius:\s*([^;]+);/g) ?? [];
    expect(radii.length).toBeGreaterThan(0);
    for (const declaration of radii) {
      expect(declaration).not.toMatch(/9{3,}px|50%/);
      const pixels = declaration.match(/(\d+(?:\.\d+)?)px/g) ?? [];
      for (const value of pixels) {
        expect(
          Number.parseFloat(value),
          `radio fuera de rango en «${declaration}»`,
        ).toBeLessThanOrEqual(4);
      }
    }
  });

  it("fija la altura del panel de decisión para que ningún estado desplace el radar", () => {
    const decision = ruleBody(
      discoveryCss,
      ".decision-result,\n.decision-panel > .discovery-error",
    );
    expect(decision).toMatch(/height:\s*\d+px/);
  });

  it("usa cifras tabulares en los datos y respeta la reducción de movimiento", () => {
    expect(discoveryCss).toMatch(/font-variant-numeric:\s*tabular-nums/);
    expect(discoveryCss).toContain("@media (prefers-reduced-motion: reduce)");
    const reduced = discoveryCss.slice(discoveryCss.indexOf("prefers-reduced-motion"));
    expect(reduced).toMatch(/animation:\s*none/);
  });
});
