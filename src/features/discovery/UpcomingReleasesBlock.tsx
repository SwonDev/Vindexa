import {
  IconBellPlus,
  IconCalendarQuestion,
  IconLoader2,
  IconLock,
  IconRocket,
  IconThumbDown,
  IconThumbUp,
  IconX,
} from "@tabler/icons-react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useMemo, useState } from "react";
import { Artwork } from "@/components/common/Artwork";
import { GamePreviewCard } from "@/components/common/GamePreviewCard";
import { AnimatedNumber, ExpandableSection, RevealOnScroll } from "@/components/motion";
import { Button } from "@/components/ui/button";
import {
  ContextMenu,
  ContextMenuContent,
  ContextMenuItem,
  ContextMenuLabel,
  ContextMenuSeparator,
  ContextMenuTrigger,
} from "@/components/ui/context-menu";
import {
  NotificationRuleDialog,
  type RulePrefill,
} from "@/features/notifications/NotificationRuleDialog";
import { releaseDateToLocalInput } from "@/features/notifications/rule-form";
import { formatDate } from "@/lib/format";
import { api, getErrorMessage } from "@/lib/tauri";
import type { TasteReport, TasteVerdict, UpcomingRelease } from "@/lib/types";
import "./discovery.css";

/** Cuántos candidatos se piden. El backend ordena por coincidencia y fecha. */
const UPCOMING_LIMIT = 12;

const weightFormatter = new Intl.NumberFormat("es-ES", {
  minimumFractionDigits: 2,
  maximumFractionDigits: 2,
  signDisplay: "exceptZero",
});

interface MatchCopy {
  percent: number;
  /** Banda legible. No es una probabilidad: es dónde cae la coincidencia. */
  band: string;
  level: "none" | "low" | "medium" | "high";
}

/**
 * Traduce `matchScore` (0–1) a algo que se pueda leer de un vistazo.
 *
 * La banda no promete acierto: dice cuánta de la huella del candidato coincide
 * con el modelo. Por eso el porcentaje nunca viaja solo — siempre lo acompaña
 * `matchReason`, que nombra las facetas concretas.
 */
export function describeMatch(score: number): MatchCopy {
  const bounded = Number.isFinite(score) ? Math.min(Math.max(score, 0), 1) : 0;
  const percent = Math.round(bounded * 100);
  if (percent === 0) return { percent, band: "Sin coincidencia medida", level: "none" };
  if (bounded >= 0.6) return { percent, band: "Coincidencia alta", level: "high" };
  if (bounded >= 0.3) return { percent, band: "Coincidencia media", level: "medium" };
  return { percent, band: "Coincidencia baja", level: "low" };
}

export interface ReleaseDateCopy {
  label: string;
  /** Solo se emite `<time dateTime>` cuando la fecha es una fecha de verdad. */
  machine: string | null;
  state: "exact" | "approximate" | "unknown";
}

/**
 * Una fecha aproximada tiene que **verse** aproximada.
 *
 * Cuando `releaseDateIsExact` es falso, el backend guarda la etiqueta tal cual
 * la publica la tienda («Q4 2026», «Próximamente»). Ni se formatea ni se
 * convierte: reescribirla como día concreto sería inventar la precisión que la
 * fuente no da.
 */
export function describeReleaseDate(item: UpcomingRelease): ReleaseDateCopy {
  const raw = item.releaseDate?.trim();
  if (!raw) return { label: "Sin fecha anunciada", machine: null, state: "unknown" };
  if (item.releaseDateIsExact) {
    return { label: formatDate(raw), machine: raw, state: "exact" };
  }
  return { label: raw, machine: null, state: "approximate" };
}

function prefillFromRelease(item: UpcomingRelease): RulePrefill {
  const date = describeReleaseDate(item);
  const scheduled =
    date.state === "exact" && item.releaseDate ? releaseDateToLocalInput(item.releaseDate) : "";
  const body =
    date.state === "exact"
      ? `Lanzamiento anunciado para el ${date.label}.`
      : date.state === "approximate"
        ? `La tienda solo publica «${date.label}»: elige tú el día del aviso.`
        : "Todavía no hay fecha publicada: elige tú el día del aviso.";
  return {
    title: `Sale «${item.title}»`,
    body,
    ...(scheduled ? { scheduledForLocal: scheduled } : {}),
  };
}

/**
 * Próximos lanzamientos puntuados contra el modelo local de gustos.
 *
 * El bloque abre con la respuesta —qué llega, cuánto coincide y por qué— y deja
 * el aprendizaje explícito: responder guarda la señal, pero el modelo no cambia
 * hasta que se recalcula. No hay ninguna cifra sin su procedencia.
 */
export function UpcomingReleasesBlock() {
  const queryClient = useQueryClient();
  const [report, setReport] = useState<TasteReport | null>(null);
  const [scored, setScored] = useState<number | null>(null);
  const [verdicts, setVerdicts] = useState<Record<number, TasteVerdict>>({});
  const [announcement, setAnnouncement] = useState("");
  const [rulePrefill, setRulePrefill] = useState<RulePrefill | null>(null);

  const upcoming = useQuery({
    queryKey: ["upcoming-releases"],
    queryFn: () => api.upcomingReleases(UPCOMING_LIMIT),
  });

  const refreshList = () => queryClient.invalidateQueries({ queryKey: ["upcoming-releases"] });

  /**
   * Recalcular es aprender y después puntuar, en ese orden: `scoreUpcoming` lee
   * los pesos que `learnTaste` acaba de reconstruir. Al revés se puntuaría
   * contra el modelo anterior.
   */
  const recompute = useMutation({
    mutationFn: async () => {
      // Primero se traen candidatos, luego se aprende y por último se puntúa.
      // Sin el primer paso no hay nada que puntuar: los candidatos salen de tu
      // lista de deseados, y hasta que no se revisan no se sabe cuáles siguen
      // sin publicarse. Va por tandas porque cada ficha cuesta una petición.
      const brought = await api.refreshUpcomingReleases();
      const learned = await api.learnTaste();
      const count = await api.scoreUpcomingReleases();
      return { brought, learned, count };
    },
    onSuccess: async ({ brought, learned, count }) => {
      setReport(learned);
      setScored(count);
      const pendientes =
        brought.pending > 0
          ? ` Quedan ${brought.pending.toLocaleString("es-ES")} deseados por revisar: vuelve a recalcular para seguir.`
          : "";
      setAnnouncement(
        `Revisados ${brought.checked} deseados, ${brought.upcoming} siguen sin salir. Modelo recalculado con ${learned.gamesAnalyzed} juegos y ${learned.facetsLearned} facetas. ${count} candidatos vueltos a puntuar.${pendientes}`,
      );
      await refreshList();
    },
    onError: (cause) => setAnnouncement(`No se pudo recalcular: ${getErrorMessage(cause)}`),
  });

  const feedback = useMutation({
    mutationFn: ({ appId, verdict }: { appId: number; verdict: TasteVerdict }) =>
      api.recordTasteFeedback(appId, verdict, "upcoming"),
    onSuccess: async (_result, { appId, verdict }) => {
      setVerdicts((current) => ({ ...current, [appId]: verdict }));
      setAnnouncement(
        verdict === "interested"
          ? "Anotado como interesante. Contará al recalcular el modelo."
          : "Anotado. El candidato deja la lista y sus rasgos restarán al recalcular.",
      );
      await refreshList();
    },
    onError: (cause) => setAnnouncement(`No se pudo anotar: ${getErrorMessage(cause)}`),
  });

  const dismiss = useMutation({
    mutationFn: (appId: number) => api.dismissUpcomingRelease(appId),
    onSuccess: async () => {
      setAnnouncement("Candidato retirado de la lista.");
      await refreshList();
    },
    onError: (cause) => setAnnouncement(`No se pudo descartar: ${getErrorMessage(cause)}`),
  });

  const items = upcoming.data ?? [];
  const busy = feedback.isPending || dismiss.isPending;
  const status = upcoming.isPending
    ? "Leyendo candidatos guardados"
    : upcoming.isError
      ? "No se pudieron leer"
      : items.length === 0
        ? "Sin candidatos guardados"
        : `${items.length} candidatos puntuados`;

  const highlights = useMemo(() => report?.highlights.slice(0, 6) ?? [], [report]);

  return (
    <section className="upcoming-block" aria-labelledby="upcoming-block-title">
      <span className="sr-only" role="status" aria-live="polite">
        {announcement}
      </span>

      <header className="upcoming-block__head">
        <IconRocket aria-hidden="true" />
        <div className="upcoming-block__identity">
          <h2 id="upcoming-block-title">Próximos lanzamientos para ti</h2>
          <span aria-live="polite">{status}</span>
        </div>
        <Button
          size="xs"
          variant="outline"
          disabled={recompute.isPending}
          onClick={() => recompute.mutate()}
        >
          {recompute.isPending ? <IconLoader2 className="is-spinning" /> : <IconRocket />}
          Recalcular
        </Button>
      </header>

      {/* El dato importa y no cambia nunca, así que no necesita tres líneas
          fijas encima de la lista: se dice en una, y la frase entera está a un
          puntero de distancia. Lo que ocupa sitio permanente es lo que cambia. */}
      <p
        className="upcoming-block__privacy"
        title="El modelo de gustos se calcula íntegramente en tu equipo, a partir de tu biblioteca y de tus respuestas. No sale de aquí ni se envía a ningún servidor."
      >
        <IconLock aria-hidden="true" />
        Se calcula en tu equipo; no sale de aquí.
      </p>

      {report && (
        <ExpandableSection
          className="taste-report"
          defaultOpen
          title="Qué aprendió el modelo"
          headerExtra={
            <span className="taste-report__stamp">
              {scored === null ? null : (
                <>
                  <AnimatedNumber value={scored} /> puntuados
                </>
              )}
            </span>
          }
        >
          <dl className="taste-report__figures">
            <div>
              <dt>Juegos analizados</dt>
              <dd>
                <AnimatedNumber value={report.gamesAnalyzed} />
              </dd>
            </div>
            <div>
              <dt>Facetas aprendidas</dt>
              <dd>
                <AnimatedNumber value={report.facetsLearned} />
              </dd>
            </div>
            <div>
              <dt>A favor</dt>
              <dd>
                <AnimatedNumber value={report.positiveFacets} />
              </dd>
            </div>
            <div>
              <dt>En contra</dt>
              <dd>
                <AnimatedNumber value={report.negativeFacets} />
              </dd>
            </div>
          </dl>
          {highlights.length ? (
            <ul className="taste-report__facets">
              {highlights.map((facet) => (
                <li key={`${facet.facet}-${facet.value}`} data-sign={facet.weight >= 0 ? "+" : "−"}>
                  <span className="taste-report__facet">
                    <strong>{facet.value}</strong>
                    <small>{facet.facetLabel}</small>
                  </span>
                  <span className="taste-report__weight">
                    {weightFormatter.format(facet.weight)}
                  </span>
                  <span className="taste-report__samples">
                    {facet.positiveSamples} a favor · {facet.negativeSamples} en contra
                  </span>
                </li>
              ))}
            </ul>
          ) : (
            <p className="upcoming-block__note">
              Ninguna faceta alcanzó peso suficiente para guardarse. Con más horas registradas y más
              respuestas, el modelo tendrá de dónde tirar.
            </p>
          )}
          <p className="upcoming-block__note">
            El peso es lo que empuja cada faceta: el signo dice si suma o resta y la magnitud,
            cuánto. Sale de tus horas, tus estados y tus descartes, nada más. Recalculado el{" "}
            {formatDate(report.computedAt)}.
          </p>
        </ExpandableSection>
      )}

      {upcoming.isPending ? (
        <p className="upcoming-block__note" role="status">
          Leyendo los candidatos guardados en tu equipo…
        </p>
      ) : upcoming.isError ? (
        <div className="upcoming-block__error" role="alert">
          <span>{getErrorMessage(upcoming.error)}</span>
          <Button
            size="xs"
            variant="outline"
            aria-label="Reintentar la lectura de candidatos"
            onClick={() => void upcoming.refetch()}
          >
            Reintentar
          </Button>
        </div>
      ) : items.length === 0 ? (
        <div className="upcoming-empty">
          <IconCalendarQuestion aria-hidden="true" />
          <strong>Todavía no hay ningún candidato que puntuar</strong>
          <p className="upcoming-empty__copy">
            Los candidatos entran al importar datos de la tienda de Steam. Mientras no haya ninguno
            guardado, aquí no aparece nada: Vindexa no rellena esta lista con títulos inventados ni
            con recomendaciones de terceros.
          </p>
          <p className="upcoming-empty__copy">
            El modelo de gustos sí se puede entrenar desde ya con tu biblioteca: «Recalcular» lo
            reconstruye y te dice qué facetas encontró.
          </p>
        </div>
      ) : (
        <ul className="upcoming-list">
          {items.map((item, index) => (
            <UpcomingRow
              key={item.appId}
              item={item}
              index={index}
              busy={busy}
              verdict={verdicts[item.appId]}
              onVerdict={(verdict) => feedback.mutate({ appId: item.appId, verdict })}
              onDismiss={() => dismiss.mutate(item.appId)}
              onSchedule={() => setRulePrefill(prefillFromRelease(item))}
            />
          ))}
        </ul>
      )}

      {items.length > 0 && (
        <p className="upcoming-block__note">
          Responder guarda la señal al momento, pero el modelo no cambia hasta que pulses
          «Recalcular». No hay nada mágico detrás: son facetas contadas contra tus horas.
        </p>
      )}

      <NotificationRuleDialog
        open={rulePrefill !== null}
        onOpenChange={(open) => {
          if (!open) setRulePrefill(null);
        }}
        {...(rulePrefill ? { prefill: rulePrefill } : {})}
        onSaved={(saved) => setAnnouncement(`Aviso «${saved.title}» programado.`)}
      />
    </section>
  );
}

function UpcomingRow({
  item,
  index,
  busy,
  verdict,
  onVerdict,
  onDismiss,
  onSchedule,
}: {
  item: UpcomingRelease;
  index: number;
  busy: boolean;
  verdict: TasteVerdict | undefined;
  onVerdict: (verdict: TasteVerdict) => void;
  onDismiss: () => void;
  onSchedule: () => void;
}) {
  const match = describeMatch(item.matchScore);
  const date = describeReleaseDate(item);
  const reason = item.matchReason.trim();
  const studio = item.developer ?? item.publisher;

  return (
    <ContextMenu>
      <ContextMenuTrigger asChild>
        {/* Un lanzamiento previsto es justo aquello de lo que no se sabe nada
            todavía: sus capturas al detenerse valen más que en cualquier otra
            lista. */}
        <GamePreviewCard
          side="bottom"
          appId={item.appId}
          title={item.title}
          fallback={
            <Artwork
              appId={item.appId}
              src={item.capsuleUrl ?? item.headerUrl ?? undefined}
              title={item.title}
              kind="cover"
            />
          }
          facts={[
            { label: "Sale", value: date.label },
            ...(studio ? [{ label: "Estudio", value: studio }] : []),
            { label: "Coincidencia", value: `${match.band} · ${match.percent} %` },
          ]}
        >
          <RevealOnScroll asChild delayMs={Math.min(index * 24, 160)}>
            <li className="upcoming-row" data-verdict={verdict ?? "none"}>
              <Artwork
                appId={item.appId}
                src={item.capsuleUrl ?? item.headerUrl ?? undefined}
                title={item.title}
                kind="cover"
                className="upcoming-row__art"
              />
              <div className="upcoming-row__body">
                <p className="upcoming-row__title">{item.title}</p>
                <p className="upcoming-row__meta">
                  {date.state === "exact" && date.machine ? (
                    <time className="upcoming-date" data-state="exact" dateTime={date.machine}>
                      {date.label}
                    </time>
                  ) : (
                    <span
                      className="upcoming-date"
                      data-state={date.state}
                      title={
                        date.state === "approximate"
                          ? "Fecha aproximada: es la etiqueta que publica la tienda, no un día concreto."
                          : "La tienda todavía no ha publicado ninguna fecha."
                      }
                    >
                      {date.state === "approximate" ? `≈ ${date.label}` : date.label}
                    </span>
                  )}
                  {studio ? <span className="upcoming-row__studio">{studio}</span> : null}
                </p>

                {/* Una puntuación sin su razón no se pinta: sería una cifra sin
              procedencia, que es justo lo que esta pantalla no hace. */}
                {reason ? (
                  <div className="upcoming-match">
                    <span
                      className="upcoming-match__meter"
                      data-level={match.level}
                      role="img"
                      aria-label={`${match.band}: ${match.percent} %`}
                    >
                      {[0, 1, 2, 3, 4].map((step) => (
                        <span key={step} data-on={match.percent > step * 20} />
                      ))}
                    </span>
                    <span className="upcoming-match__value">
                      {match.band} · {match.percent} %
                    </span>
                    <p className="upcoming-match__reason">{reason}</p>
                  </div>
                ) : (
                  <p className="upcoming-match__reason">
                    Sin razón registrada: no se muestra una coincidencia que no se pueda explicar.
                    Recalcula para volver a puntuarlo.
                  </p>
                )}

                <div className="upcoming-actions">
                  <Button
                    size="xs"
                    variant="outline"
                    disabled={busy}
                    title="Lo mantiene en la lista y cuenta a favor de sus rasgos al recalcular."
                    onClick={() => onVerdict("interested")}
                  >
                    <IconThumbUp /> Me interesa
                  </Button>
                  <Button
                    size="xs"
                    variant="outline"
                    disabled={busy}
                    title="Lo retira de la lista y sus rasgos restarán al recalcular."
                    onClick={() => onVerdict("not_interested")}
                  >
                    <IconThumbDown /> No me interesa
                  </Button>
                  <span className="upcoming-actions__spacer" />
                  <Button
                    size="icon-xs"
                    variant="ghost"
                    aria-label={`Programar un aviso para ${item.title}`}
                    title="Programar un aviso con esta fecha"
                    onClick={onSchedule}
                  >
                    <IconBellPlus />
                  </Button>
                  <Button
                    size="icon-xs"
                    variant="ghost"
                    disabled={busy}
                    aria-label={`Descartar ${item.title}`}
                    title="Solo lo retira de la lista; sus rasgos también restarán al recalcular."
                    onClick={onDismiss}
                  >
                    <IconX />
                  </Button>
                </div>

                {verdict === "interested" && (
                  <p className="upcoming-row__verdict">
                    Anotado como interesante. Se aplicará en el próximo recálculo.
                  </p>
                )}
              </div>
            </li>
          </RevealOnScroll>
        </GamePreviewCard>
      </ContextMenuTrigger>
      {/* Las mismas cuatro acciones de la fila, sin tener que apuntar a botones
          de dieciséis píxeles. */}
      <ContextMenuContent aria-label={`Acciones rápidas de ${item.title}`}>
        <ContextMenuLabel>{item.title}</ContextMenuLabel>
        <ContextMenuSeparator />
        <ContextMenuItem disabled={busy} onSelect={() => onVerdict("interested")}>
          <IconThumbUp aria-hidden="true" /> Me interesa
        </ContextMenuItem>
        <ContextMenuItem disabled={busy} onSelect={() => onVerdict("not_interested")}>
          <IconThumbDown aria-hidden="true" /> No me interesa
        </ContextMenuItem>
        <ContextMenuSeparator />
        <ContextMenuItem onSelect={onSchedule}>
          <IconBellPlus aria-hidden="true" /> Programar un aviso
        </ContextMenuItem>
        <ContextMenuItem variant="destructive" disabled={busy} onSelect={onDismiss}>
          <IconX aria-hidden="true" /> Descartar
        </ContextMenuItem>
      </ContextMenuContent>
    </ContextMenu>
  );
}
