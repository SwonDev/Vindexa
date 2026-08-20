import {
  IconAlertTriangle,
  IconCircleCheck,
  IconDownload,
  IconEye,
  IconEyeOff,
  IconLoader2,
  IconPuzzle,
  IconRefresh,
} from "@tabler/icons-react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useState } from "react";
import { MetricStrip } from "@/components/common/MetricStrip";
import {
  AnimatedNumber,
  SegmentedControl,
  type SegmentedControlOption,
  ShimmerSkeleton,
} from "@/components/motion";
import { Button } from "@/components/ui/button";
import { formatDate, initials } from "@/lib/format";
import { storeLabel } from "@/lib/stores";
import { api, getErrorMessage } from "@/lib/tauri";
import type { DlcEvidenceGap, DlcFilter, DlcRefreshReport, DlcSummary, GameDlc } from "@/lib/types";

const FILTERS: readonly SegmentedControlOption<DlcFilter>[] = [
  { value: "visible", label: "Visibles", hint: "Todo lo que no has ocultado." },
  { value: "all", label: "Todos", hint: "Incluye también lo que has ocultado." },
  {
    value: "owned",
    label: "En propiedad",
    hint: "Confirmado por el manifiesto local de Steam o marcado por ti.",
  },
  {
    value: "notOwned",
    label: "Sin confirmar",
    hint: "Sin evidencia de propiedad. No significa que no lo tengas.",
  },
  { value: "installed", label: "Instalados", hint: "Con depósito instalado en este equipo." },
  { value: "hidden", label: "Ocultos", hint: "Lo que has retirado de la lista." },
];

/**
 * Regla de este panel. Es cierta y hay que poder leerla, pero no tiene por qué
 * ocupar el cuerpo: vive en el `title` del encabezado y en texto accesible.
 */
const PANEL_RULE =
  "Vindexa marca lo que puede demostrar y deja el resto en tus manos. Corrige cualquier fila: tu marca manda sobre la comprobación automática.";

/** Por qué «sin confirmar» no equivale a «no lo tienes». */
const UNCONFIRMED_DETAIL =
  "Hay contenido —cosméticos, desbloqueables o licencias sin descargar— que no deja depósito propio en el equipo.";

/**
 * Titular corto de cada hueco de evidencia. La explicación larga la redacta el
 * backend (`LocalDlcEvidenceGap::explanation`) y se muestra tal cual: aquí solo
 * se le pone un encabezado para que se lea de un vistazo.
 */
const EVIDENCE_GAP_HEADLINE: Record<DlcEvidenceGap, string> = {
  dlc_evidence_steam_not_installed: "No hay una instalación de Steam que consultar",
  dlc_evidence_libraries_unreadable: "Las bibliotecas de Steam no se pudieron leer",
  dlc_evidence_game_not_installed: "El juego no está instalado en este equipo",
  dlc_evidence_invalid_app_id: "El AppID de Steam no es válido",
};

/**
 * El resumen lo piden dos sitios: el contador de la pestaña y este panel. Con
 * una ventana de frescura compartida, abrir la pestaña reutiliza lo que ya se
 * leyó en vez de repetir la consulta; cualquier marca manual invalida la clave
 * y fuerza la relectura igualmente.
 */
export const DLC_SUMMARY_STALE_MS = 30_000;

/**
 * Filas dibujadas antes de pedir confirmación. Hay juegos con cientos de DLC y
 * esta lista no está virtualizada: el techo evita que abrir una pestaña cueste
 * medio segundo de pintado.
 */
const ROWS_VISIBLE = 30;

type DlcFlagChange =
  | { kind: "owned"; dlcAppId: number; value: boolean }
  | { kind: "installed"; dlcAppId: number; value: boolean }
  | { kind: "hidden"; dlcAppId: number; value: boolean };

interface Props {
  appId: number;
  title: string;
  /**
   * De qué tienda es, cuando no es de Steam.
   *
   * El catálogo de contenido adicional se le pide a Steam, así que en un juego
   * de Epic o de itch.io el botón pedía un identificador que Vindexa se había
   * inventado. Sin este dato el panel no puede saberlo y ofrece una consulta
   * imposible.
   */
  externalStore?: string | undefined;
}

/**
 * Gestión del contenido adicional de un juego.
 *
 * Dos honestidades gobiernan este panel y no son negociables:
 *
 * 1. **«Sin confirmar» no es «no lo tienes».** La única prueba local es el
 *    manifiesto de Steam, y hay DLC —cosméticos, desbloqueables, licencias sin
 *    descargar— que no dejan depósito propio. Cuando el backend informa de un
 *    hueco de evidencia (`ownershipEvidenceGap`), el motivo exacto se enseña; y
 *    aunque no lo informe, la lista nunca afirma una ausencia.
 * 2. **El importe pendiente no es un total cerrado.** `pendingValueCents` solo
 *    cubre `pendingCounted`. Si quedan DLC sin precio publicado o en otra
 *    moneda, la cifra se presenta como «al menos» y se dice cuántos quedan
 *    fuera de la suma.
 */
export function GameDlcPanel({ appId, title, externalStore }: Props) {
  const queryClient = useQueryClient();
  const [filter, setFilter] = useState<DlcFilter>("visible");
  const [report, setReport] = useState<DlcRefreshReport>();
  const [failedArt, setFailedArt] = useState<readonly number[]>([]);
  const [rowsExpanded, setRowsExpanded] = useState(false);

  const listQuery = useQuery({
    queryKey: ["game-dlc", appId, filter],
    queryFn: () => api.listGameDlc(appId, filter),
    enabled: appId > 0,
  });
  const summaryQuery = useQuery({
    queryKey: ["game-dlc-summary", appId],
    queryFn: () => api.dlcSummary(appId),
    enabled: appId > 0,
    staleTime: DLC_SUMMARY_STALE_MS,
  });
  const invalidate = async () => {
    await queryClient.invalidateQueries({ queryKey: ["game-dlc", appId] });
    await queryClient.invalidateQueries({ queryKey: ["game-dlc-summary", appId] });
  };
  const refreshMutation = useMutation({
    mutationFn: () => api.refreshGameDlc(appId),
    onSuccess: async (nextReport) => {
      setReport(nextReport);
      await invalidate();
    },
  });
  const flagMutation = useMutation({
    mutationFn: (change: DlcFlagChange) => {
      if (change.kind === "owned") return api.setDlcOwned(appId, change.dlcAppId, change.value);
      if (change.kind === "installed") {
        return api.setDlcInstalled(appId, change.dlcAppId, change.value);
      }
      return api.setDlcHidden(appId, change.dlcAppId, change.value);
    },
    onSuccess: invalidate,
  });

  const summary = summaryQuery.data;
  const items = listQuery.data ?? [];
  const knownTotal = summary?.total ?? items.length;
  const visibleItems = rowsExpanded ? items : items.slice(0, ROWS_VISIBLE);

  return (
    <section className="dlc-panel" aria-labelledby="dlc-panel-title">
      <header className="dlc-panel__head">
        <div className="dlc-panel__heading">
          <h3 id="dlc-panel-title" title={PANEL_RULE}>
            <IconPuzzle /> Contenido adicional
          </h3>
          {/* La regla del panel sigue escrita —en el `title` del encabezado y
              aquí para lectores de pantalla—, pero ya no abre la pestaña con
              dos líneas de producto explicándose a sí mismo. */}
          <p className="sr-only">{PANEL_RULE}</p>
        </div>
        <Button
          size="xs"
          variant="secondary"
          disabled={refreshMutation.isPending || Boolean(externalStore)}
          onClick={() => refreshMutation.mutate()}
        >
          {refreshMutation.isPending ? <IconLoader2 className="is-spinning" /> : <IconRefresh />}
          {refreshMutation.isPending ? "Consultando la tienda…" : "Actualizar desde la tienda"}
        </Button>
      </header>

      {externalStore ? (
        <p className="dlc-panel__foreign" role="status">
          El catálogo de contenido adicional lo publica Steam. Este juego es de{" "}
          {storeLabel(externalStore)}: lo que tengas suyo se consulta en su propia tienda. Lo que
          marques aquí a mano se guarda igual.
        </p>
      ) : null}
      {summary ? <DlcCounters summary={summary} /> : null}
      {summary ? <PendingValue summary={summary} /> : null}

      {report?.ownershipEvidenceGap ? (
        <div className="dlc-evidence" data-gap={report.ownershipEvidenceGap} role="status">
          <IconAlertTriangle />
          <div>
            <strong>{EVIDENCE_GAP_HEADLINE[report.ownershipEvidenceGap]}</strong>
            <p>
              {report.ownershipEvidenceExplanation ??
                "Vindexa no pudo comprobar qué contenido adicional posees."}
            </p>
            <p className="dlc-evidence__rule">
              Por eso lo no comprobado aparece como <b>sin confirmar</b>, nunca como ausente.
              Márcalo tú si sabes que lo tienes.
            </p>
            <code>{report.ownershipEvidenceGap}</code>
          </div>
        </div>
      ) : null}

      {report ? <RefreshReceipt report={report} /> : null}
      {refreshMutation.isError ? (
        <p className="dlc-panel__failure" role="alert">
          No se pudo consultar la tienda: {getErrorMessage(refreshMutation.error)}
        </p>
      ) : null}
      {flagMutation.isError ? (
        <p className="dlc-panel__failure" role="alert">
          No se pudo guardar la marca: {getErrorMessage(flagMutation.error)}
        </p>
      ) : null}

      <div className="dlc-filters">
        <SegmentedControl
          size="sm"
          label="Filtro del contenido adicional"
          options={FILTERS}
          value={filter}
          onValueChange={(next) => {
            setFilter(next);
            setRowsExpanded(false);
          }}
        />
        {/* La frase que hace honesto el filtro se queda a la vista; el porqué
            —qué clase de contenido no deja rastro— pasa al `title` y al texto
            accesible en lugar de ocupar tres líneas antes de la lista. */}
        <p className="dlc-filters__legend" title={UNCONFIRMED_DETAIL}>
          <b>Sin confirmar</b> significa que no hay evidencia, no que no lo tengas.
          <span className="sr-only"> {UNCONFIRMED_DETAIL}</span>
        </p>
      </div>

      {listQuery.isPending ? (
        <ShimmerSkeleton count={4} height={46} gapPx={1} label="Cargando el contenido adicional" />
      ) : listQuery.isError ? (
        <div className="dlc-panel__failure" role="alert">
          <span>No se pudo leer el contenido adicional: {getErrorMessage(listQuery.error)}</span>
          <Button size="xs" variant="secondary" onClick={() => void listQuery.refetch()}>
            Reintentar
          </Button>
        </div>
      ) : items.length === 0 ? (
        <p className="dlc-panel__empty">
          {knownTotal === 0
            ? report
              ? "Steam no declara contenido adicional para este juego."
              : "Todavía no se ha consultado la tienda. Actualiza para saber si este juego tiene contenido adicional."
            : "Ningún contenido adicional coincide con este filtro."}
        </p>
      ) : (
        <ul id="dlc-list" className="dlc-list" aria-label={`Contenido adicional de ${title}`}>
          {visibleItems.map((item) => (
            <DlcRow
              key={item.dlcAppId}
              item={item}
              artFailed={failedArt.includes(item.dlcAppId)}
              busy={flagMutation.isPending && flagMutation.variables?.dlcAppId === item.dlcAppId}
              onArtError={() =>
                setFailedArt((failed) =>
                  failed.includes(item.dlcAppId) ? failed : [...failed, item.dlcAppId],
                )
              }
              onChange={(change) => flagMutation.mutate(change)}
            />
          ))}
        </ul>
      )}
      {items.length > ROWS_VISIBLE ? (
        <Button
          size="xs"
          variant="ghost"
          className="dlc-list__more"
          aria-expanded={rowsExpanded}
          aria-controls="dlc-list"
          onClick={() => setRowsExpanded((expanded) => !expanded)}
        >
          {rowsExpanded
            ? "Mostrar menos contenido"
            : `Mostrar ${items.length - ROWS_VISIBLE} contenidos más`}
        </Button>
      ) : null}
    </section>
  );
}

/**
 * Sólo los agregados que llevan a algo.
 *
 * Eran seis para cinco filas, y dos de ellos no respondían a ninguna pregunta:
 * «Gratuitos» no tiene filtro ni decisión detrás, y «Ocultos» es un estado que
 * la persona misma provoca y que ya tiene su propio filtro. Quedan el total
 * declarado —el denominador de todo lo demás— y las tres cifras que el panel
 * usa de verdad: lo que tienes, lo que está en disco y lo que falta.
 */
function DlcCounters({ summary }: { summary: DlcSummary }) {
  const counters: readonly { id: string; label: string; value: number }[] = [
    { id: "total", label: "Declarados", value: summary.total },
    { id: "owned", label: "En propiedad", value: summary.owned },
    { id: "installed", label: "Instalados", value: summary.installed },
    { id: "pending", label: "Pendientes", value: summary.pending },
  ];
  return (
    <MetricStrip
      className="dlc-counters"
      label="Recuento de contenido adicional"
      items={counters.map((counter) => ({
        id: counter.id,
        label: counter.label,
        value: <AnimatedNumber value={counter.value} />,
      }))}
    />
  );
}

/**
 * Importe pendiente, presentado como lo que es.
 *
 * `pendingValueCents` solo suma los DLC de pago, no poseídos, no ocultos y con
 * precio y moneda publicados en la misma divisa. Cuando queda algo fuera, la
 * cifra se anuncia como «al menos» y se dice exactamente cuánto no se contó.
 */
function PendingValue({ summary }: { summary: DlcSummary }) {
  if (summary.pending === 0) {
    return (
      <p className="dlc-pending dlc-pending--settled">
        No queda contenido adicional pendiente en esta lista.
      </p>
    );
  }
  const uncounted: string[] = [];
  if (summary.pendingUnknownPrice > 0) {
    uncounted.push(`${summary.pendingUnknownPrice} sin precio publicado`);
  }
  if (summary.pendingOtherCurrency > 0) {
    uncounted.push(`${summary.pendingOtherCurrency} en otra moneda`);
  }
  const open = uncounted.length > 0;
  // `null` desde Rust y `undefined` por no viajar significan lo mismo: no hay
  // importe. Distinguirlos aquí sólo servía para sumar sobre la nada.
  if (summary.pendingValueCents == null || summary.pendingCounted === 0) {
    const caveat = `Quedan ${summary.pending} pendientes y la tienda no publica un precio comparable para ninguno${
      uncounted.length ? `: ${uncounted.join(" y ")}` : ""
    }.`;
    return (
      <div className="dlc-pending" data-open="true" title={caveat}>
        <span className="dlc-pending__label">Pendiente</span>
        <strong className="dlc-pending__value">Sin importe que sumar</strong>
        <p className="sr-only">{caveat}</p>
      </div>
    );
  }
  const caveat = open
    ? `No es un total cerrado: suma ${summary.pendingCounted} ${
        summary.pendingCounted === 1 ? "contenido" : "contenidos"
      } con precio publicado y deja fuera ${uncounted.join(" y ")}.`
    : `Suma los ${summary.pendingCounted} contenidos pendientes con precio publicado.`;
  return (
    // El «al menos» y el filo de aviso siguen en el cuerpo: son la parte que no
    // puede depender de un `hover`. La aritmética de la salvedad se lee al pasar
    // el cursor y sigue escrita para lectores de pantalla.
    <div className="dlc-pending" data-open={open || undefined} title={caveat}>
      <span className="dlc-pending__label">{open ? "Pendiente: al menos" : "Pendiente"}</span>
      <strong className="dlc-pending__value">
        {formatMoney(summary.pendingValueCents, summary.pendingValueCurrency)}
      </strong>
      <p className="sr-only">{caveat}</p>
    </div>
  );
}

/** Recibo de la última consulta: qué se pidió, qué llegó y qué falta. */
function RefreshReceipt({ report }: { report: DlcRefreshReport }) {
  return (
    <p className="dlc-receipt" role="status">
      Steam declara {report.declared} contenidos. Se consultaron {report.fetchedDetails} fichas y
      quedan {report.pendingDetails} por consultar.
      {report.unavailableDetails > 0 ? ` ${report.unavailableDetails} no están publicadas.` : ""}
      {report.failedDetails > 0 ? ` ${report.failedDetails} fallaron y se reintentarán.` : ""}
      {report.truncated
        ? " La tienda devolvió más de los que caben en una consulta: la lista está recortada."
        : ""}
    </p>
  );
}

function DlcRow({
  item,
  artFailed,
  busy,
  onArtError,
  onChange,
}: {
  item: GameDlc;
  artFailed: boolean;
  busy: boolean;
  onArtError: () => void;
  onChange: (change: DlcFlagChange) => void;
}) {
  const art = item.capsuleUrl ?? item.headerUrl;
  return (
    <li className="dlc-row" data-hidden={item.hidden || undefined}>
      <span className="dlc-row__art">
        {art && !artFailed ? (
          <img src={art} alt="" loading="lazy" decoding="async" onError={onArtError} />
        ) : (
          <span className="dlc-row__art-fallback" aria-hidden="true">
            {initials(item.title)}
          </span>
        )}
      </span>
      <span className="dlc-row__main">
        <strong className="dlc-row__title">{item.title}</strong>
        <span className="dlc-row__meta">
          <span className="dlc-row__app">APP {item.dlcAppId}</span>
          {item.releaseDate ? <span>{formatDate(item.releaseDate)}</span> : null}
          {item.owned ? null : (
            <span
              className="dlc-row__unconfirmed"
              title="Sin evidencia local de propiedad. No significa que no lo tengas."
            >
              Sin confirmar
            </span>
          )}
          {item.hidden ? <span className="dlc-row__flag">Oculto</span> : null}
          {item.metadataStatus === "unavailable" ? (
            <span className="dlc-row__flag">Ficha no publicada</span>
          ) : null}
          {item.metadataStatus === "failed" ? (
            <span className="dlc-row__flag">Ficha sin conexión</span>
          ) : null}
        </span>
      </span>
      <span className="dlc-row__price">
        {item.isFree ? (
          "Gratuito"
        ) : item.priceCents == null ? (
          <span className="dlc-row__price-unknown">Sin precio publicado</span>
        ) : (
          <>
            {formatMoney(item.priceCents, item.currency)}
            {item.discountPercent ? (
              <span className="dlc-row__discount">−{item.discountPercent}%</span>
            ) : null}
          </>
        )}
      </span>
      <span className="dlc-row__controls">
        <Button
          size="xs"
          variant="ghost"
          className="dlc-flag-button"
          data-on={item.owned || undefined}
          aria-pressed={item.owned}
          aria-label={`En propiedad — ${item.title}`}
          disabled={busy}
          onClick={() => onChange({ kind: "owned", dlcAppId: item.dlcAppId, value: !item.owned })}
        >
          <IconCircleCheck /> En propiedad
        </Button>
        <Button
          size="xs"
          variant="ghost"
          className="dlc-flag-button"
          data-on={item.installed || undefined}
          aria-pressed={item.installed}
          aria-label={`Instalado — ${item.title}`}
          disabled={busy}
          onClick={() =>
            onChange({ kind: "installed", dlcAppId: item.dlcAppId, value: !item.installed })
          }
        >
          <IconDownload /> Instalado
        </Button>
        <Button
          size="xs"
          variant="ghost"
          className="dlc-flag-button"
          aria-label={`${item.hidden ? "Mostrar" : "Ocultar"} — ${item.title}`}
          disabled={busy}
          onClick={() => onChange({ kind: "hidden", dlcAppId: item.dlcAppId, value: !item.hidden })}
        >
          {item.hidden ? <IconEye /> : <IconEyeOff />}
          {item.hidden ? "Mostrar" : "Ocultar"}
        </Button>
      </span>
    </li>
  );
}

/**
 * Importe en la moneda que publica la tienda. Si el código no es válido para
 * `Intl`, se enseña la cifra con el código tal cual en vez de inventar un
 * símbolo que no corresponde.
 */
function formatMoney(cents: number, currency?: string): string {
  const amount = cents / 100;
  const plain = `${amount.toFixed(2).replace(".", ",")}${currency ? ` ${currency}` : ""}`;
  if (!currency) return plain;
  try {
    return new Intl.NumberFormat("es-ES", { style: "currency", currency }).format(amount);
  } catch {
    return plain;
  }
}
