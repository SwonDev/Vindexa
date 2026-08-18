import { Slot } from "radix-ui";
import type * as React from "react";
import { Children, forwardRef, useEffect, useRef, useState } from "react";
import { cn } from "@/lib/utils";
import "./motion.css";
import { REVEAL_DISTANCE_PX, STAGGER_MAX_MS, STAGGER_STEP_MS } from "./motion-tokens";
import { useReducedMotion } from "./use-reduced-motion";

function hasIntersectionObserver(): boolean {
  return typeof IntersectionObserver !== "undefined";
}

export interface RevealOnScrollProps extends React.ComponentPropsWithoutRef<"div"> {
  /** Aplica la aparición al hijo único en lugar de envolverlo en un `div`. */
  asChild?: boolean | undefined;
  /** Retardo de entrada en milisegundos. */
  delayMs?: number | undefined;
  /** Desplazamiento vertical inicial. Por defecto 4 px. */
  distancePx?: number | undefined;
  /** Duración de la aparición en milisegundos. Por defecto 200 ms. */
  durationMs?: number | undefined;
  /** Si es `false`, el elemento vuelve a ocultarse al salir del viewport. */
  once?: boolean | undefined;
  /** Anula la aparición: el contenido se pinta visible desde el primer fotograma. */
  disabled?: boolean | undefined;
  /** Margen del observador. Adelanta la entrada antes de que la fila se vea. */
  rootMargin?: string | undefined;
  /** Fracción visible necesaria para considerar el elemento dentro. */
  threshold?: number | undefined;
  onReveal?: (() => void) | undefined;
}

/**
 * Aparición sutil al entrar en el viewport.
 *
 * Solo toca `opacity` y `transform`, con una transición CSS: no hay bucle de
 * JavaScript por fila, así que centenares de elementos no cuestan nada. La caja
 * conserva su tamaño en todo momento, por lo que no altera la medición de una
 * lista virtualizada; con `asChild` ni siquiera añade un nodo al DOM.
 *
 * Con movimiento reducido, o sin `IntersectionObserver`, se renderiza visible
 * directamente en vez de atenuar la animación.
 */
export const RevealOnScroll = forwardRef<HTMLDivElement, RevealOnScrollProps>(
  function RevealOnScroll(
    {
      asChild = false,
      delayMs = 0,
      distancePx,
      durationMs,
      once = true,
      disabled = false,
      rootMargin = "0px 0px -8px 0px",
      threshold = 0,
      onReveal,
      className,
      style,
      children,
      ...props
    },
    forwardedRef,
  ) {
    const reducedMotion = useReducedMotion();
    const skip = disabled || reducedMotion || !hasIntersectionObserver();
    const [revealed, setRevealed] = useState(false);
    const elementRef = useRef<HTMLDivElement | null>(null);
    const revealCallback = useRef(onReveal);

    // Se guarda en un efecto, no durante el render: escribir en una referencia
    // mientras se renderiza no es seguro en modo concurrente.
    useEffect(() => {
      revealCallback.current = onReveal;
    });

    useEffect(() => {
      if (skip) {
        revealCallback.current?.();
        return;
      }
      const element = elementRef.current;
      if (!element) return;
      const observer = new IntersectionObserver(
        (entries) => {
          const isVisible = entries.some((entry) => entry.isIntersecting);
          if (isVisible) {
            setRevealed(true);
            revealCallback.current?.();
            if (once) observer.disconnect();
          } else if (!once) {
            setRevealed(false);
          }
        },
        { rootMargin, threshold },
      );
      observer.observe(element);
      return () => observer.disconnect();
    }, [once, rootMargin, skip, threshold]);

    const Component = asChild ? Slot.Root : "div";
    const isRevealed = skip || revealed;

    // Solo se publican las variables que se apartan del valor por defecto: en
    // una lista larga, cada atributo `style` de más es peso muerto en el DOM.
    const revealStyle = {
      ...style,
      ...(delayMs > 0 ? { "--vx-reveal-delay": `${delayMs}ms` } : {}),
      ...(distancePx !== undefined && distancePx !== REVEAL_DISTANCE_PX
        ? { "--vx-reveal-distance": `${distancePx}px` }
        : {}),
      ...(durationMs !== undefined ? { "--vx-reveal-duration": `${durationMs}ms` } : {}),
    } as React.CSSProperties;

    return (
      <Component
        ref={(node: HTMLDivElement | null) => {
          elementRef.current = node;
          if (typeof forwardedRef === "function") forwardedRef(node);
          else if (forwardedRef) forwardedRef.current = node;
        }}
        className={cn("vx-reveal", className)}
        data-slot="reveal-on-scroll"
        data-revealed={isRevealed}
        style={revealStyle}
        {...props}
      >
        {children}
      </Component>
    );
  },
);

type StaggerElement = "div" | "ul" | "ol" | "section" | "nav";

export interface StaggerListProps extends React.ComponentPropsWithoutRef<"div"> {
  /**
   * Etiqueta del contenedor. Permite que `StaggerList` *sea* la lista existente
   * en vez de añadir un nodo intermedio que rompa una rejilla ya montada.
   */
  as?: StaggerElement | undefined;
  /** Retardo añadido por cada elemento. Por defecto 24 ms. */
  stepMs?: number | undefined;
  /** Techo del retardo acumulado. Por defecto 160 ms. */
  maxDelayMs?: number | undefined;
  /** Índice del primer hijo, para encadenar varios bloques seguidos. */
  startIndex?: number | undefined;
  distancePx?: number | undefined;
  once?: boolean | undefined;
  disabled?: boolean | undefined;
  /** Aplica cada aparición al propio hijo en vez de envolverlo. */
  itemAsChild?: boolean | undefined;
}

/**
 * Aparición escalonada de un grupo corto: filas de una sección, chips de
 * filtro, tarjetas de un panel. El retardo acumulado está limitado para que el
 * último elemento nunca se haga esperar.
 *
 * No es para rejillas virtualizadas: al reciclar filas volvería a animarlas en
 * cada desplazamiento. Ahí conviene un único `RevealOnScroll` en el contenedor.
 */
export const StaggerList = forwardRef<HTMLDivElement, StaggerListProps>(function StaggerList(
  {
    as = "div",
    stepMs = STAGGER_STEP_MS,
    maxDelayMs = STAGGER_MAX_MS,
    startIndex = 0,
    distancePx,
    once = true,
    disabled = false,
    itemAsChild = false,
    className,
    children,
    ...props
  },
  ref,
) {
  // Todas las etiquetas admitidas comparten props y referencia de elemento; el
  // aserto evita que TypeScript intersecte cinco interfaces de referencia.
  const Container = as as "div";
  return (
    <Container
      ref={ref}
      className={cn("vx-stagger-list", className)}
      data-slot="stagger-list"
      {...props}
    >
      {Children.map(children, (child, index) => (
        <RevealOnScroll
          asChild={itemAsChild}
          delayMs={Math.min((startIndex + index) * stepMs, maxDelayMs)}
          distancePx={distancePx}
          once={once}
          disabled={disabled}
        >
          {child}
        </RevealOnScroll>
      ))}
    </Container>
  );
});
