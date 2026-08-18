import { Progress } from "@/components/ui/progress";
import { cn } from "@/lib/utils";

interface ProgressMeterProps {
  /** Porcentaje completado. Se recorta a 0–100 antes de dibujarlo. */
  value: number;
  /** Etiqueta accesible de la barra; describe de qué juego es el progreso. */
  label: string;
  /**
   * Deja solo la cifra. Las densidades más apretadas no tienen ancho para la
   * banda, pero el dato tiene que seguir estando.
   */
  barHidden?: boolean | undefined;
  className?: string | undefined;
}

/**
 * Medidor de progreso de la aplicación.
 *
 * Es el elemento que más se repite en pantalla —una vez por juego— y el único
 * sitio donde se decide cómo se ve el porcentaje: banda, relleno, cifra y la
 * marca de «terminado». Cualquier pantalla que necesite mostrar avance usa
 * este componente; no hay una segunda forma de dibujarlo.
 */
export function ProgressMeter({ value, label, barHidden, className }: ProgressMeterProps) {
  const percent = Math.min(100, Math.max(0, Math.round(value)));
  const complete = percent >= 100;
  return (
    <div className={cn("progress-meter", className)} data-complete={complete ? "true" : undefined}>
      {barHidden ? null : <Progress value={percent} aria-label={label} />}
      <span className="progress-meter__value">{percent}%</span>
    </div>
  );
}
