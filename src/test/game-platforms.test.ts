import { describe, expect, it } from "vitest";
import {
  detectHostPlatform,
  platformSupport,
  platformWarning,
} from "@/features/library/game-platforms";

const MAC_UA =
  "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko)";

describe("detectHostPlatform", () => {
  it("reconoce los tres sistemas de escritorio", () => {
    expect(detectHostPlatform(MAC_UA)).toBe("mac");
    expect(detectHostPlatform("Mozilla/5.0 (Windows NT 10.0; Win64; x64)")).toBe("windows");
    expect(detectHostPlatform("Mozilla/5.0 (X11; Linux x86_64)")).toBe("linux");
  });

  it("ante un agente que no reconoce no inventa un sistema", () => {
    expect(detectHostPlatform("algo raro")).toBe("unknown");
  });
});

describe("platformSupport", () => {
  it("distingue las tres respuestas posibles", () => {
    expect(platformSupport({ platformMac: true }, "mac")).toBe("supported");
    expect(platformSupport({ platformMac: false }, "mac")).toBe("unsupported");
    // Sin dato no se afirma nada: es la diferencia que evita desaconsejar una
    // instalación que quizá sí funciona.
    expect(platformSupport({}, "mac")).toBe("unknown");
  });

  it("trata el `null` del backend como desconocido, no como incompatible", () => {
    // Rust envía `Option<bool>` como `null`, no omite el campo. Comparar sólo
    // contra `undefined` dejaba pasar el `null` a un ternario donde es falsy, y
    // un juego sin dato salía marcado como imposible de instalar. Le pasó a
    // Crimson Desert, que sí tiene versión para macOS.
    expect(platformSupport({ platformMac: null }, "mac")).toBe("unknown");
    expect(platformSupport({ platformWindows: null }, "windows")).toBe("unknown");
    expect(platformSupport({ platformLinux: null }, "linux")).toBe("unknown");
  });

  it("con un sistema desconocido nunca se pronuncia", () => {
    expect(platformSupport({ platformMac: false }, "unknown")).toBe("unknown");
  });

  it("mira la bandera del sistema correcto", () => {
    const juego = { platformWindows: true, platformMac: false, platformLinux: false };
    expect(platformSupport(juego, "windows")).toBe("supported");
    expect(platformSupport(juego, "mac")).toBe("unsupported");
    expect(platformSupport(juego, "linux")).toBe("unsupported");
  });
});

describe("platformWarning", () => {
  it("dice para qué sistemas sí existe", () => {
    expect(
      platformWarning({ platformWindows: true, platformMac: false, platformLinux: false }, "mac"),
    ).toBe("Este juego no tiene versión para macOS. La tienda solo lo ofrece para Windows.");
  });

  it("enumera varios sistemas alternativos", () => {
    expect(
      platformWarning({ platformWindows: true, platformMac: false, platformLinux: true }, "mac"),
    ).toBe(
      "Este juego no tiene versión para macOS. La tienda solo lo ofrece para Windows y Linux.",
    );
  });

  it("no promete alternativas cuando no las hay", () => {
    expect(
      platformWarning({ platformWindows: false, platformMac: false, platformLinux: false }, "mac"),
    ).toBe("Este juego no tiene versión para macOS.");
  });

  it("calla cuando es compatible o cuando no se sabe", () => {
    expect(platformWarning({ platformMac: true }, "mac")).toBeUndefined();
    expect(platformWarning({}, "mac")).toBeUndefined();
    expect(platformWarning({ platformMac: null }, "mac")).toBeUndefined();
    expect(platformWarning({ platformMac: false }, "unknown")).toBeUndefined();
  });

  it("no cuenta como alternativa un sistema del que no se sabe nada", () => {
    // Con `null` en Windows y Linux, la única frase honesta es que no hay
    // versión para macOS: prometer que «sólo está en Windows» sería inventarlo.
    expect(
      platformWarning({ platformWindows: null, platformMac: false, platformLinux: null }, "mac"),
    ).toBe("Este juego no tiene versión para macOS.");
  });
});
