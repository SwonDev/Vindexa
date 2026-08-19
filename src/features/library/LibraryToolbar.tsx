import {
  IconBookmarkPlus,
  IconLayoutGrid,
  IconList,
  IconListDetails,
  IconSearch,
  IconX,
} from "@tabler/icons-react";
import { AnimatedNumber } from "@/components/motion";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import {
  Select,
  SelectContent,
  SelectGroup,
  SelectItem,
  SelectLabel,
  SelectSeparator,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";
import { LibraryFiltersPopover } from "@/features/library/LibraryFiltersPopover";
import type {
  FilterChoice,
  LibraryFilterOptions,
  LibraryFilters,
} from "@/features/library/library-filters";
import { LIBRARY_GROUPINGS, type LibraryGrouping } from "@/features/library/library-grouping";
import type { GameSort, LibraryView } from "@/lib/types";

export type ExtraFilters = LibraryFilters;

interface LibraryToolbarProps {
  mode?: "library" | "family";
  title: string;
  total?: number;
  query: string;
  onQueryChange: (value: string) => void;
  sort: GameSort;
  onSortChange: (value: GameSort) => void;
  view: LibraryView;
  onViewChange: (view: LibraryView) => void;
  grouping?: LibraryGrouping | undefined;
  onGroupingChange?: ((grouping: LibraryGrouping) => void) | undefined;
  filters: LibraryFilters;
  onFiltersChange: (filters: LibraryFilters) => void;
  statuses?: FilterChoice[] | undefined;
  collections?: FilterChoice[] | undefined;
  filterOptions?: LibraryFilterOptions | undefined;
  /** Congela la consulta actual como vista con nombre. */
  onSaveView?: (() => void) | undefined;
  /** Rótulo del botón: cambia al apilar varias vistas. */
  saveViewLabel?: string | undefined;
}

const sortGroups = [
  {
    label: "PERSONAL",
    options: [
      ["manual", "Prioridad manual"],
      ["random", "Orden aleatorio"],
    ],
  },
  {
    label: "ACTIVIDAD",
    options: [
      ["lastPlayed", "Jugados recientemente"],
      ["recentlyAdded", "Añadidos recientes"],
    ],
  },
  {
    label: "TÍTULO",
    options: [
      ["alphabetical", "Título: A–Z"],
      ["alphabeticalDesc", "Título: Z–A"],
    ],
  },
  {
    label: "LANZAMIENTO",
    options: [
      ["releaseDate", "Lanzamiento más reciente"],
      ["releaseDateAsc", "Lanzamiento más antiguo"],
    ],
  },
  {
    label: "TIEMPO DE JUEGO",
    options: [
      ["playtime", "Más horas jugadas"],
      ["playtimeAsc", "Menos horas jugadas"],
    ],
  },
  {
    label: "INSTALACIÓN",
    options: [
      ["installedFirst", "Instalados primero"],
      ["uninstalledFirst", "No instalados primero"],
      ["sizeDesc", "Mayor tamaño en disco"],
      ["sizeAsc", "Menor tamaño en disco"],
    ],
  },
  {
    label: "ORGANIZACIÓN",
    options: [
      ["progress", "Mayor progreso"],
      ["rating", "Mejor valoración"],
      ["targetDate", "Fecha objetivo más próxima"],
    ],
  },
] satisfies readonly {
  label: string;
  options: readonly (readonly [GameSort, string])[];
}[];

export function LibraryToolbar(props: LibraryToolbarProps) {
  const familyMode = props.mode === "family";
  return (
    <div className="library-toolbar">
      <div className="library-toolbar__primary">
        <div className="library-title">
          <h1 title={props.title}>{props.title}</h1>
          {typeof props.total === "number" && (
            /* Cuenta hacia el nuevo valor al filtrar o buscar: el número deja
               de saltar y se ve de dónde a dónde ha ido. */
            <span>
              <AnimatedNumber value={props.total} />
            </span>
          )}
        </div>
        <label className="search-field" htmlFor="library-search">
          <IconSearch aria-hidden="true" size={16} />
          <Input
            id="library-search"
            type="search"
            value={props.query}
            onChange={(event) => props.onQueryChange(event.currentTarget.value)}
            placeholder={
              familyMode ? "Buscar en Steam Family" : "Buscar en títulos, notas o checkpoints"
            }
            aria-label={familyMode ? "Buscar en Steam Family" : "Buscar en la biblioteca"}
          />
          {props.query && (
            <button
              type="button"
              aria-label="Limpiar búsqueda"
              onClick={() => props.onQueryChange("")}
            >
              <IconX size={14} />
            </button>
          )}
        </label>
        {
          <LibraryFiltersPopover
            filters={props.filters}
            onChange={props.onFiltersChange}
            statuses={props.statuses ?? []}
            collections={props.collections ?? []}
            options={props.filterOptions}
          />
        }
        {
          <Select
            value={props.sort}
            onValueChange={(value) => props.onSortChange(value as GameSort)}
          >
            <SelectTrigger className="sort-select" aria-label="Ordenar biblioteca">
              <SelectValue />
            </SelectTrigger>
            <SelectContent className="library-sort-menu">
              {sortGroups.map((group, groupIndex) => (
                <SelectGroup key={group.label}>
                  {groupIndex > 0 && <SelectSeparator />}
                  <SelectLabel>{group.label}</SelectLabel>
                  {group.options.map(([value, label]) => (
                    <SelectItem key={value} value={value}>
                      {label}
                    </SelectItem>
                  ))}
                </SelectGroup>
              ))}
            </SelectContent>
          </Select>
        }
        {/* El encabezado ocupa una fila entera del lienzo y el grupo siguiente
            arranca en la columna cero, así que la rejilla admite los mismos
            cortes que las listas sin romper ninguna fila. */}
        {props.onGroupingChange && (
          <Select
            value={props.grouping ?? "none"}
            onValueChange={(value) => props.onGroupingChange?.(value as LibraryGrouping)}
          >
            <SelectTrigger className="grouping-select" aria-label="Agrupar biblioteca">
              <SelectValue />
            </SelectTrigger>
            <SelectContent className="library-sort-menu">
              <SelectGroup>
                <SelectLabel>AGRUPAR POR</SelectLabel>
                {LIBRARY_GROUPINGS.map((option) => (
                  <SelectItem key={option.id} value={option.id} title={option.hint}>
                    {option.label}
                  </SelectItem>
                ))}
              </SelectGroup>
            </SelectContent>
          </Select>
        )}
        {props.onSaveView && (
          <Tooltip>
            <TooltipTrigger asChild>
              <Button
                variant="ghost"
                size="icon-sm"
                aria-label={props.saveViewLabel ?? "Guardar esta vista"}
                onClick={props.onSaveView}
              >
                <IconBookmarkPlus />
              </Button>
            </TooltipTrigger>
            <TooltipContent>{props.saveViewLabel ?? "Guardar esta vista"}</TooltipContent>
          </Tooltip>
        )}
        <fieldset className="view-switcher">
          <legend className="sr-only">
            {familyMode ? "Vista del catálogo de Steam Family" : "Vista de biblioteca"}
          </legend>
          <Tooltip>
            <TooltipTrigger asChild>
              <Button
                variant="ghost"
                size="icon-sm"
                data-active={props.view === "grid"}
                aria-label="Vista de cuadrícula"
                aria-pressed={props.view === "grid"}
                onClick={() => props.onViewChange("grid")}
              >
                <IconLayoutGrid />
              </Button>
            </TooltipTrigger>
            <TooltipContent>Cuadrícula</TooltipContent>
          </Tooltip>
          <Tooltip>
            <TooltipTrigger asChild>
              <Button
                variant="ghost"
                size="icon-sm"
                data-active={props.view === "list"}
                aria-label="Vista de lista"
                aria-pressed={props.view === "list"}
                onClick={() => props.onViewChange("list")}
              >
                <IconList />
              </Button>
            </TooltipTrigger>
            <TooltipContent>Lista</TooltipContent>
          </Tooltip>
          <Tooltip>
            <TooltipTrigger asChild>
              <Button
                variant="ghost"
                size="icon-sm"
                data-active={props.view === "compact"}
                aria-label="Vista ultracompacta"
                aria-pressed={props.view === "compact"}
                onClick={() => props.onViewChange("compact")}
              >
                <IconListDetails />
              </Button>
            </TooltipTrigger>
            <TooltipContent>Ultracompacta</TooltipContent>
          </Tooltip>
        </fieldset>
      </div>
    </div>
  );
}
