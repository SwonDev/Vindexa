import type { CSSProperties, ReactNode } from "react";
import { cn } from "@/lib/utils";

export interface MetricStripItem {
  id: string;
  /** Rótulo del dato, siempre en la misma tipografía apagada. */
  label: string;
  value: ReactNode;
  /** Icono a la izquierda del rótulo. */
  icon?: ReactNode;
  /** Aclaración breve bajo la cifra: procedencia, fecha, salvedad. */
  note?: ReactNode;
  /** Acción que solo tiene sentido junto a este dato. */
  action?: ReactNode;
  /** Texto completo cuando la celda recorta o la cifra necesita contexto. */
  title?: string;
  /** Marca la celda que exige atención, sin cambiar su geometría. */
  alert?: boolean;
}

interface MetricStripProps {
  items: readonly MetricStripItem[];
  /** Nombre accesible de la tira; sin él es una lista de cifras sin contexto. */
  label: string;
  className?: string | undefined;
}

/**
 * Tira de métricas.
 *
 * Un único componente para las cifras agregadas de cualquier pantalla o panel.
 * Reparte las celdas por igual, comparte rótulo, cifra y nota, y mantiene las
 * cifras en `tabular-nums` para que no bailen al actualizarse.
 */
export function MetricStrip({ items, label, className }: MetricStripProps) {
  return (
    <dl
      className={cn("metric-strip", className)}
      aria-label={label}
      // Como variable y no como `grid-template-columns`: así una consulta de
      // medios puede replegar la tira sin competir con un estilo en línea.
      style={{ "--metric-strip-columns": items.length } as CSSProperties}
    >
      {items.map((item) => (
        <div
          key={item.id}
          className="metric-strip__cell"
          data-alert={item.alert ? "true" : undefined}
          title={item.title}
        >
          <dt>
            {item.icon}
            {item.label}
          </dt>
          <dd>{item.value}</dd>
          {item.note ? <p className="metric-strip__note">{item.note}</p> : null}
          {item.action ?? null}
        </div>
      ))}
    </dl>
  );
}
