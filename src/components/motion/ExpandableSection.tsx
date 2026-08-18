import { IconChevronDown } from "@tabler/icons-react";
import { AnimatePresence, motion } from "motion/react";
import type * as React from "react";
import { useCallback, useEffect, useId, useRef, useState } from "react";
import { cn } from "@/lib/utils";
import "./motion.css";
import { TRANSITION_DISCLOSURE, TRANSITION_NONE } from "./motion-tokens";
import { useReducedMotion } from "./use-reduced-motion";

export interface ExpandableSectionProps {
  /** Contenido de la cabecera pulsable. */
  title: React.ReactNode;
  children: React.ReactNode;
  /** Estado controlado. Si se omite, la sección se gobierna sola. */
  open?: boolean | undefined;
  /** Estado inicial cuando la sección no está controlada. */
  defaultOpen?: boolean | undefined;
  onOpenChange?: ((open: boolean) => void) | undefined;
  /** Contenido alineado a la derecha de la cabecera: contadores, acciones. */
  headerExtra?: React.ReactNode | undefined;
  disabled?: boolean | undefined;
  /**
   * Conserva el contenido montado al cerrarse. Necesario cuando dentro hay un
   * formulario con estado; el contenido queda además marcado como inerte.
   */
  keepMounted?: boolean | undefined;
  /**
   * Se invoca con la altura medida del contenido cada vez que cambia. Es el
   * enganche para revalidar un virtualizador (`virtualizer.measure()`) cuando
   * la sección vive dentro de una fila de altura dinámica.
   */
  onHeightChange?: ((height: number) => void) | undefined;
  className?: string | undefined;
  contentClassName?: string | undefined;
  id?: string | undefined;
  ref?: React.Ref<HTMLDivElement> | undefined;
}

/**
 * Desplegable de altura medida.
 *
 * La altura se interpola entre 0 y la medida real del contenido, así que no hay
 * salto al abrir ni corte al cerrar. Es la única animación de la carpeta que
 * toca `layout`, y por eso está limitada a 300 ms y a una sola vez por gesto;
 * el resto de componentes se mantiene en `transform` y `opacity`.
 *
 * Al cerrarse el contenido se desmonta —o queda inerte con `keepMounted`—, de
 * modo que nunca queda un control enfocable dentro de una caja de altura cero.
 */
export function ExpandableSection({
  title,
  children,
  open,
  defaultOpen = false,
  onOpenChange,
  headerExtra,
  disabled = false,
  keepMounted = false,
  onHeightChange,
  className,
  contentClassName,
  id,
  ref,
}: ExpandableSectionProps) {
  const reducedMotion = useReducedMotion();
  const generatedId = useId();
  const contentId = id ? `${id}-content` : `${generatedId}-content`;
  const [uncontrolledOpen, setUncontrolledOpen] = useState(defaultOpen);
  const isControlled = open !== undefined;
  const isOpen = isControlled ? open : uncontrolledOpen;
  const contentRef = useRef<HTMLDivElement | null>(null);
  const heightCallback = useRef(onHeightChange);

  // Igual que en `RevealOnScroll`: la referencia se actualiza en un efecto para
  // no escribir en ella durante el render.
  useEffect(() => {
    heightCallback.current = onHeightChange;
  });

  const reportHeight = useCallback(() => {
    const notify = heightCallback.current;
    if (!notify) return;
    notify(isOpen ? (contentRef.current?.scrollHeight ?? 0) : 0);
  }, [isOpen]);

  useEffect(() => {
    reportHeight();
    const element = contentRef.current;
    if (!element || typeof ResizeObserver === "undefined") return;
    const observer = new ResizeObserver(() => reportHeight());
    observer.observe(element);
    return () => observer.disconnect();
  }, [reportHeight]);

  const toggle = () => {
    if (disabled) return;
    const next = !isOpen;
    if (!isControlled) setUncontrolledOpen(next);
    onOpenChange?.(next);
  };

  const transition = reducedMotion ? TRANSITION_NONE : TRANSITION_DISCLOSURE;

  const content = (
    <div ref={contentRef} className={cn("vx-expandable__content", contentClassName)}>
      {children}
    </div>
  );

  return (
    <section
      ref={ref}
      className={cn("vx-expandable", className)}
      data-slot="expandable-section"
      data-open={isOpen}
      {...(id ? { id } : {})}
    >
      <div className="vx-expandable__header">
        <button
          type="button"
          className="vx-expandable__trigger"
          data-slot="expandable-section-trigger"
          aria-expanded={isOpen}
          // El panel desmontado no existe en el DOM: apuntar a un id ausente
          // sería una referencia ARIA rota.
          {...(keepMounted || isOpen ? { "aria-controls": contentId } : {})}
          disabled={disabled}
          onClick={toggle}
        >
          <IconChevronDown
            className="vx-expandable__chevron"
            size={13}
            stroke={2}
            data-open={isOpen}
            aria-hidden="true"
          />
          <span className="vx-expandable__title">{title}</span>
        </button>
        {headerExtra ? <div className="vx-expandable__extra">{headerExtra}</div> : null}
      </div>
      {keepMounted ? (
        <motion.div
          id={contentId}
          className="vx-expandable__panel"
          initial={false}
          animate={{ height: isOpen ? "auto" : 0, opacity: isOpen ? 1 : 0 }}
          transition={transition}
          aria-hidden={!isOpen}
          inert={!isOpen}
        >
          {content}
        </motion.div>
      ) : (
        <AnimatePresence initial={false}>
          {isOpen ? (
            <motion.div
              key="panel"
              id={contentId}
              className="vx-expandable__panel"
              initial={{ height: 0, opacity: 0 }}
              animate={{ height: "auto", opacity: 1 }}
              exit={{ height: 0, opacity: 0 }}
              transition={transition}
            >
              {content}
            </motion.div>
          ) : null}
        </AnimatePresence>
      )}
    </section>
  );
}
