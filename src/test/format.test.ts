import { describe, expect, it } from "vitest";
import {
  formatBytes,
  formatDate,
  formatPlaytime,
  formatSteamDeckStatus,
  initials,
} from "@/lib/format";

describe("formato de metadatos de biblioteca", () => {
  it("muestra minutos y horas sin perder el resto", () => {
    expect(formatPlaytime(0)).toBe("0 min");
    expect(formatPlaytime(59)).toBe("59 min");
    expect(formatPlaytime(60)).toBe("1 h");
    expect(formatPlaytime(185)).toBe("3 h 5 min");
  });

  it("usa unidades binarias legibles, coma decimal y un vacío explícito", () => {
    expect(formatBytes()).toBe("—");
    expect(formatBytes(0)).toBe("—");
    // La interfaz está en español: un punto decimal se lee como separador de
    // millar y convierte «1.5 GB» en «mil quinientos gigabytes».
    expect(formatBytes(1_024)).toBe("1,0 KB");
    expect(formatBytes(1_536 * 1_024 * 1_024)).toBe("1,5 GB");
    expect(formatBytes(10 * 1_024 * 1_024)).toBe("10 MB");
    expect(formatBytes(8 * 1_024 * 1_024 * 1_024)).toBe("8,0 GB");
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

describe("formatSteamDeckStatus", () => {
  it("traduce los valores oficiales de compatibilidad", () => {
    expect(formatSteamDeckStatus("verified")).toBe("Verificado");
    expect(formatSteamDeckStatus("Playable")).toBe("Jugable");
    expect(formatSteamDeckStatus("unsupported")).toBe("No compatible");
    expect(formatSteamDeckStatus("unknown")).toBe("Sin comprobar");
  });

  it("no inventa traducciones para valores que no conoce", () => {
    expect(formatSteamDeckStatus("pending_review")).toBe("pending_review");
    expect(formatSteamDeckStatus("")).toBeUndefined();
    expect(formatSteamDeckStatus(undefined)).toBeUndefined();
  });
});
