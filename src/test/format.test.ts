import { describe, expect, it } from "vitest";
import { formatBytes, formatDate, formatPlaytime, initials } from "@/lib/format";

describe("formato de metadatos de biblioteca", () => {
  it("muestra minutos y horas sin perder el resto", () => {
    expect(formatPlaytime(0)).toBe("0 min");
    expect(formatPlaytime(59)).toBe("59 min");
    expect(formatPlaytime(60)).toBe("1 h");
    expect(formatPlaytime(185)).toBe("3 h 5 min");
  });

  it("usa unidades binarias legibles y un vacío explícito", () => {
    expect(formatBytes()).toBe("—");
    expect(formatBytes(0)).toBe("—");
    expect(formatBytes(1_024)).toBe("1.0 KB");
    expect(formatBytes(10 * 1_024 * 1_024)).toBe("10 MB");
  });

  it("conserva una fecha inválida y localiza una fecha real", () => {
    expect(formatDate()).toBe("Nunca");
    expect(formatDate("fecha-desconocida")).toBe("fecha-desconocida");
    expect(formatDate("2026-08-14T12:00:00Z")).toMatch(/14.*ago.*2026/i);
  });

  it("crea iniciales Unicode para placeholders de carátula", () => {
    expect(initials("Árbol Cósmico")).toBe("ÁC");
    expect(initials("Portal")).toBe("P");
    expect(initials("   ")).toBe("");
  });
});
