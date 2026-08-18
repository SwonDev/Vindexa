import type { ReactNode } from "react";
import { Eyebrow } from "@/components/common/Eyebrow";
import { cn } from "@/lib/utils";

interface PageHeaderProps {
  /** Línea corta en mayúsculas que sitúa la pantalla. */
  eyebrow: string;
  title: string;
  /** Identificador del `h1` cuando otra región lo referencia con `aria-labelledby`. */
  titleId?: string | undefined;
  /** Cifra o aclaración breve bajo el título. */
  meta?: ReactNode;
  /** Controles alineados al extremo opuesto del título. */
  actions?: ReactNode;
  className?: string | undefined;
}

/**
 * Cabecera de pantalla.
 *
 * Todas las pantallas de primer nivel entran por aquí: misma altura, mismo
 * fondo y misma relación entre antetítulo, título y controles. Sin subtítulo
 * de plantilla: solo `meta` cuando hay un dato que la pantalla no repite más
 * abajo.
 */
export function PageHeader({ eyebrow, title, titleId, meta, actions, className }: PageHeaderProps) {
  return (
    <header className={cn("screen-heading", className)}>
      <div className="screen-heading__identity">
        <Eyebrow>{eyebrow}</Eyebrow>
        <h1 id={titleId}>{title}</h1>
        {meta}
      </div>
      {actions ? <div className="screen-heading__actions">{actions}</div> : null}
    </header>
  );
}
