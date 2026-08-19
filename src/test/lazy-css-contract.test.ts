import { readdirSync, readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";

/**
 * Una hoja que llega tarde no puede vestir algo que ya está en pantalla.
 *
 * # El fallo que la motivó
 *
 * El botón del agente vive en el pie desde que arranca la aplicación, pero su
 * CSS estaba en `agent-chat.css`, que sólo se descarga cuando se abre el panel
 * del agente. Al abrir Vindexa el botón salía desbordado, enorme, y se colocaba
 * bien en cuanto se pulsaba —porque entonces, y sólo entonces, llegaba su hoja—.
 *
 * No falla nada. No hay error en consola. El estilo existe, está escrito y es
 * correcto; simplemente no está ahí cuando hace falta.
 *
 * # Qué vigila
 *
 * Una hoja que se carga con su pantalla sólo puede vestir dos cosas: lo suyo
 * —reconocible por el prefijo— o algo que ya tenga estilo base en `index.css`,
 * que se carga desde `main.tsx` y por tanto siempre está. Vestir por primera vez
 * algo de fuera es exactamente el fallo de arriba.
 *
 * Nombrar algo de fuera como **ancestro** sí vale: en
 * `.app-shell[data-density="comfortable"] .rail-block` lo que se viste es
 * `.rail-block`; `.app-shell` sólo dice dónde.
 *
 * # Hasta dónde llega
 *
 * Detecta vestir por primera vez algo ajeno, que es el fallo de arriba. No
 * detecta *afinar* desde una hoja tardía algo que ya tiene base global: eso no
 * rompe la primera pintada, sólo la matiza cuando la hoja llega. Comprobado
 * inyectando las dos cosas: la ajena falla, la afinada no.
 */

interface Hoja {
  /** Prefijos que esta hoja tiene derecho a declarar. */
  readonly prefijos: readonly string[];
  /**
   * Se carga a la vez que el arranque, así que puede vestir lo que quiera.
   * Sólo cuando **todo** lo que viste se monta con ella o después.
   */
  readonly conElArranque?: string;
}

const HOJAS: Record<string, Hoja> = {
  "features/agent/agent-chat.css": { prefijos: ["agent-chat"] },
  "features/collections/collections.css": { prefijos: ["collection-", "collections-"] },
  "features/couch/couch.css": { prefijos: ["couch"] },
  "features/discovery/discovery.css": {
    prefijos: [
      "decision-",
      "discovery-",
      "dismissed-list",
      "panel-heading",
      "radar-",
      "rail-block",
      "reminder-list",
      "rules-panel",
      "signal-",
      "skeleton-",
      "taste-report",
      "upcoming-",
    ],
  },
  "features/library/game-detail.css": {
    prefijos: ["detail-", "dlc-", "game-detail-sheet", "priority-"],
  },
  "features/library/library-dnd.css": {
    prefijos: ["library-", "selection-", "game-browser", "drag-"],
    conElArranque: "LibraryScreen no es diferida: es la pantalla con la que abre la aplicación.",
  },
  "features/library/saved-views.css": {
    prefijos: ["saved-view"],
    conElArranque: "SavedViewsBar se monta dentro de LibraryScreen, que llega con el arranque.",
  },
  "features/notifications/notifications.css": {
    prefijos: ["notification", "rules-panel", "rule-"],
    conElArranque: "NotificationsPopover está en la barra superior desde el primer pintado.",
  },
  "features/planner/planner-advanced.css": { prefijos: ["planner-", "kanban-board"] },
  "features/settings/agents-panel.css": { prefijos: ["agent-", "agents-panel"] },
  "features/settings/stores-panel.css": { prefijos: ["store", "itch-"] },
  "features/shell/command-palette.css": { prefijos: ["command-palette"] },
  "features/wishlist/wishlist.css": {
    prefijos: ["curated-", "game-picker", "video-", "wishlist-"],
  },
};

/** El sujeto de cada selector: la última clase, que es la que se viste. */
function sujetos(css: string): string[] {
  const sinComentarios = css.replace(/\/\*[\s\S]*?\*\//g, "");
  const encontrados = new Set<string>();
  for (const bloque of sinComentarios.split("}")) {
    const abre = bloque.indexOf("{");
    if (abre === -1) continue;
    for (const selector of bloque.slice(0, abre).split(",")) {
      const limpio = selector.trim();
      if (!limpio || limpio.startsWith("@") || limpio.startsWith(":root")) continue;
      const tramo = limpio
        .split(/[\s>+~]+/)
        .filter(Boolean)
        .at(-1);
      const clases = tramo?.match(/\.[A-Za-z0-9_-]+/g);
      if (!clases) continue;
      // Dentro de un mismo tramo, `.a.b` viste ambas: cuentan las dos.
      for (const clase of clases) encontrados.add(clase.slice(1));
    }
  }
  return [...encontrados];
}

/** Clases con estilo base en la hoja global: refinarlas desde fuera es seguro. */
const GLOBALES = new Set(sujetos(readFileSync("src/index.css", "utf8")));

const hojas = Object.keys(HOJAS);

describe("una hoja que llega con su pantalla sólo viste lo suyo", () => {
  it("no queda ninguna hoja sin contemplar", () => {
    // Una hoja nueva entra en la lista de arriba con sus prefijos, o esto falla.
    // Sin esto, el contrato se queda quieto mientras la aplicación crece.
    const enDisco: string[] = [];
    const recorrer = (dir: string) => {
      for (const entrada of readdirSync(`src/${dir}`, { withFileTypes: true })) {
        if (entrada.isDirectory()) recorrer(`${dir}/${entrada.name}`);
        else if (entrada.name.endsWith(".css")) enDisco.push(`${dir}/${entrada.name}`);
      }
    };
    recorrer("features");
    expect(enDisco.sort()).toEqual(hojas.slice().sort());
  });

  it.each(hojas.filter((hoja) => !HOJAS[hoja]?.conElArranque))("%s", (hoja) => {
    const { prefijos } = HOJAS[hoja] as Hoja;
    const intrusos = sujetos(readFileSync(`src/${hoja}`, "utf8")).filter(
      (clase) => !prefijos.some((prefijo) => clase.startsWith(prefijo)) && !GLOBALES.has(clase),
    );
    expect(intrusos).toEqual([]);
  });
});
