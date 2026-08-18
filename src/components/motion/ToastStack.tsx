import {
  IconAlertCircle,
  IconAlertTriangle,
  IconCheck,
  IconInfoCircle,
  IconX,
} from "@tabler/icons-react";
import { AnimatePresence, motion } from "motion/react";
import type * as React from "react";
import { useEffect } from "react";
import { cn } from "@/lib/utils";
import "./motion.css";
import { SPRING_STACK, TRANSITION_NONE, TRANSITION_SLOW } from "./motion-tokens";
import { useReducedMotion } from "./use-reduced-motion";

export type ToastKind = "info" | "success" | "warning" | "error";

export interface ToastItem {
  /** Identidad estable: es la clave de la animación de entrada y salida. */
  id: string;
  message: string;
  kind?: ToastKind | undefined;
  /** Segunda línea con el detalle técnico del error o del resultado. */
  detail?: string | undefined;
  /** Acción única, p. ej. «Deshacer» tras mover juegos a una colección. */
  action?: { label: string; onClick: () => void } | undefined;
  /** Anula el autocierre global solo para este aviso. */
  autoDismissMs?: number | undefined;
}

export type ToastPosition = "bottom-right" | "bottom-left" | "top-right" | "top-left";

export interface ToastStackProps {
  toasts: readonly ToastItem[];
  onDismiss: (id: string) => void;
  position?: ToastPosition | undefined;
  /** Máximo de avisos visibles. Se conservan los últimos de la lista. */
  max?: number | undefined;
  /** Milisegundos hasta el autocierre. `0` lo desactiva. */
  autoDismissMs?: number | undefined;
  /** Nombre accesible de la región. */
  label?: string | undefined;
  className?: string | undefined;
  ref?: React.Ref<HTMLOListElement> | undefined;
}

const KIND_ICON = {
  info: IconInfoCircle,
  success: IconCheck,
  warning: IconAlertTriangle,
  error: IconAlertCircle,
} as const;

interface ToastRowProps {
  toast: ToastItem;
  onDismiss: (id: string) => void;
  autoDismissMs: number;
  reducedMotion: boolean;
}

function ToastRow({ toast, onDismiss, autoDismissMs, reducedMotion }: ToastRowProps) {
  const kind = toast.kind ?? "info";
  const Icon = KIND_ICON[kind];
  // Un error no se cierra solo: es información que el usuario tiene que leer.
  const delay = toast.autoDismissMs ?? (kind === "error" ? 0 : autoDismissMs);

  useEffect(() => {
    if (delay <= 0) return;
    const timer = window.setTimeout(() => onDismiss(toast.id), delay);
    return () => window.clearTimeout(timer);
  }, [delay, onDismiss, toast.id]);

  const offset = reducedMotion ? 0 : 6;

  return (
    <motion.li
      layout={!reducedMotion}
      className="vx-toast"
      data-slot="toast"
      data-kind={kind}
      role={kind === "error" ? "alert" : "status"}
      initial={{ opacity: 0, y: offset }}
      animate={{ opacity: 1, y: 0 }}
      exit={{ opacity: 0, y: offset }}
      transition={reducedMotion ? TRANSITION_NONE : { ...TRANSITION_SLOW, layout: SPRING_STACK }}
    >
      <Icon className="vx-toast__icon" size={15} stroke={1.8} aria-hidden="true" />
      <div className="vx-toast__body">
        <p className="vx-toast__message">{toast.message}</p>
        {toast.detail ? <p className="vx-toast__detail">{toast.detail}</p> : null}
      </div>
      {toast.action ? (
        <button type="button" className="vx-toast__action" onClick={toast.action.onClick}>
          {toast.action.label}
        </button>
      ) : null}
      <button
        type="button"
        className="vx-toast__close"
        aria-label={`Descartar aviso: ${toast.message}`}
        onClick={() => onDismiss(toast.id)}
      >
        <IconX size={13} stroke={2} aria-hidden="true" />
      </button>
    </motion.li>
  );
}

/**
 * Pila de avisos controlada por quien la usa: recibe la lista y notifica el
 * descarte, sin almacén global ni singleton escondido.
 *
 * Entra y sale con opacidad y un desplazamiento de 6 px; cuando desaparece uno
 * de en medio, los demás recolocan su posición con una animación de disposición
 * que `motion` resuelve por `transform`, no moviendo la caja.
 *
 * Los errores no se cierran solos y se anuncian como `alert`; el resto son
 * `status` y se retiran a los cinco segundos.
 */
export function ToastStack({
  toasts,
  onDismiss,
  position = "bottom-right",
  max = 3,
  autoDismissMs = 5000,
  label = "Avisos",
  className,
  ref,
}: ToastStackProps) {
  const reducedMotion = useReducedMotion();
  const visible = max > 0 ? toasts.slice(-max) : toasts.slice();

  return (
    <ol
      ref={ref}
      className={cn("vx-toast-stack", className)}
      data-slot="toast-stack"
      data-position={position}
      aria-label={label}
    >
      <AnimatePresence initial={false} mode="popLayout">
        {visible.map((toast) => (
          <ToastRow
            key={toast.id}
            toast={toast}
            onDismiss={onDismiss}
            autoDismissMs={autoDismissMs}
            reducedMotion={reducedMotion}
          />
        ))}
      </AnimatePresence>
    </ol>
  );
}
