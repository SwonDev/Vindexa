import { describe, expect, it } from "vitest";
import CSS from "@/index.css?raw";

/**
 * Trinquete de literales de color en la hoja global.
 *
 * `DESIGN.md` fija los colores del sistema y `:root` los declara. Todo lo que
 * se escribe en hexadecimal fuera de ese bloque es un color que nadie puede
 * reutilizar ni auditar: por eso la hoja acabó con seis grises de pista, dos
 * convenios de progreso y varias familias de gris casi idénticas.
 *
 * Esta prueba no exige cero literales de golpe —serían ciento cincuenta y tres
 * cambios visuales a la vez—, pero sí congela el inventario: no entra ninguno
 * nuevo, no se puede ampliar la lista para tapar uno recién escrito, y cada
 * literal que se retire debe salir también de aquí. La única dirección
 * permitida es hacia abajo.
 */

/** Fondos, bordes y separadores todavía sin token de superficie propio. */
const SUPERFICIES = [
  "#0e1217",
  "#10151b",
  "#13202a",
  "#151a20",
  "#151b21",
  "#151c23",
  "#181e25",
  "#191f26",
  "#19212a",
  "#1a222b",
  "#1b222a",
  "#1b222b",
  "#1c2229",
  "#1c232b",
  "#1d232a",
  "#1d232b",
  "#1d242c",
  "#1d242d",
  "#1d252f",
  "#1d2934",
  "#1e262f",
  "#1e2a37",
  "#20252c",
  "#20262e",
  "#202730",
  "#202831",
  "#202832",
  "#202833",
  "#202a34",
  "#202b35",
  "#212d38",
  "#222a33",
  "#222b35",
  "#252126",
  "#252e37",
  "#252e38",
  "#252e39",
  "#25384a",
  "#26313d",
  "#263748",
  "#263949",
  "#29252a",
  "#29333e",
  "#293847",
  "#2a333d",
  "#2a3b49",
  "#2b333d",
  "#2d3a48",
  "#303943",
  "#303944",
  "#303a45",
  "#304257",
  "#313b48",
  "#31445b",
  "#323d47",
  "#323d48",
  "#343e49",
  "#343e4b",
  "#36414c",
  "#37414d",
  "#37424d",
  "#37434e",
  "#38434e",
  "#394551",
  "#3b4651",
  "#3b4753",
  "#3b4855",
  "#3b4957",
  "#3c4a56",
  "#3d4b59",
  "#3e4a56",
  "#405363",
  "#425364",
  "#4a5664",
  "#4c6677",
  "#506477",
  "#516272",
  "#526273",
  "#607d43",
];

/**
 * Grises de texto y tintes de estado. Su contraste sí está vigilado por
 * `contrast.test.ts`; lo que falta es que salgan de un token compartido.
 */
const TEXTOS = [
  "#526c83",
  "#597080",
  "#8fb9cc",
  "#91a0a8",
  "#93a1a8",
  "#95a3ab",
  "#97a5ab",
  "#9b4a4a",
  "#9ba8ad",
  "#9ba8b3",
  "#9eaaaf",
  "#9eabb0",
  "#a7b2b8",
  "#aab5ba",
  "#abb7bc",
  "#adb8bc",
  "#aeb8bd",
  "#aeb9bd",
  "#aeb9be",
  "#b5c0c3",
  "#b8d980",
  "#b9cbd2",
  "#bcc6c9",
  "#bfced4",
  "#c1d4bd",
  "#c3cbce",
  "#c8d3d6",
  "#c8e868",
  "#cbd4d7",
  "#ccd5d8",
  "#cdd5d7",
  "#cf6a6a",
  "#cfbcbc",
  "#d5dcdf",
  "#d65c5c",
  "#d6e78c",
  "#d8e3e5",
  "#d9b66e",
  "#d9e1e3",
  "#dae1e3",
  "#dbe7ea",
  "#dce3e5",
  "#dce4e6",
  "#dce4e7",
  "#dfe6e8",
  "#e1e7e9",
  "#e1e8ea",
  "#e4ecee",
  "#e6a3a3",
  "#e7ecee",
  "#e7edef",
  "#e8edef",
  "#e9eef0",
  "#e9eff1",
  "#ed8d8d",
  "#edb1b1",
  "#edf2f4",
  "#edf3f4",
  "#eef2f3",
  "#eef2f4",
  "#eef6f9",
  "#ef7777",
  "#f0f4f5",
  "#f18a8a",
  "#f2a2a2",
  "#f2b0b0",
  "#f2b2b2",
  "#f2dede",
  "#f4f6f7",
];

/** Negro y blanco puros: solo aparecen dentro de sombras y degradados. */
const NEUTROS = ["#000", "#ffffff"];

const EXCEPCIONES = new Set([...SUPERFICIES, ...TEXTOS, ...NEUTROS]);

/**
 * Techo de ocurrencias. Baja cuando se migra un literal; si sube es que se
 * reutilizó una excepción existente para escribir un color nuevo, que es
 * exactamente la vía de escape que esta prueba cierra.
 */
const OCURRENCIAS_MAXIMAS = 201;

/** Componentes compartidos: aquí no se admite ni un literal. */
const COMPARTIDOS = [
  ".eyebrow",
  ".screen-heading",
  ".screen-heading__identity",
  ".screen-heading__actions",
  ".metric-strip",
  ".metric-strip__cell",
  ".metric-strip__note",
  ".progress-meter",
  ".progress-meter__value",
  '[data-slot="progress"]',
  '[data-slot="progress-indicator"]',
];

const sinComentarios = CSS.replace(/\/\*[\s\S]*?\*\//g, "");
const bloqueRaiz = /:root,\s*\.dark\s*\{[\s\S]*?\n\}/.exec(sinComentarios);
const fueraDeRaiz = bloqueRaiz ? sinComentarios.replace(bloqueRaiz[0], "") : sinComentarios;
const literales = [...fueraDeRaiz.matchAll(/#[0-9a-fA-F]{3,8}\b/g)].map((match) =>
  match[0].toLowerCase(),
);
const distintos = [...new Set(literales)].sort();

interface ReglaCss {
  selector: string;
  cuerpo: string;
}

function reglas(css: string): ReglaCss[] {
  const encontradas: ReglaCss[] = [];
  const patron = /([^{}]+)\{([^{}]*)\}/g;
  let coincidencia = patron.exec(css);
  while (coincidencia) {
    const selector = (coincidencia[1] ?? "").trim();
    const cuerpo = coincidencia[2] ?? "";
    if (selector && !selector.startsWith("@")) encontradas.push({ selector, cuerpo });
    coincidencia = patron.exec(css);
  }
  return encontradas;
}

describe("literales de color en la hoja global", () => {
  it("declara los tokens en un único bloque `:root`", () => {
    expect(bloqueRaiz, "`:root, .dark` debe seguir siendo el sitio de los tokens").not.toBeNull();
    expect(bloqueRaiz?.[0]).toContain("--v-cyan");
    expect(bloqueRaiz?.[0]).toContain("--v-lime");
  });

  it("no aparece ningún color nuevo fuera de `:root`", () => {
    const intrusos = distintos.filter((hex) => !EXCEPCIONES.has(hex));
    expect(
      intrusos,
      `Colores sin token ni excepción: ${intrusos.join(", ")}. Usa un token de \`:root\`.`,
    ).toEqual([]);
  });

  it("la lista de excepciones no conserva colores ya migrados", () => {
    const caducadas = [...EXCEPCIONES].filter((hex) => !distintos.includes(hex)).sort();
    expect(
      caducadas,
      `Excepciones que ya no existen en la hoja: ${caducadas.join(", ")}. Bórralas de la lista.`,
    ).toEqual([]);
  });

  it("el inventario de literales no crece", () => {
    expect(literales.length).toBeLessThanOrEqual(OCURRENCIAS_MAXIMAS);
  });

  it("los componentes compartidos salen enteros de los tokens", () => {
    const culpables = reglas(sinComentarios)
      .filter((regla) =>
        regla.selector
          .split(",")
          .map((parte) => parte.trim())
          .some((parte) => COMPARTIDOS.includes(parte)),
      )
      .filter((regla) => /#[0-9a-fA-F]{3,8}\b/.test(regla.cuerpo))
      .map((regla) => regla.selector);
    expect(
      culpables,
      `Componentes compartidos con color literal: ${culpables.join(" | ")}`,
    ).toEqual([]);
  });

  it("el progreso se dibuja con un solo convenio", () => {
    // Dos pantallas pintaban el mismo porcentaje en azul acero y en lima. El
    // relleno parcial y el completo se declaran una vez, y en ningún sitio más.
    const indicadores = reglas(sinComentarios).filter((regla) =>
      regla.selector.includes('[data-slot="progress-indicator"]'),
    );
    expect(indicadores).toHaveLength(2);
    const fondos = indicadores.map((regla) =>
      (/background:\s*([^;]+);/.exec(regla.cuerpo)?.[1] ?? "").trim(),
    );
    expect(fondos).toEqual(["var(--v-success)", "var(--v-lime)"]);
  });
});
