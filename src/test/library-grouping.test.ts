import { describe, expect, it } from "vitest";
import {
  groupIndex,
  groupLibrary,
  LIBRARY_GROUPINGS,
  lastPlayedBucket,
  titleInitial,
} from "@/features/library/library-grouping";
import type { GameSummary } from "@/lib/types";

function game(overrides: Partial<GameSummary> & Pick<GameSummary, "appId" | "title">): GameSummary {
  return {
    playtimeMinutes: 0,
    playtimeRecentMinutes: 0,
    isEarlyAccess: false,
    isFree: false,
    ownershipSource: "owned",
    familyAvailability: "not_applicable",
    installed: false,
    statusId: "unclassified",
    statusName: "Sin clasificar",
    statusColor: "#8493A4",
    progress: 0,
    priority: 0,
    pinned: false,
    tracking: false,
    manualPosition: 0,
    collectionIds: [],
    ...overrides,
  };
}

const NOW = new Date("2026-08-18T12:00:00Z");

describe("inicial del título", () => {
  it("ignora artículos iniciales en español e inglés", () => {
    expect(titleInitial("The Witcher 3")).toBe("W");
    expect(titleInitial("El Reino Olvidado")).toBe("R");
    expect(titleInitial("Las Crónicas")).toBe("C");
    expect(titleInitial("A Plague Tale")).toBe("P");
  });

  it("normaliza acentos y agrupa dígitos y símbolos bajo '#'", () => {
    expect(titleInitial("Ánima")).toBe("A");
    expect(titleInitial("Übermorgen")).toBe("U");
    expect(titleInitial("7 Days to Die")).toBe("#");
    expect(titleInitial("...y luego nada")).toBe("#");
    expect(titleInitial("")).toBe("#");
  });
});

describe("antigüedad de la última sesión", () => {
  it("reparte en cortes con sentido y trata lo desconocido aparte", () => {
    expect(lastPlayedBucket(undefined, NOW)).toBe("Nunca jugado");
    expect(lastPlayedBucket("no-es-una-fecha", NOW)).toBe("Nunca jugado");
    expect(lastPlayedBucket("2026-08-18T08:00:00Z", NOW)).toBe("Hoy");
    expect(lastPlayedBucket("2026-08-14T08:00:00Z", NOW)).toBe("Esta semana");
    expect(lastPlayedBucket("2026-08-01T08:00:00Z", NOW)).toBe("Este mes");
    expect(lastPlayedBucket("2026-06-01T08:00:00Z", NOW)).toBe("Últimos tres meses");
    expect(lastPlayedBucket("2026-01-01T08:00:00Z", NOW)).toBe("Este año");
    expect(lastPlayedBucket("2020-01-01T08:00:00Z", NOW)).toBe("Hace más de un año");
  });
});

describe("agrupación de la biblioteca", () => {
  const games = [
    game({
      appId: 1,
      title: "Zelda",
      statusName: "Jugando",
      genres: ["Aventura"],
      releaseDate: "2017-03-03",
      developer: "Nintendo",
    }),
    game({
      appId: 2,
      title: "The Witcher 3",
      statusName: "Terminados",
      genres: ["Rol"],
      releaseDate: "2015-05-18",
      developer: "CD PROJEKT RED",
    }),
    game({
      appId: 3,
      title: "Ánima",
      statusName: "Jugando",
      genres: ["Rol"],
      releaseDate: "2015-01-09",
      developer: "Nintendo",
    }),
    game({
      appId: 4,
      title: "7 Days to Die",
      statusName: "Pendientes",
      developer: "The Fun Pimps",
    }),
  ];

  it("no agrupa cuando no se pide", () => {
    expect(groupLibrary(games, "none", NOW)).toEqual([]);
    expect(groupLibrary([], "initial", NOW)).toEqual([]);
  });

  it("agrupa por inicial en orden alfabético y deja '#' al final", () => {
    const groups = groupLibrary(games, "initial", NOW);
    expect(groups.map((group) => group.key)).toEqual(["A", "W", "Z", "#"]);
    expect(groups[0]?.games.map((item) => item.appId)).toEqual([3]);
  });

  it("conserva el orden recibido dentro de cada grupo", () => {
    const groups = groupLibrary(games, "status", NOW);
    const playing = groups.find((group) => group.key === "Jugando");
    // El orden lo decide el backend: agrupar no puede reordenar por su cuenta.
    expect(playing?.games.map((item) => item.appId)).toEqual([1, 3]);
  });

  it("ordena los años de más reciente a más antiguo y aparta lo desconocido", () => {
    const groups = groupLibrary(games, "releaseYear", NOW);
    expect(groups.map((group) => group.key)).toEqual(["2017", "2015", "Sin dato"]);
  });

  it("agrupa por género y por estudio, con un cajón explícito para lo que falta", () => {
    expect(groupLibrary(games, "genre", NOW).map((group) => group.key)).toEqual([
      "Aventura",
      "Rol",
      "Sin dato",
    ]);
    expect(groupLibrary(games, "developer", NOW).map((group) => group.key)).toEqual([
      "CD PROJEKT RED",
      "Nintendo",
      "The Fun Pimps",
    ]);
  });

  it("no pierde ni duplica ningún juego al agrupar", () => {
    for (const option of LIBRARY_GROUPINGS) {
      if (option.id === "none") continue;
      const groups = groupLibrary(games, option.id, NOW);
      const ids = groups.flatMap((group) => group.games.map((item) => item.appId)).sort();
      expect(ids, `agrupación ${option.id}`).toEqual([1, 2, 3, 4]);
    }
  });

  it("el índice de salto acorta las etiquetas largas y respeta el orden", () => {
    const groups = groupLibrary(games, "status", NOW);
    expect(groupIndex(groups).map((entry) => entry.key)).toEqual(groups.map((group) => group.key));
    expect(groupIndex(groups).every((entry) => entry.label.length <= 4)).toBe(true);
  });
});
