import {
  IconBrandSteam,
  IconBrowser,
  IconHeartPlus,
  IconListDetails,
  IconListNumbers,
} from "@tabler/icons-react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useMemo, useState } from "react";
import { PageHeader } from "@/components/common/PageHeader";
import { SegmentedControl } from "@/components/motion";
import { Button } from "@/components/ui/button";
import {
  type BrowserWishlistImportResult,
  importWishlistFromBrowser,
  suggestsBrowserImport,
} from "@/features/wishlist/browser-import";
import { CuratedListsPanel } from "@/features/wishlist/CuratedListsPanel";
import { WishlistBoard } from "@/features/wishlist/WishlistBoard";
import { summarizeTargets } from "@/features/wishlist/wishlist-model";
import { api, getErrorMessage } from "@/lib/tauri";
import type { AppBootstrap, SteamWishlistImportResult, WishlistImportReport } from "@/lib/types";
import "@/features/wishlist/wishlist.css";

type WishlistView = "list" | "buckets" | "curated";

/**
 * Aviso de la pantalla.
 *
 * `offerBrowser` no es decoración: es el único momento en el que ofrecer la
 * importación desde el navegador significa algo, porque acaba de fallar la otra
 * por un perfil que no es público. Fuera de ahí el botón de la cabecera ya está
 * disponible y repetirlo sólo añadiría ruido.
 */
interface WishlistNotice {
  text: string;
  offerBrowser?: boolean;
}

const VIEW_OPTIONS = [
  /* La lista va primero: con una importación de Steam encima, el tablero por
     intención deja tres carriles vacíos y uno con mil cuatrocientos. */
  { value: "list" as const, label: "Lista", icon: <IconListNumbers aria-hidden="true" /> },
  { value: "buckets" as const, label: "Por intención", icon: <IconHeartPlus aria-hidden="true" /> },
  {
    value: "curated" as const,
    label: "Listas curadas",
    icon: <IconListDetails aria-hidden="true" />,
  },
];

/**
 * Sumandos comunes a las dos importaciones.
 *
 * Los dos caminos escriben en la misma lista y devuelven el mismo recuento, así
 * que la frase se construye una sola vez: lo único que cambia es de dónde salió
 * la lista y qué extras puede añadir cada uno.
 */
function reportParts(report: WishlistImportReport): string[] {
  const { fetched, imported, alreadyPresent, skipped, limitReached } = report;
  const unnamed = skipped.filter((game) => game.reason === "unresolved_title").length;
  const parts = [`${fetched} en Steam`, `${imported} nuevos`, `${alreadyPresent} ya estaban`];
  if (unnamed > 0) parts.push(`${unnamed} se quedaron fuera por no tener nombre`);
  if (limitReached) parts.push("se alcanzó el límite de deseados");
  return parts;
}

/**
 * Cuenta lo que ha pasado al importar, sin redondear a «importado ✓».
 *
 * Los sumandos vuelven a dar el total que devolvió Steam, para que quien lo lea
 * pueda cuadrarlo sin fiarse de un único número.
 */
function importSummary(result: SteamWishlistImportResult): string {
  if (result.report.fetched === 0) {
    return result.visibilityUnknown
      ? "Steam devolvió una lista vacía. Si la tuya no lo está, comprueba que tu perfil y «Detalles del juego» sean públicos, o impórtala desde el navegador."
      : "Steam devolvió una lista vacía.";
  }
  const parts = reportParts(result.report);
  if (result.titlesUnresolved > 0) {
    parts.push(`${result.titlesUnresolved} sin nombre en la tienda`);
  }
  return `${parts.join(" · ")}.`;
}

/**
 * Lo mismo para la lectura desde el navegador, más lo único que ese camino
 * puede saber y el otro no: cuántos juegos esconden los filtros que la propia
 * lista de Steam tiene guardados. Sin ese dato la cifra parecería completa
 * cuando no lo es.
 */
function browserImportSummary(result: BrowserWishlistImportResult): string {
  if (result.report.fetched === 0) {
    return "Tu lista de deseados de Steam se abrió, pero no traía ningún juego.";
  }
  const parts = reportParts(result.report);
  if (result.titlesUnresolved > 0) {
    parts.push(`${result.titlesUnresolved} sin nombre en la tienda`);
  }
  if (result.hiddenByFilters > 0) {
    parts.push(`${result.hiddenByFilters} los esconden los filtros de tu lista en Steam`);
  }
  return `${parts.join(" · ")}.`;
}

/**
 * Pantalla de Deseados.
 *
 * Dos mitades que conviven porque responden a dos preguntas distintas sobre lo
 * que todavía no tienes: **cuándo lo compro** —los cuatro cubos de intención,
 * con su precio objetivo y los vídeos que te ayudaron a decidir— y **qué
 * defiendo** —las listas curadas, selecciones editoriales con orden y nota—.
 * Comparten sitio en vez de vivir en dos secciones porque el material es el
 * mismo y la persona salta de una a otra mientras decide.
 */
export function WishlistScreen({ loading }: { bootstrap?: AppBootstrap; loading?: boolean }) {
  /**
   * La vista elegida a mano, si se ha elegido.
   *
   * Sin elección, la decide el tamaño: el tablero por intención se lee de un
   * vistazo con veinte juegos y se vuelve inservible con mil cuatrocientos —tres
   * carriles vacíos y uno sin fondo—. Elegir por quien mira, y dejarle cambiar,
   * es mejor que obligar a cambiar cada vez.
   */
  const [viewOverride, setViewOverride] = useState<WishlistView>();
  const [notice, setNotice] = useState<WishlistNotice>();
  const queryClient = useQueryClient();

  const overview = useQuery({
    queryKey: ["wishlist-overview"],
    queryFn: api.wishlistOverview,
  });

  const refreshOverview = () => {
    void queryClient.invalidateQueries({ queryKey: ["wishlist-overview"] });
  };

  const importFromSteam = useMutation({
    mutationFn: api.importSteamWishlist,
    onSuccess: (result) => {
      setNotice({
        text: importSummary(result),
        // Una lista vacía con la visibilidad en duda es exactamente el caso que
        // el navegador sí puede resolver: allí la lista la sirve Steam a quien
        // ha iniciado sesión, y la privacidad del perfil deja de estorbar.
        offerBrowser: result.report.fetched === 0 && result.visibilityUnknown,
      });
      refreshOverview();
    },
    onError: (cause) =>
      setNotice({ text: getErrorMessage(cause), offerBrowser: suggestsBrowserImport(cause) }),
  });

  const importFromBrowser = useMutation({
    mutationFn: importWishlistFromBrowser,
    onSuccess: (result) => {
      setNotice({ text: browserImportSummary(result) });
      refreshOverview();
    },
    onError: (cause) => setNotice({ text: getErrorMessage(cause) }),
  });

  const importing = importFromSteam.isPending || importFromBrowser.isPending;

  const summary = useMemo(() => summarizeTargets(overview.data), [overview.data]);
  /**
   * A partir de aquí el tablero deja de servir.
   *
   * Sesenta juegos son unas quince tarjetas por carril: todavía se recorre. Con
   * ciento cincuenta ya no, y una importación de Steam trae mil cuatrocientos.
   */
  const LISTA_DESDE = 60;
  const view: WishlistView =
    viewOverride ?? ((overview.data?.total ?? 0) > LISTA_DESDE ? "list" : "buckets");
  /*
   * La aritmética completa —por qué la cifra es un suelo y qué aporta cada
   * moneda— deja de ocupar dos párrafos del encabezado y pasa al `title` de la
   * cifra, duplicada en texto para lectores de pantalla. El dato no se pierde
   * ni se suaviza: lo que cambia es su peso visual. El «Al menos» sigue delante
   * del número, que es la parte que no puede depender de un `hover`.
   */
  const figureDetail = useMemo(() => {
    const breakdown =
      summary.currencies.length > 1
        ? summary.currencies
            .map(
              (entry) =>
                `${entry.amount} en ${entry.entries.toLocaleString("es-ES")} ${
                  entry.entries === 1 ? "entrada" : "entradas"
                }`,
            )
            .join("; ")
        : "";
    return [summary.caveat, breakdown].filter(Boolean).join(" ");
  }, [summary]);

  return (
    <section className="wishlist-screen" data-layout="split">
      <PageHeader
        eyebrow="ANTES DE COMPRAR"
        title="Deseados"
        meta={
          /* Con la lista vacía no hay cifra que dar: el estado vacío del
             tablero es el que explica para qué sirve cada carril. */
          (overview.data?.total ?? 0) > 0 ? (
            <div className="wishlist-heading__meta">
              <b
                className="wishlist-heading__figure"
                data-at-least={summary.atLeast}
                title={figureDetail || undefined}
              >
                {summary.headline}
              </b>
              {figureDetail && <span className="sr-only">{figureDetail}</span>}
            </div>
          ) : null
        }
        actions={
          <>
            <Button
              variant="outline"
              size="sm"
              disabled={importing}
              title="Pregunta tu lista a la API pública de Steam. Necesita que tu perfil sea público."
              onClick={() => importFromSteam.mutate()}
            >
              <IconBrandSteam aria-hidden="true" />
              {importFromSteam.isPending ? "Importando…" : "Importar de Steam"}
            </Button>
            <Button
              variant="outline"
              size="sm"
              disabled={importing}
              title="Abre tu lista en el navegador integrado y la lee de tu propia sesión. Funciona con el perfil cerrado."
              onClick={() => importFromBrowser.mutate()}
            >
              <IconBrowser aria-hidden="true" />
              {importFromBrowser.isPending ? "Leyendo el navegador…" : "Desde el navegador"}
            </Button>
            <SegmentedControl
              label="Vista de deseados"
              options={VIEW_OPTIONS}
              value={view}
              onValueChange={setViewOverride}
            />
          </>
        }
      />

      {notice && (
        <div
          className="wishlist-screen__notice"
          role="status"
          aria-label="Resultado de la última importación"
        >
          <p className="wishlist-screen__notice-text">{notice.text}</p>
          {notice.offerBrowser && (
            <Button
              variant="outline"
              size="sm"
              disabled={importing}
              onClick={() => importFromBrowser.mutate()}
            >
              <IconBrowser aria-hidden="true" />
              Importar desde el navegador
            </Button>
          )}
        </div>
      )}

      <div className="wishlist-workspace">
        {view === "curated" ? (
          <CuratedListsPanel />
        ) : (
          <WishlistBoard
            overview={overview.data}
            pending={overview.isPending || Boolean(loading)}
            error={overview.error}
            onRetry={() => void overview.refetch()}
            layout={view}
          />
        )}
      </div>
    </section>
  );
}
