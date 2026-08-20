import { describe, expect, it } from "vitest";
import {
  buildLibraryLayout,
  currentGroupAt,
  GROUP_HEADER_HEIGHT,
  rowOffsets,
  stickyGroupAt,
} from "@/features/library/group-layout";
import { groupLibrary } from "@/features/library/library-grouping";
import type { GameSummary } from "@/lib/types";

function game(appId: number, title: string): GameSummary {
  return {
    appId,
    title,
    playtimeMinutes: 0,
    playtimeRecentMinutes: 0,
    isFree: false,
    drmState: "unknown",
    ownershipSource: "owned",
    familyAvailability: "not_applicable",
    genres: [],
    isEarlyAccess: false,
    installed: false,
    statusId: "unclassified",
    statusName: "Sin clasificar",
    statusColor: "#8493A4",
    progress: 0,
    priority: 0,
    pinned: false,
    tracking: false,
    manualPosition: appId,
    collectionIds: [],
  };
}

const NOW = new Date("2026-08-18T12:00:00Z");
const games = [
  game(1, "Abzû"),
  game(2, "Alba"),
  game(3, "Braid"),
  game(4, "Celeste"),
  game(5, "Control"),
  game(6, "Cuphead"),
  game(7, "Dead Cells"),
];
const groups = groupLibrary(games, "initial", NOW);

describe("filas de la biblioteca", () => {
  it("sin agrupar entrega una fila por juego y ningún destino de salto", () => {
    const layout = buildLibraryLayout(games, [], 1);
    expect(layout.rows).toHaveLength(games.length);
    expect(layout.rows.every((row) => row.kind === "games")).toBe(true);
    expect(layout.jumps).toEqual([]);
    expect(layout.groupOfRow.every((group) => group === -1)).toBe(true);
    expect(layout.rowOfGame.get(7)).toBe(6);
  });

  it("intercala un encabezado por grupo y apunta el salto a su fila", () => {
    const layout = buildLibraryLayout(games, groups, 1);
    expect(layout.rows.map((row) => (row.kind === "header" ? row.label : "·"))).toEqual([
      "A",
      "·",
      "·",
      "B",
      "·",
      "C",
      "·",
      "·",
      "·",
      "D",
      "·",
    ]);
    expect(layout.jumps.map((jump) => [jump.key, jump.row])).toEqual([
      ["A", 0],
      ["B", 3],
      ["C", 5],
      ["D", 9],
    ]);
    expect(layout.rows[0]).toMatchObject({ kind: "header", loaded: 2 });
  });

  it("en rejilla cada grupo arranca en la columna cero y nunca mezcla dos", () => {
    const layout = buildLibraryLayout(games, groups, 3);
    const cells = layout.rows.flatMap((row) => (row.kind === "games" ? [row.games.length] : []));
    // «C» tiene tres juegos y llena una fila; «A» y «D» dejan la suya a medias.
    expect(cells).toEqual([2, 1, 3, 1]);
    for (const row of layout.rows) {
      if (row.kind !== "games") continue;
      const initials = new Set(row.games.map((item) => item.title.charAt(0)));
      expect(initials.size).toBe(1);
    }
  });

  it("no pierde ni duplica ningún juego al repartir en columnas", () => {
    for (const columns of [1, 2, 3, 5, 8]) {
      const layout = buildLibraryLayout(games, groups, columns);
      const ids = layout.rows
        .flatMap((row) => (row.kind === "games" ? row.games.map((item) => item.appId) : []))
        .sort((a, b) => a - b);
      expect(ids, `con ${columns} columnas`).toEqual([1, 2, 3, 4, 5, 6, 7]);
      expect(layout.rowOfGame.size).toBe(games.length);
    }
  });
});

describe("desplazamientos de las filas", () => {
  it("encadena encabezados y filas sin solape ni hueco", () => {
    const layout = buildLibraryLayout(games, groups, 1);
    const offsets = rowOffsets(layout.rows, 40);
    expect(offsets).toHaveLength(layout.rows.length + 1);
    for (let index = 0; index < layout.rows.length; index += 1) {
      const size = layout.rows[index]?.kind === "header" ? GROUP_HEADER_HEIGHT : 40;
      expect((offsets[index + 1] ?? 0) - (offsets[index] ?? 0)).toBe(size);
    }
    expect(offsets.at(-1)).toBe(4 * GROUP_HEADER_HEIGHT + games.length * 40);
  });
});

describe("grupo fijado al borde", () => {
  const layout = buildLibraryLayout(games, groups, 1);
  const offsets = rowOffsets(layout.rows, 40);
  const startOf = (jump: number) => offsets[layout.jumps[jump]?.row ?? 0] ?? 0;

  it("releva al grupo justo cuando su encabezado alcanza la franja", () => {
    expect(stickyGroupAt(layout.jumps, offsets, startOf(1) - 1)?.index).toBe(0);
    expect(stickyGroupAt(layout.jumps, offsets, startOf(1))?.index).toBe(1);
  });

  it("deja que el encabezado siguiente empuje la franja sin saltos", () => {
    const next = startOf(1);
    expect(stickyGroupAt(layout.jumps, offsets, next - GROUP_HEADER_HEIGHT)?.shift).toBe(0);
    expect(stickyGroupAt(layout.jumps, offsets, next - GROUP_HEADER_HEIGHT / 2)?.shift).toBe(
      -GROUP_HEADER_HEIGHT / 2,
    );
    // Al relevarse, el empuje vuelve a cero: la franja no da un tirón.
    expect(stickyGroupAt(layout.jumps, offsets, next)?.index).toBe(1);
    expect(stickyGroupAt(layout.jumps, offsets, next)?.shift).toBe(0);
  });

  it("no fija nada por encima del primer encabezado ni sin agrupación", () => {
    expect(stickyGroupAt(layout.jumps, offsets, -12)).toBeUndefined();
    expect(stickyGroupAt([], [], 0)).toBeUndefined();
    expect(currentGroupAt([], [], 0)).toBe(-1);
  });

  it("el grupo marcado en el índice sigue a la lectura", () => {
    expect(currentGroupAt(layout.jumps, offsets, 0)).toBe(0);
    expect(currentGroupAt(layout.jumps, offsets, startOf(2) + 5)).toBe(2);
    expect(currentGroupAt(layout.jumps, offsets, offsets.at(-1) ?? 0)).toBe(3);
  });
});
