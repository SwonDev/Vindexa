import { describe, expect, it } from "vitest";
import {
  DURATION,
  DURATION_MS,
  EASE_IN_OUT,
  EASE_OUT,
  REVEAL_DISTANCE_PX,
  SPRING_SNAP,
  SPRING_STACK,
  STAGGER_MAX_MS,
  STAGGER_STEP_MS,
  TRANSITION_NONE,
  withReducedMotion,
} from "@/components/motion";
import motionCss from "@/components/motion/motion.css?raw";
import appCss from "@/index.css?raw";

describe("tokens de movimiento", () => {
  it("mantiene todas las duraciones dentro del rango del sistema de diseño", () => {
    const functional = [DURATION.instant, DURATION.fast, DURATION.base, DURATION.slow];
    for (const duration of functional) {
      expect(duration).toBeGreaterThanOrEqual(0.12);
      expect(duration).toBeLessThanOrEqual(0.26);
    }
    // Única excepción permitida: el desplegable de altura medida.
    expect(DURATION.disclosure).toBeLessThanOrEqual(0.32);
  });

  it("expresa las mismas duraciones en segundos y en milisegundos", () => {
    for (const key of Object.keys(DURATION) as (keyof typeof DURATION)[]) {
      expect(DURATION_MS[key]).toBe(Math.round(DURATION[key] * 1000));
    }
  });

  it("reproduce exactamente las curvas declaradas en index.css", () => {
    expect(appCss).toContain(`--ease-out: cubic-bezier(${EASE_OUT.join(", ")})`);
    expect(appCss).toContain(`--ease-in-out: cubic-bezier(${EASE_IN_OUT.join(", ")})`);
  });

  it("define muelles sobreamortiguados, sin rebote visible", () => {
    for (const spring of [SPRING_SNAP, SPRING_STACK]) {
      const stiffness = spring.stiffness as number;
      const damping = spring.damping as number;
      const mass = spring.mass as number;
      const critical = 2 * Math.sqrt(stiffness * mass);
      expect(damping).toBeGreaterThan(critical);
    }
  });

  it("limita el escalonado para que el último elemento no se haga esperar", () => {
    expect(STAGGER_STEP_MS).toBeLessThanOrEqual(40);
    expect(STAGGER_MAX_MS).toBeLessThanOrEqual(200);
    expect(REVEAL_DISTANCE_PX).toBeLessThanOrEqual(8);
  });

  it("devuelve una transición nula cuando hay que suprimir el movimiento", () => {
    const transition = { duration: 0.2 };
    expect(withReducedMotion(transition, false)).toBe(transition);
    expect(withReducedMotion(transition, true)).toBe(TRANSITION_NONE);
    expect(TRANSITION_NONE.duration).toBe(0);
  });
});

describe("hoja de estilos de las microinteracciones", () => {
  it("solo anima transform y opacity de forma continua", () => {
    const animations = motionCss.match(/animation:\s*[^;]+;/g) ?? [];
    expect(animations.length).toBeGreaterThan(0);
    for (const declaration of animations) {
      if (declaration.includes("none")) continue;
      expect(declaration).toContain("vx-skeleton-sweep");
    }
    const sweep = motionCss.slice(motionCss.indexOf("@keyframes vx-skeleton-sweep"));
    expect(sweep.slice(0, 120)).toContain("transform: translateX(100%)");
  });

  it("usa las variables de curva del sistema y no valores sueltos", () => {
    expect(motionCss).toContain("var(--ease-out)");
    expect(motionCss).not.toMatch(/cubic-bezier\(/);
  });

  it("no introduce radios por encima de 3 px", () => {
    const radii = motionCss.match(/border-radius:\s*(\d+)px/g) ?? [];
    expect(radii.length).toBeGreaterThan(0);
    for (const declaration of radii) {
      const value = Number(declaration.replace(/\D/g, ""));
      expect(value).toBeLessThanOrEqual(3);
    }
  });

  it("suprime el movimiento por completo con prefers-reduced-motion", () => {
    const index = motionCss.indexOf("@media (prefers-reduced-motion: reduce)");
    expect(index).toBeGreaterThan(-1);
    const block = motionCss.slice(index);
    expect(block).toContain("transition: none");
    expect(block).toContain("transform: none");
    expect(block).toContain("animation: none");
  });
});
