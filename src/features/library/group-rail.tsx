import { useEffect, useRef, useState } from "react";
import { GROUP_HEADER_HEIGHT, type GroupJump } from "@/features/library/group-layout";

/** Ancho reservado al índice de salto, incluido su margen con las filas. */
export const GROUP_RAIL_WIDTH = 26;

/** Por debajo de tres grupos el índice no ahorra ni un desplazamiento. */
const MIN_GROUPS_FOR_RAIL = 3;

export function groupRailVisible(groupCount: number): boolean {
  return groupCount >= MIN_GROUPS_FOR_RAIL;
}

/**
 * Encabezado del grupo activo, dibujado fuera del lienzo virtual.
 *
 * Dentro del lienzo el encabezado viaja con las filas y se pierde de vista en
 * cuanto entras en el grupo; aquí queda clavado en el borde superior y el grupo
 * siguiente lo empuja hacia arriba al alcanzarlo, sin saltos.
 */
export function GroupStickyHeader({
  label,
  loaded,
  complete,
  shift,
}: {
  label: string;
  loaded: number;
  complete: boolean;
  shift: number;
}) {
  return (
    <div className="library-group-band" aria-hidden="true">
      <div
        className="library-group-header library-group-header--pinned"
        style={{ height: GROUP_HEADER_HEIGHT, transform: `translateY(${shift}px)` }}
      >
        <span>{label}</span>
        <GroupCount loaded={loaded} complete={complete} />
      </div>
    </div>
  );
}

/**
 * Recuento del encabezado.
 *
 * Mientras queden páginas por traer, el número solo puede hablar de lo que hay
 * en memoria: decirlo a secas convertiría un «35» en un total que cambiaría
 * solo al seguir desplazando.
 */
export function GroupCount({ loaded, complete }: { loaded: number; complete: boolean }) {
  const formatted = loaded.toLocaleString("es-ES");
  if (complete) return <data value={loaded}>{formatted}</data>;
  return (
    <data value={loaded} title="Recuento sobre los juegos ya cargados">
      {formatted} cargados
    </data>
  );
}

/**
 * Índice de salto: un raíl con la etiqueta corta de cada grupo que desplaza la
 * lista hasta su primera fila.
 */
export function GroupRail({
  jumps,
  current,
  maxHeight,
  onJump,
}: {
  jumps: readonly GroupJump[];
  current: number;
  maxHeight: number;
  onJump: (row: number) => void;
}) {
  const [cursor, setCursor] = useState<number | undefined>(undefined);
  const railRef = useRef<HTMLElement>(null);
  const active = cursor ?? Math.max(0, current);

  // Si la agrupación cambia, el cursor del teclado puede apuntar a un grupo que
  // ya no existe.
  useEffect(() => {
    setCursor((value) => (value !== undefined && value >= jumps.length ? undefined : value));
  }, [jumps.length]);

  const focusEntry = (index: number) => {
    const clamped = Math.min(jumps.length - 1, Math.max(0, index));
    setCursor(clamped);
    const buttons = railRef.current?.querySelectorAll<HTMLButtonElement>("button");
    buttons?.[clamped]?.focus();
  };

  return (
    <div className="library-group-rail-anchor">
      <nav
        ref={railRef}
        className="library-group-rail"
        aria-label="Índice de grupos"
        style={{ maxHeight }}
        onBlur={(event) => {
          if (!event.currentTarget.contains(event.relatedTarget)) setCursor(undefined);
        }}
        onKeyDown={(event) => {
          if (event.key === "ArrowDown") focusEntry(active + 1);
          else if (event.key === "ArrowUp") focusEntry(active - 1);
          else if (event.key === "Home") focusEntry(0);
          else if (event.key === "End") focusEntry(jumps.length - 1);
          else return;
          event.preventDefault();
        }}
      >
        {jumps.map((jump, index) => (
          <button
            key={jump.key}
            type="button"
            tabIndex={index === active ? 0 : -1}
            data-current={index === current}
            aria-current={index === current ? "true" : undefined}
            aria-label={`Ir a ${jump.key}`}
            onClick={() => {
              setCursor(index);
              onJump(jump.row);
            }}
          >
            {jump.label}
          </button>
        ))}
      </nav>
    </div>
  );
}
