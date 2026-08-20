import { IconAdjustmentsHorizontal, IconInfoCircle, IconX } from "@tabler/icons-react";
import { StaggerList } from "@/components/motion";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Popover, PopoverContent, PopoverTrigger } from "@/components/ui/popover";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import {
  activeLibraryFilterCount,
  type FilterChoice,
  filterChips,
  type LibraryFilterOptions,
  type LibraryFilters,
  normalizeLibraryFilters,
} from "@/features/library/library-filters";

interface Props {
  filters: LibraryFilters;
  onChange: (filters: LibraryFilters) => void;
  statuses: FilterChoice[];
  collections: FilterChoice[];
  options?: LibraryFilterOptions | undefined;
}

const ANY = "__vindexa_any__";
const YES = "yes";
const NO = "no";
const filterPresets = [
  { id: "unplayed", label: "Sin estrenar", filters: { neverPlayed: true } },
  {
    id: "installed-unplayed",
    label: "Instalados pendientes",
    filters: { installed: true, neverPlayed: true },
  },
  {
    id: "nearly-finished",
    label: "Casi terminados",
    filters: { minProgress: 75, maxProgress: 99 },
  },
  { id: "short-sessions", label: "Sesiones cortas", filters: { maxSessionMinutes: 60 } },
  { id: "tracking", label: "En seguimiento", filters: { tracking: true } },
  { id: "early-access", label: "Early Access", filters: { earlyAccess: true } },
] as const satisfies readonly {
  id: string;
  label: string;
  filters: LibraryFilters;
}[];

export function LibraryFiltersPopover({
  filters,
  onChange,
  statuses,
  collections,
  options,
}: Props) {
  const normalized = normalizeLibraryFilters(filters);
  const activeCount = activeLibraryFilterCount(normalized);
  const chips = filterChips(normalized, {
    statuses,
    collections,
    tags: options?.tags ?? [],
  });
  const metadataAvailable = Boolean(options?.metadataGames);
  const achievementsAvailable = Boolean(options?.achievementGames);
  const deckAvailable = Boolean(options?.steamDeckGames);
  const drmAvailable = Boolean(options?.drmGames);
  const coverage = options?.totalGames
    ? `${options.metadataGames.toLocaleString("es-ES")} de ${options.totalGames.toLocaleString("es-ES")}`
    : "0";
  const set = <K extends keyof LibraryFilters>(key: K, value: LibraryFilters[K]) =>
    onChange(normalizeLibraryFilters({ ...normalized, [key]: value }));

  return (
    <>
      <Popover>
        <PopoverTrigger asChild>
          <Button
            variant="secondary"
            size="sm"
            className="filter-button"
            aria-label={activeCount ? `Filtros, ${activeCount} activos` : "Filtros"}
          >
            <IconAdjustmentsHorizontal /> <span>Filtros</span>
            {activeCount > 0 && <Badge>{activeCount}</Badge>}
          </Button>
        </PopoverTrigger>
        <PopoverContent align="end" className="filters-popover">
          <div className="filters-popover__header">
            <div>
              <p className="popover-title">FILTROS COMBINABLES</p>
              <p className="filters-popover__subtitle">
                Todos se aplican en SQLite antes de paginar.
              </p>
            </div>
            <Button
              type="button"
              variant="ghost"
              size="sm"
              disabled={activeCount === 0}
              aria-label="Restablecer todos los filtros"
              onClick={() => onChange({})}
            >
              Restablecer
            </Button>
          </div>

          <fieldset className="filter-presets">
            <legend>ATAJOS</legend>
            <div>
              {filterPresets.map((preset) => {
                const selected = Object.entries(preset.filters).every(
                  ([key, value]) => normalized[key as keyof LibraryFilters] === value,
                );
                const disabled = preset.id === "early-access" && !metadataAvailable;
                return (
                  <Button
                    key={preset.id}
                    type="button"
                    variant="ghost"
                    size="sm"
                    aria-pressed={selected}
                    disabled={disabled}
                    onClick={() =>
                      onChange(normalizeLibraryFilters({ ...normalized, ...preset.filters }))
                    }
                  >
                    {preset.label}
                  </Button>
                );
              })}
            </div>
          </fieldset>

          <div className="filters-popover__scroll">
            <FilterSection title="BIBLIOTECA">
              <FilterSelect
                label="Estado personal"
                value={normalized.statusId}
                onChange={(value) => set("statusId", value)}
                choices={statuses}
                anyLabel="Todos los estados"
              />
              <TriStateSelect
                label="Instalación"
                value={normalized.installed}
                yes="Instalados"
                no="No instalados"
                onChange={(value) => set("installed", value)}
              />
              <TriStateSelect
                label="Tiempo jugado"
                value={normalized.neverPlayed}
                yes="Nunca jugados"
                no="Ya jugados"
                onChange={(value) => set("neverPlayed", value)}
              />
              <RangeFields
                label="Horas jugadas"
                minLabel="Horas mínimas jugadas"
                maxLabel="Horas máximas jugadas"
                min={toHours(normalized.minPlaytimeMinutes)}
                max={toHours(normalized.maxPlaytimeMinutes)}
                onMinChange={(value) => set("minPlaytimeMinutes", toMinutes(value))}
                onMaxChange={(value) => set("maxPlaytimeMinutes", toMinutes(value))}
                minValue={0}
                step={0.5}
              />
              <RangeFields
                label="Progreso"
                minLabel="Progreso mínimo"
                maxLabel="Progreso máximo"
                min={normalized.minProgress}
                max={normalized.maxProgress}
                onMinChange={(value) => set("minProgress", value)}
                onMaxChange={(value) => set("maxProgress", value)}
                minValue={0}
                maxValue={100}
                suffix="%"
              />
              <RangeFields
                label="Valoración"
                minLabel="Valoración mínima"
                maxLabel="Valoración máxima"
                min={normalized.minRating}
                max={normalized.maxRating}
                onMinChange={(value) => set("minRating", value)}
                onMaxChange={(value) => set("maxRating", value)}
                minValue={1}
                maxValue={10}
                suffix="/10"
              />
            </FilterSection>

            <FilterSection title="CLASIFICACIÓN">
              <FilterSelect
                label="Género"
                value={normalized.genre}
                onChange={(value) => set("genre", value)}
                choices={(options?.genres ?? []).map((name) => ({ id: name, name }))}
                anyLabel="Todos los géneros"
                disabled={!metadataAvailable}
              />
              <FilterSelect
                label="Categoría"
                value={normalized.category}
                onChange={(value) => set("category", value)}
                choices={(options?.categories ?? []).map((name) => ({ id: name, name }))}
                anyLabel="Todas las categorías"
                disabled={!metadataAvailable}
              />
              <FilterSelect
                label="Etiqueta personal"
                value={normalized.tagId}
                onChange={(value) => set("tagId", value)}
                choices={options?.tags ?? []}
                anyLabel="Todas las etiquetas"
                disabled={!options?.tags.length}
              />
              <FilterSelect
                label="Colección"
                value={normalized.collectionId}
                onChange={(value) => set("collectionId", value)}
                choices={collections}
                anyLabel="Todas las colecciones"
                disabled={!collections.length}
              />
              <AvailabilityNote>
                Metadatos de Steam disponibles en {coverage} juegos. Los juegos sin datos no se
                presentan como coincidencias.
              </AvailabilityNote>
            </FilterSection>

            <FilterSection title="FECHAS">
              <DateRange
                label="Fecha de lanzamiento"
                minLabel="Lanzado desde"
                maxLabel="Lanzado hasta"
                min={normalized.releaseFrom}
                max={normalized.releaseTo}
                disabled={!metadataAvailable}
                onMinChange={(value) => set("releaseFrom", value)}
                onMaxChange={(value) => set("releaseTo", value)}
              />
              <DateRange
                label="Última vez jugado"
                minLabel="Jugado desde"
                maxLabel="Jugado hasta"
                min={normalized.lastPlayedFrom}
                max={normalized.lastPlayedTo}
                onMinChange={(value) => set("lastPlayedFrom", value)}
                onMaxChange={(value) => set("lastPlayedTo", value)}
              />
              <DateRange
                label="Fecha objetivo"
                minLabel="Objetivo desde"
                maxLabel="Objetivo hasta"
                min={normalized.targetDateFrom}
                max={normalized.targetDateTo}
                onMinChange={(value) => set("targetDateFrom", value)}
                onMaxChange={(value) => set("targetDateTo", value)}
              />
            </FilterSection>

            <FilterSection title="STEAM Y PLANIFICACIÓN">
              <TriStateSelect
                label="Seguimiento"
                value={normalized.tracking}
                yes="En seguimiento"
                no="Sin seguimiento"
                onChange={(value) => set("tracking", value)}
              />
              <TriStateSelect
                label="Early Access"
                value={normalized.earlyAccess}
                yes="Early Access"
                no="Fuera de Early Access"
                disabled={!metadataAvailable}
                onChange={(value) => set("earlyAccess", value)}
              />
              <RangeFields
                label="Logros desbloqueados"
                minLabel="Porcentaje mínimo de logros"
                maxLabel="Porcentaje máximo de logros"
                min={normalized.minAchievementPercent}
                max={normalized.maxAchievementPercent}
                disabled={!achievementsAvailable}
                onMinChange={(value) => set("minAchievementPercent", value)}
                onMaxChange={(value) => set("maxAchievementPercent", value)}
                minValue={0}
                maxValue={100}
                suffix="%"
              />
              {!achievementsAvailable && (
                <AvailabilityNote>
                  Se habilitará cuando hayas actualizado los logros de al menos un juego.
                </AvailabilityNote>
              )}
              {/* El DRM no se marca en la carátula: una carátula es del juego,
                  no de cómo se distribuye. Aquí sí, porque aquí se pregunta
                  justo eso: qué parte de la biblioteca no depende de nadie. */}
              <FilterSelect
                label="Protección anticopia (DRM)"
                value={normalized.drmState}
                onChange={(value) => set("drmState", value)}
                choices={[
                  { id: "drm_free", name: "Sin DRM" },
                  { id: "third_party_drm", name: "DRM de terceros" },
                  { id: "steam_drm", name: "Steam DRM" },
                  { id: "unknown", name: "Sin comprobar" },
                ]}
                anyLabel="Cualquier protección"
                disabled={!drmAvailable}
              />
              {!drmAvailable && (
                <AvailabilityNote>
                  Se habilitará cuando Vindexa haya comprobado el DRM de al menos un juego, cosa que
                  ocurre al enriquecer sus fichas.
                </AvailabilityNote>
              )}
              <FilterSelect
                label="Compatibilidad con Steam Deck"
                value={normalized.steamDeckStatus}
                onChange={(value) => set("steamDeckStatus", value)}
                choices={[
                  { id: "verified", name: "Verificado" },
                  { id: "playable", name: "Jugable" },
                  { id: "unsupported", name: "No compatible" },
                  { id: "unknown", name: "Steam no lo ha valorado" },
                ]}
                anyLabel="Cualquier compatibilidad"
                disabled={!deckAvailable}
              />
              {!deckAvailable && (
                <AvailabilityNote>
                  Vindexa lo pregunta al informe público de la tienda por tandas, en segundo
                  plano; el filtro se habilita en cuanto haya un juego valorado.
                </AvailabilityNote>
              )}
              <RangeFields
                label="Duración media de sesión"
                minLabel="Duración media mínima de sesión"
                maxLabel="Duración media máxima de sesión"
                min={toHours(normalized.minSessionMinutes)}
                max={toHours(normalized.maxSessionMinutes)}
                onMinChange={(value) => set("minSessionMinutes", toMinutes(value))}
                onMaxChange={(value) => set("maxSessionMinutes", toMinutes(value))}
                minValue={0}
                step={0.5}
                suffix="h"
              />
              <AvailabilityNote>
                Se calcula únicamente con sesiones terminadas registradas en Vindexa; los juegos sin
                historial no coinciden con este rango.
              </AvailabilityNote>
            </FilterSection>
          </div>
        </PopoverContent>
      </Popover>

      {chips.length > 0 && (
        // Los filtros activos aparecen escalonados. Es la única señal de que se
        // ha aplicado algo —la lista de debajo simplemente cambia— y verlos
        // entrar uno detrás de otro cuenta que son varios sin tener que
        // contarlos. Aquí no hay arrastre, así que nada se pelea por la misma
        // transformación.
        <StaggerList
          as="div"
          className="filter-chips"
          role="status"
          aria-label="Filtros activos"
          itemAsChild
          stepMs={18}
        >
          {chips.map((chip) => (
            <button
              key={chip.key}
              type="button"
              className="filter-chip"
              aria-label={`Quitar filtro ${chip.label}`}
              title={chip.label}
              onClick={() => onChange(chip.remove(normalized))}
            >
              <span>{chip.label}</span>
              <IconX aria-hidden="true" size={12} />
            </button>
          ))}
        </StaggerList>
      )}
    </>
  );
}

function FilterSection({ title, children }: { title: string; children: React.ReactNode }) {
  return (
    <fieldset className="filter-section">
      <legend>{title}</legend>
      <div className="filter-section__grid">{children}</div>
    </fieldset>
  );
}

function AvailabilityNote({ children }: { children: React.ReactNode }) {
  return (
    <p className="filter-availability">
      <IconInfoCircle aria-hidden="true" size={14} />
      <span>{children}</span>
    </p>
  );
}

function FilterSelect({
  label,
  value,
  onChange,
  choices,
  anyLabel,
  disabled = false,
}: {
  label: string;
  value?: string | undefined;
  onChange: (value?: string) => void;
  choices: FilterChoice[];
  anyLabel: string;
  disabled?: boolean | undefined;
}) {
  return (
    <div className="filter-field">
      <span>{label}</span>
      <Select
        value={value ?? ANY}
        disabled={disabled}
        onValueChange={(next) => onChange(next === ANY ? undefined : next)}
      >
        <SelectTrigger aria-label={label} size="sm">
          <SelectValue />
        </SelectTrigger>
        <SelectContent position="popper">
          <SelectItem value={ANY}>{anyLabel}</SelectItem>
          {choices.map((choice) => (
            <SelectItem key={choice.id} value={choice.id}>
              {choice.name}
            </SelectItem>
          ))}
        </SelectContent>
      </Select>
    </div>
  );
}

function TriStateSelect({
  label,
  value,
  yes,
  no,
  onChange,
  disabled = false,
}: {
  label: string;
  value?: boolean | undefined;
  yes: string;
  no: string;
  onChange: (value?: boolean) => void;
  disabled?: boolean | undefined;
}) {
  return (
    <div className="filter-field">
      <span>{label}</span>
      <Select
        value={value === undefined ? ANY : value ? YES : NO}
        disabled={disabled}
        onValueChange={(next) => onChange(next === ANY ? undefined : next === YES)}
      >
        <SelectTrigger aria-label={label} size="sm">
          <SelectValue />
        </SelectTrigger>
        <SelectContent position="popper">
          <SelectItem value={ANY}>Cualquiera</SelectItem>
          <SelectItem value={YES}>{yes}</SelectItem>
          <SelectItem value={NO}>{no}</SelectItem>
        </SelectContent>
      </Select>
    </div>
  );
}

function RangeFields({
  label,
  minLabel,
  maxLabel,
  min,
  max,
  onMinChange,
  onMaxChange,
  minValue,
  maxValue,
  step = 1,
  suffix,
  disabled = false,
}: {
  label: string;
  minLabel: string;
  maxLabel: string;
  min?: number | undefined;
  max?: number | undefined;
  onMinChange: (value?: number) => void;
  onMaxChange: (value?: number) => void;
  minValue: number;
  maxValue?: number | undefined;
  step?: number | undefined;
  suffix?: string | undefined;
  disabled?: boolean | undefined;
}) {
  return (
    <div className="filter-field filter-field--wide">
      <span>{label}</span>
      <div className="filter-range">
        <NumberField
          label={minLabel}
          placeholder="Mín."
          value={min}
          min={minValue}
          max={maxValue}
          step={step}
          disabled={disabled}
          onChange={onMinChange}
        />
        <span aria-hidden="true">–</span>
        <NumberField
          label={maxLabel}
          placeholder="Máx."
          value={max}
          min={minValue}
          max={maxValue}
          step={step}
          disabled={disabled}
          onChange={onMaxChange}
        />
        {suffix && <small>{suffix}</small>}
      </div>
    </div>
  );
}

function NumberField({
  label,
  value,
  onChange,
  ...props
}: {
  label: string;
  value?: number | undefined;
  onChange: (value?: number) => void;
  placeholder: string;
  min: number;
  max?: number | undefined;
  step: number;
  disabled: boolean;
}) {
  return (
    <Input
      {...props}
      type="number"
      inputMode="decimal"
      aria-label={label}
      value={value ?? ""}
      onChange={(event) => {
        const raw = event.currentTarget.value;
        onChange(raw === "" ? undefined : Number(raw));
      }}
    />
  );
}

function DateRange({
  label,
  minLabel,
  maxLabel,
  min,
  max,
  onMinChange,
  onMaxChange,
  disabled = false,
}: {
  label: string;
  minLabel: string;
  maxLabel: string;
  min?: string | undefined;
  max?: string | undefined;
  onMinChange: (value?: string) => void;
  onMaxChange: (value?: string) => void;
  disabled?: boolean | undefined;
}) {
  return (
    <div className="filter-field filter-field--wide">
      <span>{label}</span>
      <div className="filter-range filter-range--date">
        <Input
          type="date"
          aria-label={minLabel}
          value={min ?? ""}
          disabled={disabled}
          onChange={(event) => onMinChange(event.currentTarget.value || undefined)}
        />
        <span aria-hidden="true">–</span>
        <Input
          type="date"
          aria-label={maxLabel}
          value={max ?? ""}
          disabled={disabled}
          onChange={(event) => onMaxChange(event.currentTarget.value || undefined)}
        />
      </div>
    </div>
  );
}

function toHours(minutes?: number): number | undefined {
  return minutes === undefined ? undefined : minutes / 60;
}

function toMinutes(hours?: number): number | undefined {
  return hours === undefined ? undefined : Math.round(hours * 60);
}
