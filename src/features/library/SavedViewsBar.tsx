import { IconAlertTriangle, IconBookmark, IconPin, IconX } from "@tabler/icons-react";
import { useMemo, useState } from "react";
import { Button } from "@/components/ui/button";
import {
  ContextMenu,
  ContextMenuContent,
  ContextMenuItem,
  ContextMenuLabel,
  ContextMenuSeparator,
  ContextMenuTrigger,
} from "@/components/ui/context-menu";
import { Input } from "@/components/ui/input";
import { Popover, PopoverContent, PopoverTrigger } from "@/components/ui/popover";
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";
import type { FilterChoice } from "@/features/library/library-filters";
import "@/features/library/saved-views.css";
import {
  describeView,
  queryMatchesView,
  type SavedLibraryView,
  type SavedViewQuery,
  type ViewConflict,
} from "@/features/library/library-views";

interface SavedViewsBarProps {
  /** Controlado desde fuera: el botón vive en la barra de herramientas. */
  creating: boolean;
  onCreatingChange: (creating: boolean) => void;
  views: SavedLibraryView[];
  /** Identificadores apilados, en el orden en que se aplicaron. */
  activeIds: string[];
  onToggle: (view: SavedLibraryView) => void;
  onSave: (name: string) => void;
  onUpdate: (view: SavedLibraryView) => void;
  onDelete: (view: SavedLibraryView) => void;
  onTogglePinned: (view: SavedLibraryView) => void;
  /** Consulta que hay ahora mismo en pantalla, para saber si ya está guardada. */
  currentQuery: SavedViewQuery;
  conflicts: ViewConflict[];
  statuses?: FilterChoice[] | undefined;
  collections?: FilterChoice[] | undefined;
}

function choiceMap(choices?: FilterChoice[]): Map<string, string> {
  return new Map((choices ?? []).map((choice) => [choice.id, choice.name]));
}

export function SavedViewsBar(props: SavedViewsBarProps) {
  const { creating, onCreatingChange } = props;
  const [draft, setDraft] = useState("");

  const context = useMemo(
    () => ({ statuses: choiceMap(props.statuses), collections: choiceMap(props.collections) }),
    [props.statuses, props.collections],
  );

  // Con una sola vista activa se puede comprobar si la consulta sigue siendo la
  // guardada. Con varias apiladas el resultado es una mezcla, así que la única
  // acción sensata es guardar la mezcla como vista nueva.
  const single =
    props.activeIds.length === 1
      ? props.views.find((view) => view.id === props.activeIds[0])
      : undefined;
  const drifted = single ? !queryMatchesView(props.currentQuery, single) : false;

  function submit(event: React.FormEvent) {
    event.preventDefault();
    const name = draft.trim();
    if (!name) return;
    props.onSave(name);
    setDraft("");
    onCreatingChange(false);
  }

  if (props.views.length === 0 && !creating) return null;

  return (
    <div className="saved-views">
      <ul className="saved-views__list">
        {props.views.map((view) => {
          const active = props.activeIds.includes(view.id);
          const order = props.activeIds.indexOf(view.id) + 1;
          return (
            <li key={view.id}>
              {/* Las mismas opciones que el botón «⋯», al alcance del gesto que
                  se prueba primero. Sin él, había que dar con un botón de tres
                  puntos que sólo aparece al pasar por encima. */}
              <ContextMenu>
                <Tooltip>
                  <TooltipTrigger asChild>
                    <ContextMenuTrigger asChild>
                      <button
                        type="button"
                        className="saved-view"
                        data-active={active}
                        data-accent={view.accent}
                        aria-pressed={active}
                        onClick={() => props.onToggle(view)}
                      >
                        {view.pinned ? <IconPin size={12} /> : <IconBookmark size={12} />}
                        <span className="saved-view__name">{view.name}</span>
                        {active && props.activeIds.length > 1 && (
                          <span className="saved-view__order" aria-hidden="true">
                            {order}
                          </span>
                        )}
                      </button>
                    </ContextMenuTrigger>
                  </TooltipTrigger>
                  <TooltipContent>
                    <p className="saved-view__summary">{describeView(view, context)}</p>
                    {view.description && <p>{view.description}</p>}
                  </TooltipContent>
                </Tooltip>
                <ContextMenuContent aria-label={`Acciones rápidas de ${view.name}`}>
                  <ContextMenuLabel>{view.name}</ContextMenuLabel>
                  <ContextMenuSeparator />
                  <ContextMenuItem onSelect={() => props.onToggle(view)}>
                    <IconBookmark aria-hidden="true" />
                    {active ? "Quitar del filtro" : "Aplicar"}
                  </ContextMenuItem>
                  <ContextMenuItem onSelect={() => props.onTogglePinned(view)}>
                    <IconPin aria-hidden="true" />
                    {view.pinned ? "Desanclar" : "Anclar al principio"}
                  </ContextMenuItem>
                  <ContextMenuItem
                    disabled={!active || !drifted}
                    onSelect={() => props.onUpdate(view)}
                  >
                    <IconBookmark aria-hidden="true" /> Actualizar con lo que veo
                  </ContextMenuItem>
                  <ContextMenuSeparator />
                  <ContextMenuItem variant="destructive" onSelect={() => props.onDelete(view)}>
                    <IconX aria-hidden="true" /> Eliminar vista
                  </ContextMenuItem>
                </ContextMenuContent>
              </ContextMenu>
              <Popover>
                <PopoverTrigger asChild>
                  <button
                    type="button"
                    className="saved-view__menu"
                    aria-label={`Opciones de ${view.name}`}
                  >
                    ⋯
                  </button>
                </PopoverTrigger>
                <PopoverContent align="start" className="saved-view__actions">
                  <button type="button" onClick={() => props.onTogglePinned(view)}>
                    <IconPin size={14} />
                    {view.pinned ? "Desanclar" : "Anclar al principio"}
                  </button>
                  <button
                    type="button"
                    onClick={() => props.onUpdate(view)}
                    disabled={!active || !drifted}
                    title={
                      active
                        ? drifted
                          ? undefined
                          : "La vista ya coincide con lo que ves"
                        : "Aplica la vista antes de actualizarla"
                    }
                  >
                    <IconBookmark size={14} />
                    Actualizar con lo que veo
                  </button>
                  <button
                    type="button"
                    className="saved-view__danger"
                    onClick={() => props.onDelete(view)}
                  >
                    <IconX size={14} />
                    Eliminar vista
                  </button>
                </PopoverContent>
              </Popover>
            </li>
          );
        })}
      </ul>

      {creating && (
        <form className="saved-views__form" onSubmit={submit}>
          <Input
            // El campo aparece por una acción explícita y escribir el nombre es
            // lo único que queda por hacer.
            ref={(node) => node?.focus()}
            value={draft}
            onChange={(event) => setDraft(event.target.value)}
            placeholder="Nombre de la vista"
            aria-label="Nombre de la vista"
            maxLength={60}
            onKeyDown={(event) => {
              if (event.key === "Escape") {
                onCreatingChange(false);
                setDraft("");
              }
            }}
          />
          <Button type="submit" size="sm" disabled={!draft.trim()}>
            Guardar
          </Button>
          <Button
            type="button"
            variant="ghost"
            size="sm"
            onClick={() => {
              onCreatingChange(false);
              setDraft("");
            }}
          >
            Cancelar
          </Button>
        </form>
      )}

      {props.conflicts.length > 0 && (
        <p className="saved-views__conflicts" role="status">
          <IconAlertTriangle size={13} />
          <span>
            {props.conflicts.length === 1
              ? "Una condición no se puede cumplir a la vez:"
              : `${props.conflicts.length} condiciones no se pueden cumplir a la vez:`}{" "}
            {props.conflicts
              .map((conflict) => `${conflict.label} ${conflict.discarded} → ${conflict.kept}`)
              .join(" · ")}
          </span>
        </p>
      )}
    </div>
  );
}
