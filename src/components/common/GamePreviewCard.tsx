import { HoverCard as HoverCardPrimitive } from "radix-ui";
import { type ReactNode, useEffect, useRef, useState } from "react";
import { useReducedMotion } from "@/components/motion/use-reduced-motion";
import { api } from "@/lib/tauri";
import "./game-preview-card.css";

/**
 * La vista rápida de un juego al pasar el ratón por encima.
 *
 * # Qué problema resuelve
 *
 * Una carátula dice cómo se llama un juego; no dice cómo se ve. Mirando una
 * lista de setenta ofertas, eso es justo lo que hace falta para decidir, y
 * abrir la ficha de cada una para averiguarlo es el camino largo. Steam lo
 * resolvió con un emergente que pasa capturas; aquí se hace igual y se le añade
 * lo que Vindexa sabe y la tienda no —el estado, lo jugado, el precio—.
 *
 * # Cuándo aparece y cuándo no
 *
 * Aparece tras una pausa deliberada: sin ella, recorrer una rejilla con el ratón
 * sería un desfile de emergentes. Desaparece al instante, porque esperar a que
 * se vaya algo que no querías es peor que esperar a que llegue algo que sí.
 *
 * **No aparece durante un arrastre** ni con el movimiento reducido activado. Con
 * movimiento reducido las capturas no rotan solas: se enseña la primera y ya.
 *
 * # Por qué las capturas llegan tarde
 *
 * Sólo ciento sesenta juegos de la biblioteca tienen capturas guardadas, y
 * ninguno de los mil trescientos deseados que no se poseen. Pedirlas por
 * adelantado sería mil trescientas peticiones para algo que quizá nadie mire;
 * pedirlas al detenerse es una petición de medio kilobyte que además queda
 * guardada. Mientras llegan, el emergente ya enseña el texto: nunca está vacío.
 */

/** Cuánto hay que detenerse antes de que aparezca. */
const OPEN_DELAY_MS = 420;
/** Cada cuánto cambia la captura. Suficiente para verla, sin marear. */
const ROTATE_MS = 1_400;

export interface GamePreviewCardProps {
  appId: number;
  title: string;
  /** Lo que se pinta mientras no hay captura: la carátula que ya está en pantalla. */
  fallback?: ReactNode | undefined;
  /** Datos que Vindexa sabe y la tienda no. Se enseñan bajo el título. */
  facts?: readonly { label: string; value: string }[] | undefined;
  /** Línea destacada: el precio, la oferta, lo que motive mirar. */
  headline?: ReactNode | undefined;
  /** Desactiva la vista rápida (arrastre en curso, listas en movimiento). */
  disabled?: boolean;
  children: ReactNode;
}

export function GamePreviewCard({
  appId,
  title,
  fallback,
  facts,
  headline,
  disabled = false,
  children,
}: GamePreviewCardProps) {
  const [open, setOpen] = useState(false);
  const reducedMotion = useReducedMotion();

  return (
    <HoverCardPrimitive.Root
      openDelay={OPEN_DELAY_MS}
      closeDelay={0}
      open={disabled ? false : open}
      onOpenChange={setOpen}
    >
      <HoverCardPrimitive.Trigger asChild>{children}</HoverCardPrimitive.Trigger>
      <HoverCardPrimitive.Portal>
        <HoverCardPrimitive.Content
          className="game-preview"
          side="right"
          align="start"
          sideOffset={10}
          collisionPadding={12}
          // No roba el foco ni el ratón: es una ayuda para mirar, no un panel
          // con el que interactuar. Quien quiera actuar tiene el clic derecho.
          onPointerDownOutside={(event) => event.preventDefault()}
        >
          <PreviewBody
            appId={appId}
            title={title}
            fallback={fallback}
            facts={facts}
            headline={headline}
            open={open && !disabled}
            reducedMotion={reducedMotion}
          />
        </HoverCardPrimitive.Content>
      </HoverCardPrimitive.Portal>
    </HoverCardPrimitive.Root>
  );
}

function PreviewBody({
  appId,
  title,
  fallback,
  facts,
  headline,
  open,
  reducedMotion,
}: {
  appId: number;
  title: string;
  fallback?: ReactNode | undefined;
  facts?: readonly { label: string; value: string }[] | undefined;
  headline?: ReactNode | undefined;
  open: boolean;
  reducedMotion: boolean;
}) {
  const [shots, setShots] = useState<string[]>([]);
  const [index, setIndex] = useState(0);
  const pedido = useRef<number | undefined>(undefined);

  // Las capturas se piden al abrirse, una sola vez por juego y sesión: el
  // backend guarda lo que la tienda diga, incluida la ausencia de capturas.
  useEffect(() => {
    if (!open || pedido.current === appId) return;
    pedido.current = appId;
    let vigente = true;
    api
      .gamePreview(appId)
      .then((preview) => {
        if (!vigente) return;
        setShots(preview.screenshots);
        setIndex(0);
      })
      .catch(() => {
        // Sin capturas la vista sigue siendo útil: el texto ya está.
        if (vigente) setShots([]);
      });
    return () => {
      vigente = false;
    };
  }, [appId, open]);

  useEffect(() => {
    if (!open || reducedMotion || shots.length < 2) return;
    const id = window.setInterval(
      () => setIndex((actual) => (actual + 1) % shots.length),
      ROTATE_MS,
    );
    return () => window.clearInterval(id);
  }, [open, reducedMotion, shots.length]);

  const actual = shots[index];

  return (
    <>
      <div className="game-preview__stage">
        {actual ? (
          shots.map((shot, position) => (
            <img
              key={shot}
              className="game-preview__shot"
              src={shot}
              alt=""
              data-active={position === index}
              draggable={false}
            />
          ))
        ) : (
          <div className="game-preview__fallback">{fallback}</div>
        )}
        {shots.length > 1 && (
          <div className="game-preview__dots" aria-hidden="true">
            {shots.map((shot, position) => (
              <span key={shot} data-active={position === index} />
            ))}
          </div>
        )}
      </div>
      <div className="game-preview__body">
        <p className="game-preview__title">{title}</p>
        {headline && <div className="game-preview__headline">{headline}</div>}
        {facts && facts.length > 0 && (
          <dl className="game-preview__facts">
            {facts.map((fact) => (
              <div key={fact.label}>
                <dt>{fact.label}</dt>
                <dd>{fact.value}</dd>
              </div>
            ))}
          </dl>
        )}
      </div>
    </>
  );
}
