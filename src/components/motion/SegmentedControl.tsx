import { LayoutGroup, motion } from "motion/react";
import type * as React from "react";
import { useId, useRef } from "react";
import { cn } from "@/lib/utils";
import "./motion.css";
import { SPRING_SNAP } from "./motion-tokens";
import { useReducedMotion } from "./use-reduced-motion";

export interface SegmentedControlOption<TValue extends string> {
  value: TValue;
  label: React.ReactNode;
  /** Icono opcional a la izquierda de la etiqueta. */
  icon?: React.ReactNode | undefined;
  /** Texto de ayuda para el atributo `title`. */
  hint?: string | undefined;
  disabled?: boolean | undefined;
}

export interface SegmentedControlProps<TValue extends string> {
  options: readonly SegmentedControlOption<TValue>[];
  value: TValue;
  onValueChange: (value: TValue) => void;
  /** Nombre accesible del grupo. Obligatorio: no hay etiqueta implícita. */
  label: string;
  size?: "sm" | "md" | undefined;
  className?: string | undefined;
  id?: string | undefined;
  ref?: React.Ref<HTMLDivElement> | undefined;
}

/**
 * Conmutador de segmentos con indicador deslizante.
 *
 * El indicador es un único elemento compartido (`layoutId`), así que al cambiar
 * de opción se desplaza con un muelle sobreamortiguado —llega y para, sin
 * rebote— en lugar de aparecer y desaparecer. Es la microinteracción que hace
 * legible el cambio de vista, orden o densidad sin mover el texto.
 *
 * Está pensado para conmutar vistas o filtros, no para pestañas con paneles:
 * para eso el repositorio ya tiene `@/components/ui/tabs` sobre Radix. Por eso
 * cada opción es un `input[type=radio]` nativo dentro de su etiqueta: el estado
 * marcado, el nombre accesible y el orden de tabulación salen del navegador, no
 * de atributos ARIA imitados. Las flechas, `Inicio` y `Fin` se manejan de forma
 * explícita para que el comportamiento sea idéntico en todos los motores.
 *
 * Con movimiento reducido el indicador salta a su posición sin animar.
 */
export function SegmentedControl<TValue extends string>({
  options,
  value,
  onValueChange,
  label,
  size = "md",
  className,
  id,
  ref,
}: SegmentedControlProps<TValue>) {
  const reducedMotion = useReducedMotion();
  const groupId = useId();
  const inputsRef = useRef<(HTMLInputElement | null)[]>([]);

  const moveFocus = (from: number, direction: 1 | -1) => {
    const total = options.length;
    for (let step = 1; step <= total; step += 1) {
      const index = (((from + direction * step) % total) + total) % total;
      const option = options[index];
      if (!option || option.disabled) continue;
      onValueChange(option.value);
      inputsRef.current[index]?.focus();
      return;
    }
  };

  const focusEdge = (edge: "first" | "last") => {
    const indexes = options.map((_, index) => index);
    const ordered = edge === "first" ? indexes : indexes.reverse();
    for (const index of ordered) {
      const option = options[index];
      if (!option || option.disabled) continue;
      onValueChange(option.value);
      inputsRef.current[index]?.focus();
      return;
    }
  };

  const handleKeyDown = (event: React.KeyboardEvent<HTMLInputElement>, index: number) => {
    if (event.key === "ArrowRight" || event.key === "ArrowDown") {
      event.preventDefault();
      moveFocus(index, 1);
    } else if (event.key === "ArrowLeft" || event.key === "ArrowUp") {
      event.preventDefault();
      moveFocus(index, -1);
    } else if (event.key === "Home") {
      event.preventDefault();
      focusEdge("first");
    } else if (event.key === "End") {
      event.preventDefault();
      focusEdge("last");
    }
  };

  return (
    <LayoutGroup id={groupId}>
      <div
        ref={ref}
        id={id}
        role="radiogroup"
        aria-label={label}
        className={cn("vx-segmented", className)}
        data-slot="segmented-control"
        data-size={size}
      >
        {options.map((option, index) => {
          const selected = option.value === value;
          return (
            <label
              key={option.value}
              className="vx-segmented__option"
              data-slot="segmented-control-option"
              data-selected={selected}
              data-disabled={option.disabled ?? false}
            >
              <input
                ref={(node) => {
                  inputsRef.current[index] = node;
                }}
                type="radio"
                name={groupId}
                className="vx-segmented__input"
                value={option.value}
                checked={selected}
                disabled={option.disabled ?? false}
                {...(option.hint ? { title: option.hint } : {})}
                onChange={() => onValueChange(option.value)}
                onKeyDown={(event) => handleKeyDown(event, index)}
              />
              {selected ? (
                reducedMotion ? (
                  <span
                    className="vx-segmented__indicator"
                    data-slot="segmented-control-indicator"
                  />
                ) : (
                  <motion.span
                    layoutId="vx-segmented-indicator"
                    className="vx-segmented__indicator"
                    data-slot="segmented-control-indicator"
                    transition={SPRING_SNAP}
                  />
                )
              ) : null}
              {option.icon ? (
                <span className="vx-segmented__icon" aria-hidden="true">
                  {option.icon}
                </span>
              ) : null}
              <span className="vx-segmented__label">{option.label}</span>
            </label>
          );
        })}
      </div>
    </LayoutGroup>
  );
}
