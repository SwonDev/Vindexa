import { describe, expect, it } from "vitest";
import {
  describeUndo,
  MAX_UNDO_ENTRIES,
  peekUndo,
  popUndo,
  pruneUndo,
  pushUndo,
  type UndoEntry,
} from "@/features/library/library-undo";

function edicion(appId: number, label = `edición ${appId}`): UndoEntry {
  return {
    kind: "gameEdit",
    label,
    previous: {
      appId,
      statusId: "backlog",
      progress: 0,
      priority: 3,
      pinned: false,
      tracking: false,
    },
  };
}

const arrastre: UndoEntry = {
  kind: "drop",
  label: "3 juegos a Terminados",
  receipt: { entries: [] } as unknown as UndoEntry extends { receipt: infer R } ? R : never,
};

describe("pila de deshacer", () => {
  it("apila y devuelve la última entrada", () => {
    const pila = pushUndo(pushUndo([], edicion(1)), edicion(2));
    expect(pila).toHaveLength(2);
    expect(peekUndo(pila)?.label).toBe("edición 2");
  });

  it("saca la cima sin mutar la pila original", () => {
    const pila = pushUndo(pushUndo([], edicion(1)), edicion(2));
    const { entry, rest } = popUndo(pila);
    expect(entry?.label).toBe("edición 2");
    expect(rest).toHaveLength(1);
    expect(pila).toHaveLength(2);
  });

  it("una pila vacía no da nada que deshacer", () => {
    expect(peekUndo([])).toBeUndefined();
    expect(popUndo([])).toEqual({ entry: undefined, rest: [] });
    expect(describeUndo(undefined)).toBeUndefined();
  });

  it("descarta las entradas más antiguas al llegar al tope", () => {
    let pila: UndoEntry[] = [];
    for (let indice = 0; indice < MAX_UNDO_ENTRIES + 5; indice += 1) {
      pila = pushUndo(pila, edicion(indice));
    }
    expect(pila).toHaveLength(MAX_UNDO_ENTRIES);
    // La más antigua que sobrevive es la número 5: las cinco primeras se fueron.
    expect(pila[0]?.label).toBe("edición 5");
    expect(peekUndo(pila)?.label).toBe(`edición ${MAX_UNDO_ENTRIES + 4}`);
  });

  it("retira las ediciones de juegos que ya no están", () => {
    const pila = pushUndo(pushUndo([], edicion(1)), edicion(2));
    const podada = pruneUndo(pila, new Set([2]));
    expect(podada).toHaveLength(1);
    expect(podada[0]?.label).toBe("edición 2");
  });

  it("no retira los movimientos ni las reordenaciones", () => {
    const pila = pushUndo(pushUndo([], arrastre), {
      kind: "collectionOrder",
      label: "orden de colecciones",
      previous: ["a", "b"],
    });
    expect(pruneUndo(pila, new Set())).toHaveLength(2);
  });

  it("describe la acción en primera persona del producto", () => {
    expect(describeUndo(edicion(1, "ELDEN RING pasó a «Terminados»"))).toBe(
      "Deshacer: ELDEN RING pasó a «Terminados»",
    );
  });
});
