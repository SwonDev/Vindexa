/**
 * Completa la caché de arte mientras la aplicación no hace nada.
 *
 * # Por qué hace falta
 *
 * La rejilla ya adelanta las carátulas de la página cargada, pero eso sólo
 * cubre lo que se ha llegado a mirar. En una biblioteca de miles de juegos, el
 * resto se descarga al desplazarse hasta él: la primera vez que llegas a la
 * letra T, esas carátulas aparecen con retraso y, sin conexión, no aparecen.
 *
 * Aquí se completa el resto poco a poco, en los ratos en que el navegador está
 * ocioso. Al cabo de un rato con la aplicación abierta, la biblioteca entera se
 * pinta desde el disco.
 *
 * # Por qué no compite con lo que estás mirando
 *
 * - Sólo arranca cuando el navegador declara que está libre
 *   (`requestIdleCallback`), y se detiene en cuanto deja de estarlo.
 * - Va por tandas pequeñas, con la misma cola de peticiones que usa la rejilla:
 *   lo que pidas tú entra en la misma fila y no se queda detrás de mil.
 * - Lo ya resuelto no se vuelve a pedir, así que la segunda vuelta no cuesta
 *   nada.
 *
 * # Qué lo frena
 *
 * Por defecto la caché no tiene techo, así que lo normal es que esto recorra la
 * biblioteca entera y pare al terminarla. Sólo hay dos motivos para detenerse
 * antes, y ninguno es un número inventado:
 *
 * - Alguien fijó un techo a mano por debajo de lo que su biblioteca necesita.
 *   Seguir adelantando ahí provocaría un tira y afloja con el desalojo —se
 *   descarga, se llena, se borra, se vuelve a descargar— que gasta red y disco
 *   sin dejar nada. Comprobado sobre una biblioteca real cuando el techo de
 *   fábrica eran 512 MiB: la caché **bajaba** de tamaño mientras esto corría.
 * - El disco se está quedando sin sitio. Ese límite es físico y no se negocia:
 *   quien se queda sin espacio no es Vindexa, es el sistema entero.
 */

import { useEffect } from "react";
import { prefetchArtwork } from "@/components/common/Artwork";
import { api } from "@/lib/tauri";

/** Carátulas por tanda. Suficiente para avanzar, poco para estorbar. */
const BATCH = 24;

/** Espera entre tandas cuando el navegador no ofrece un hueco de reposo. */
const FALLBACK_DELAY_MS = 1_500;

/**
 * Parte del techo que puede ocupar lo adelantado.
 *
 * El margen que queda es para lo que se pide al mirar, que siempre va primero:
 * una precarga no puede dejar sin sitio a la carátula que estás mirando.
 */
const FILL_LIMIT = 0.95;

/** Cada cuántas tandas se vuelve a medir el disco. Medirlo cuesta recorrerlo. */
const CHECK_EVERY = 5;

type IdleWindow = Window & {
  requestIdleCallback?: (callback: () => void, options?: { timeout: number }) => number;
  cancelIdleCallback?: (handle: number) => void;
};

/**
 * @param enabled Se apaga en las pantallas donde la biblioteca no se ve, para
 * no descargar arte que nadie va a mirar ahora mismo.
 */
export function useIdleArtworkPrefetch(enabled: boolean): void {
  useEffect(() => {
    if (!enabled) return;
    const view = window as IdleWindow;
    let cancelled = false;
    let handle: number | undefined;
    let timer: number | undefined;

    const schedule = (run: () => void) => {
      if (view.requestIdleCallback) {
        // El plazo evita que una aplicación siempre ocupada deje esto sin
        // ejecutar nunca.
        handle = view.requestIdleCallback(run, { timeout: 4_000 });
      } else {
        timer = window.setTimeout(run, FALLBACK_DELAY_MS);
      }
    };

    void (async () => {
      let targets: { appId: number; coverUrl: string }[];
      try {
        targets = await api.listArtworkTargets();
      } catch {
        // Sin lista no hay nada que precargar. No es un error que merezca
        // molestar: la rejilla sigue resolviendo lo suyo al desplazarse.
        return;
      }
      if (cancelled) return;

      let index = 0;
      let batches = 0;
      const step = () => {
        if (cancelled || index >= targets.length) return;
        void (async () => {
          if (batches % CHECK_EVERY === 0) {
            try {
              const usage = await api.getArtCacheUsage();
              if (usage.budgetBytes !== null && usage.bytes >= usage.budgetBytes * FILL_LIMIT) {
                return;
              }
              // `null` es «no se sabe», y adelantar a ciegas contra el disco no
              // se hace: se para igual que si estuviera lleno.
              if (usage.freeDiskBytes === null || usage.freeDiskBytes <= usage.minFreeDiskBytes) {
                return;
              }
            } catch {
              // Sin la medida no se sigue: adelantar a ciegas es lo que
              // provocaba el tira y afloja con el desalojo.
              return;
            }
          }
          if (cancelled) return;
          batches += 1;
          const batch = targets.slice(index, index + BATCH);
          index += BATCH;
          prefetchArtwork(
            batch.map((target) => ({ appId: target.appId, src: target.coverUrl })),
            "cover",
          );
          schedule(step);
        })();
      };
      schedule(step);
    })();

    return () => {
      cancelled = true;
      if (handle !== undefined) view.cancelIdleCallback?.(handle);
      if (timer !== undefined) window.clearTimeout(timer);
    };
  }, [enabled]);
}
