import {
  IconAlertTriangle,
  IconChevronDown,
  IconLoader2,
  IconLock,
  IconLockOpen2,
  IconRefresh,
} from "@tabler/icons-react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useMemo, useState } from "react";
import { AnimatedNumber, ShimmerSkeleton } from "@/components/motion";
import { Button } from "@/components/ui/button";
import { Switch } from "@/components/ui/switch";
import { formatRelativeDate } from "@/lib/format";
import { api, getErrorMessage } from "@/lib/tauri";
import type { PrioritySignal } from "@/lib/types";

/** Señales visibles antes de plegar el resto. Cuatro caben sin desplazar. */
const SIGNALS_VISIBLE = 4;

const weightFormat = new Intl.NumberFormat("es-ES", {
  maximumFractionDigits: 1,
  signDisplay: "always",
});
const decimalFormat = new Intl.NumberFormat("es-ES", { maximumFractionDigits: 1 });

interface Props {
  appId: number;
}

/**
 * Explicación de la prioridad dinámica de un juego.
 *
 * El backend (`src-tauri/src/db/priority.rs`) calcula una puntuación 0-100 a
 * partir de hechos que ya están en SQLite y guarda **por qué** salió ese número.
 * Este panel enseña ese porqué entero: la frase principal, cada señal con su
 * aporte —positivo sube, negativo baja— y la aritmética completa, de modo que
 * la suma se puede comprobar a ojo en lugar de creerse un ranking opaco.
 *
 * Hay una sola escala a la vista, 0-100, porque una sola es la que ordena la
 * biblioteca. El anclado manual sigue sin ser un detalle escondido —el
 * interruptor dice en una frase quién manda, y el backend redacta el aviso
 * cuando las dos lecturas discrepan—, pero la prioridad manual se lee y se
 * edita donde se fija: el deslizador «Prioridad» de esta misma pestaña.
 */
export function PriorityExplanation({ appId }: Props) {
  const queryClient = useQueryClient();
  const [signalsExpanded, setSignalsExpanded] = useState(false);
  const explanationQuery = useQuery({
    queryKey: ["priority-explanation", appId],
    queryFn: () => api.explainPriority(appId),
    enabled: appId > 0,
  });
  const lockMutation = useMutation({
    mutationFn: (locked: boolean) => api.setPriorityLock(appId, locked),
    onSuccess: async () => {
      await queryClient.invalidateQueries({ queryKey: ["priority-explanation", appId] });
      await queryClient.invalidateQueries({ queryKey: ["game", appId] });
      void queryClient.invalidateQueries({ queryKey: ["games"] });
    },
  });
  const recomputeMutation = useMutation({
    mutationFn: () => api.recomputePriorities(),
    onSuccess: async () => {
      await queryClient.invalidateQueries({ queryKey: ["priority-explanation", appId] });
      void queryClient.invalidateQueries({ queryKey: ["games"] });
    },
  });

  const explanation = explanationQuery.data;
  const signals = explanation?.signals ?? [];
  const scale = useMemo(
    () => signals.reduce((top, signal) => Math.max(top, Math.abs(signal.weight)), 0),
    [signals],
  );
  const visibleSignals = signalsExpanded ? signals : signals.slice(0, SIGNALS_VISIBLE);

  if (explanationQuery.isPending) {
    return (
      <section className="priority-panel priority-panel--loading">
        <ShimmerSkeleton height={92} label="Calculando la explicación de la prioridad" />
      </section>
    );
  }

  if (explanationQuery.isError || !explanation) {
    return (
      <section className="priority-panel priority-panel--error" role="alert">
        <IconAlertTriangle />
        <div>
          <strong>No se pudo explicar la prioridad de este juego.</strong>
          <p>{getErrorMessage(explanationQuery.error)}</p>
        </div>
        <Button size="xs" variant="secondary" onClick={() => void explanationQuery.refetch()}>
          Reintentar
        </Button>
      </section>
    );
  }

  const meterRatio = Math.min(Math.max(explanation.effectiveScore / 100, 0), 1);
  const signalsTotal = signals.reduce((total, signal) => total + signal.weight, 0);
  const baseline = explanation.score - signalsTotal;
  const busy = lockMutation.isPending || recomputeMutation.isPending;
  const freshness = explanation.computedAt
    ? `Calculado ${formatRelativeDate(explanation.computedAt)}`
    : "Sin calcular todavía";

  return (
    <section className="priority-panel" aria-labelledby="priority-panel-title">
      <header className="priority-panel__head">
        <div className="priority-panel__heading">
          <h3 id="priority-panel-title">Por qué está aquí</h3>
          <p className="priority-panel__caption">
            {explanation.locked
              ? "Puntuación efectiva: la proyección de tu prioridad manual."
              : "Puntuación efectiva: la que ordena este juego en tu biblioteca."}
          </p>
        </div>
        <p className="priority-score" data-locked={explanation.locked || undefined}>
          <AnimatedNumber
            className="priority-score__value"
            value={Math.round(explanation.effectiveScore)}
          />
          <span className="priority-score__scale">/100</span>
        </p>
      </header>
      <div className="priority-meter" aria-hidden="true">
        <i style={{ "--priority-meter-fill": String(meterRatio) } as React.CSSProperties} />
      </div>
      <p className="priority-panel__reason">{explanation.reason}</p>
      {/* Una sola escala. Las dos cifras 0-5 que vivían aquí eran el mismo
          concepto contado de otra forma: la manual es el deslizador «Prioridad»
          que está más abajo en esta misma pestaña, y la derivada es este mismo
          0-100 redondeado. Cuál de las dos manda lo dice la frase de anclado
          —y, cuando difieren, el aviso que redacta el backend. */}
      {explanation.manualOverride ? (
        <p className="priority-override">
          <IconLock />
          <span>{explanation.manualOverride}</span>
        </p>
      ) : null}
      <div className="priority-lock">
        <Switch
          checked={explanation.locked}
          disabled={busy}
          aria-label="Anclar mi prioridad manual"
          onCheckedChange={(locked) => lockMutation.mutate(locked)}
        />
        <div className="priority-lock__copy">
          <strong>
            {explanation.locked ? <IconLock /> : <IconLockOpen2 />}
            Anclar mi prioridad manual
          </strong>
          <span>
            {explanation.locked
              ? `Este juego se ordena por tu ${explanation.manualPriority}/5. El cálculo sigue funcionando, pero ya no lo mueve.`
              : "Actívalo y este juego se ordenará por la prioridad que fijes tú abajo, no por el cálculo."}
          </span>
        </div>
      </div>
      {lockMutation.isError ? (
        <p className="priority-panel__failure" role="alert">
          No se pudo cambiar el anclado: {getErrorMessage(lockMutation.error)}
        </p>
      ) : null}
      {signals.length > 0 ? (
        <div className="priority-signals">
          <h4 id="priority-signals-title">Señales que lo mueven</h4>
          <ul
            id="priority-signals-list"
            className="priority-signals__list"
            aria-labelledby="priority-signals-title"
          >
            {visibleSignals.map((signal) => (
              <SignalRow key={signal.signal} signal={signal} scale={scale} />
            ))}
          </ul>
          {signals.length > SIGNALS_VISIBLE ? (
            <Button
              size="xs"
              variant="ghost"
              className="priority-signals__more"
              aria-expanded={signalsExpanded}
              aria-controls="priority-signals-list"
              onClick={() => setSignalsExpanded((expanded) => !expanded)}
            >
              <IconChevronDown />
              {signalsExpanded ? "Mostrar menos señales" : moreSignalsLabel(signals.length)}
            </Button>
          ) : null}
          {/* La suma es comprobable: si no cuadrase, el número no sería creíble. */}
          <p className="priority-arithmetic">
            Parte de {decimalFormat.format(baseline)} y las señales suman{" "}
            {weightFormat.format(signalsTotal)} → {decimalFormat.format(explanation.score)} sobre
            100.
          </p>
        </div>
      ) : (
        <p className="priority-panel__empty">
          Todavía no hay señales guardadas para este juego. Se escriben al recalcular la prioridad
          de la biblioteca.
        </p>
      )}
      {/* La mecánica interna —cuándo se calculó y cómo se vuelve a calcular—
          deja de ser una línea del cuerpo: la fecha viaja en el `title` del
          único control que la usa, y para lectores de pantalla sigue escrita. */}
      <footer className="priority-panel__foot">
        <span className="sr-only">{freshness}</span>
        <Button
          size="xs"
          variant="ghost"
          disabled={busy}
          title={`${freshness}. Recalcula la prioridad de toda la biblioteca, no solo la de este juego.`}
          onClick={() => recomputeMutation.mutate()}
        >
          {recomputeMutation.isPending ? <IconLoader2 className="is-spinning" /> : <IconRefresh />}
          Recalcular la biblioteca
        </Button>
      </footer>
      {recomputeMutation.isError ? (
        <p className="priority-panel__failure" role="alert">
          No se pudo recalcular: {getErrorMessage(recomputeMutation.error)}
        </p>
      ) : null}
    </section>
  );
}

/** «Mostrar 1 señal más» / «Mostrar 3 señales más»: el plural no se descuida. */
function moreSignalsLabel(total: number): string {
  const rest = total - SIGNALS_VISIBLE;
  return rest === 1 ? "Mostrar 1 señal más" : `Mostrar ${rest} señales más`;
}

/**
 * Una señal con su aporte.
 *
 * La dirección se transmite por tres canales independientes —el signo del
 * número, el lado hacia el que crece la barra y el texto para lectores— así que
 * nunca depende solo del color.
 */
function SignalRow({ signal, scale }: { signal: PrioritySignal; scale: number }) {
  const positive = signal.weight >= 0;
  const ratio = scale > 0 ? Math.min(Math.abs(signal.weight) / scale, 1) : 0;
  return (
    <li
      className="priority-signal"
      data-direction={positive ? "up" : "down"}
      data-signal={signal.signal}
    >
      <span className="priority-signal__detail">{signal.detail}</span>
      <span className="priority-signal__bar" aria-hidden="true">
        <i style={{ "--priority-signal-fill": String(ratio) } as React.CSSProperties} />
      </span>
      <span className="priority-signal__weight">
        {weightFormat.format(signal.weight)}
        <span className="sr-only"> puntos; {positive ? "sube" : "baja"} la prioridad</span>
      </span>
    </li>
  );
}
