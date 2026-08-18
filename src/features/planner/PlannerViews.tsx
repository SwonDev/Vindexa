import {
  IconCalendar,
  IconChevronDown,
  IconChevronUp,
  IconClock,
  IconFlag3,
  IconGauge,
  IconPencil,
} from "@tabler/icons-react";
import { type FormEvent, useEffect, useState } from "react";
import { Artwork } from "@/components/common/Artwork";
import { ProgressMeter } from "@/components/common/ProgressMeter";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
  DialogTrigger,
} from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { formatDate, formatPlaytime } from "@/lib/format";
import type { PlannerSettings, SavePlannerItemInput } from "@/lib/types";
import type { PlannerMetricItem, PlannerPeriodResult } from "./planner-periods";

export interface PlannerViewItem extends PlannerMetricItem {
  coverUrl?: string;
  position: number;
  queuePosition: number;
}

function optional(value: string): string | null {
  const trimmed = value.trim();
  return trimmed || null;
}

export function PlannerCapacityEditor({
  settings,
  onSave,
}: {
  settings: PlannerSettings;
  onSave: (settings: PlannerSettings) => Promise<void>;
}) {
  const [open, setOpen] = useState(false);
  const [weeklyHours, setWeeklyHours] = useState(String(settings.weeklyCapacityMinutes / 60));
  const [monthlyHours, setMonthlyHours] = useState(String(settings.monthlyCapacityMinutes / 60));
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string>();

  useEffect(() => {
    if (!open) return;
    setWeeklyHours(String(settings.weeklyCapacityMinutes / 60));
    setMonthlyHours(String(settings.monthlyCapacityMinutes / 60));
    setError(undefined);
  }, [open, settings]);

  const submit = async (event: FormEvent) => {
    event.preventDefault();
    const weekly = Number(weeklyHours);
    const monthly = Number(monthlyHours);
    if (!Number.isFinite(weekly) || !Number.isFinite(monthly) || weekly < 1 || monthly < 1) {
      setError("La capacidad debe ser de al menos una hora.");
      return;
    }
    if (monthly < weekly) {
      setError("La capacidad mensual debe ser igual o mayor que la semanal.");
      return;
    }
    setSaving(true);
    setError(undefined);
    try {
      await onSave({
        weeklyCapacityMinutes: Math.round(weekly * 60),
        monthlyCapacityMinutes: Math.round(monthly * 60),
      });
      setOpen(false);
    } catch (caught) {
      setError(caught instanceof Error ? caught.message : "No se pudo guardar la capacidad.");
    } finally {
      setSaving(false);
    }
  };

  return (
    <Dialog open={open} onOpenChange={setOpen}>
      <DialogTrigger asChild>
        <Button variant="outline" size="sm" aria-label="Ajustar capacidad">
          <IconGauge /> Capacidad
        </Button>
      </DialogTrigger>
      <DialogContent className="planner-capacity-dialog">
        <DialogHeader>
          <DialogTitle>Capacidad de juego</DialogTitle>
          <DialogDescription>
            Vindexa señalará la sobrecarga sin eliminar ni reordenar tu plan.
          </DialogDescription>
        </DialogHeader>
        <form className="planner-item-form" onSubmit={submit}>
          <div className="planner-item-form__dates">
            <label htmlFor="planner-weekly-capacity">
              <span>Capacidad semanal en horas</span>
              <Input
                id="planner-weekly-capacity"
                type="number"
                min="1"
                max="10000"
                step="0.5"
                value={weeklyHours}
                onChange={(event) => setWeeklyHours(event.target.value)}
              />
            </label>
            <label htmlFor="planner-monthly-capacity">
              <span>Capacidad mensual en horas</span>
              <Input
                id="planner-monthly-capacity"
                type="number"
                min="1"
                max="40000"
                step="0.5"
                value={monthlyHours}
                onChange={(event) => setMonthlyHours(event.target.value)}
              />
            </label>
          </div>
          {error && (
            <p className="planner-form-error" role="alert">
              {error}
            </p>
          )}
          <DialogFooter>
            <Button type="button" variant="outline" onClick={() => setOpen(false)}>
              Cancelar
            </Button>
            <Button type="submit" disabled={saving}>
              {saving ? "Guardando…" : "Guardar capacidad"}
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}

export function PlannerItemEditor({
  item,
  onSave,
}: {
  item: PlannerViewItem;
  onSave: (input: SavePlannerItemInput) => Promise<void>;
}) {
  const [open, setOpen] = useState(false);
  const [objective, setObjective] = useState(item.objective ?? "");
  const [plannedFor, setPlannedFor] = useState(item.plannedFor ?? "");
  const [targetDate, setTargetDate] = useState(item.targetDate ?? "");
  const [estimatedHours, setEstimatedHours] = useState(
    item.estimatedMinutes === undefined ? "" : String(item.estimatedMinutes / 60),
  );
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string>();

  useEffect(() => {
    if (!open) return;
    setObjective(item.objective ?? "");
    setPlannedFor(item.plannedFor ?? "");
    setTargetDate(item.targetDate ?? "");
    setEstimatedHours(
      item.estimatedMinutes === undefined ? "" : String(item.estimatedMinutes / 60),
    );
    setError(undefined);
  }, [item, open]);

  const submit = async (event: FormEvent) => {
    event.preventDefault();
    const hours = estimatedHours.trim() ? Number(estimatedHours) : undefined;
    if (hours !== undefined && (!Number.isFinite(hours) || hours < 0 || hours > 10_000)) {
      setError("La estimación debe estar entre 0 y 10.000 horas.");
      return;
    }
    if (objective.trim().length > 160) {
      setError("El objetivo no puede superar 160 caracteres.");
      return;
    }
    setSaving(true);
    setError(undefined);
    try {
      await onSave({
        appId: item.appId,
        objective: optional(objective),
        plannedFor: optional(plannedFor),
        targetDate: optional(targetDate),
        estimatedMinutes: hours === undefined ? null : Math.round(hours * 60),
      });
      setOpen(false);
    } catch (caught) {
      setError(caught instanceof Error ? caught.message : "No se pudo guardar la planificación.");
    } finally {
      setSaving(false);
    }
  };

  return (
    <Dialog open={open} onOpenChange={setOpen}>
      <DialogTrigger asChild>
        <Button variant="ghost" size="icon-xs" aria-label={`Planificar ${item.title}`}>
          <IconPencil />
        </Button>
      </DialogTrigger>
      <DialogContent className="planner-item-dialog">
        <DialogHeader>
          <DialogTitle>Planificar {item.title}</DialogTitle>
          <DialogDescription>
            Define cuándo quieres jugarlo y qué resultado concreto quieres conseguir.
          </DialogDescription>
        </DialogHeader>
        <form className="planner-item-form" onSubmit={submit}>
          <label htmlFor={`planner-objective-${item.appId}`}>
            <span>Objetivo</span>
            <Input
              id={`planner-objective-${item.appId}`}
              value={objective}
              onChange={(event) => setObjective(event.target.value)}
              maxLength={160}
              placeholder="Ej. terminar la campaña principal"
            />
          </label>
          <div className="planner-item-form__dates">
            <label htmlFor={`planner-planned-${item.appId}`}>
              <span>Programado para</span>
              <Input
                id={`planner-planned-${item.appId}`}
                type="date"
                value={plannedFor}
                onChange={(event) => setPlannedFor(event.target.value)}
              />
            </label>
            <label htmlFor={`planner-target-${item.appId}`}>
              <span>Fecha objetivo</span>
              <Input
                id={`planner-target-${item.appId}`}
                type="date"
                value={targetDate}
                min={plannedFor || undefined}
                onChange={(event) => setTargetDate(event.target.value)}
              />
            </label>
          </div>
          <label htmlFor={`planner-estimate-${item.appId}`}>
            <span>Horas estimadas</span>
            <Input
              id={`planner-estimate-${item.appId}`}
              type="number"
              inputMode="decimal"
              min="0"
              max="10000"
              step="0.25"
              value={estimatedHours}
              onChange={(event) => setEstimatedHours(event.target.value)}
              placeholder="0"
            />
          </label>
          {error && (
            <p className="planner-form-error" role="alert">
              {error}
            </p>
          )}
          <DialogFooter>
            <Button type="button" variant="outline" onClick={() => setOpen(false)}>
              Cancelar
            </Button>
            <Button type="submit" disabled={saving}>
              {saving ? "Guardando…" : "Guardar planificación"}
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}

export function PlannerQueueView({
  items,
  onMove,
  onEdit,
  moving = false,
}: {
  items: PlannerViewItem[];
  onMove: (appId: number, position: number) => void;
  onEdit: (input: SavePlannerItemInput) => Promise<void>;
  moving?: boolean;
}) {
  if (!items.length) {
    return (
      <div className="planner-view-empty" role="status">
        <IconFlag3 />
        <strong>Tu cola está vacía</strong>
        <span className="planner-view-empty__copy">
          Añade un juego desde cualquier columna para crear el siguiente paso.
        </span>
      </div>
    );
  }

  return (
    <ol className="planner-queue" aria-label="Cola lineal">
      {items.map((item, index) => (
        <li key={item.appId} className="planner-queue__item">
          <data value={index + 1}>{String(index + 1).padStart(2, "0")}</data>
          <Artwork appId={item.appId} src={item.coverUrl} title={item.title} kind="cover" />
          <PlannerItemSummary item={item} />
          <div className="planner-queue__actions">
            <Button
              variant="ghost"
              size="icon-xs"
              disabled={moving || index === 0}
              aria-label={`Subir ${item.title} en la cola`}
              onClick={() => onMove(item.appId, index - 1)}
            >
              <IconChevronUp />
            </Button>
            <Button
              variant="ghost"
              size="icon-xs"
              disabled={moving || index === items.length - 1}
              aria-label={`Bajar ${item.title} en la cola`}
              onClick={() => onMove(item.appId, index + 1)}
            >
              <IconChevronDown />
            </Button>
            <PlannerItemEditor item={item} onSave={onEdit} />
          </div>
        </li>
      ))}
    </ol>
  );
}

function PlannerItemSummary({ item }: { item: PlannerMetricItem }) {
  return (
    <div className="planner-item-summary">
      <strong>{item.title}</strong>
      {item.objective && <p>{item.objective}</p>}
      <ProgressMeter value={item.progress} label={`Progreso de ${item.title}`} />
      <div>
        {item.estimatedMinutes !== undefined && (
          <span>
            <IconClock /> {formatPlaytime(item.estimatedMinutes)}
          </span>
        )}
        {item.targetDate && (
          <span>
            <IconFlag3 /> {formatDate(item.targetDate)}
          </span>
        )}
      </div>
    </div>
  );
}

export function PlannerPeriodView({
  period,
  label,
  onEdit,
}: {
  period: PlannerPeriodResult;
  label: string;
  onEdit: (input: SavePlannerItemInput) => Promise<void>;
}) {
  return (
    <section className="planner-period" aria-label={label}>
      <div className="planner-period__segments">
        {period.segments.map((segment) => (
          <section className="planner-period__segment" key={segment.date}>
            <header>
              <IconCalendar />
              <h2>{segment.label}</h2>
              <data>{segment.items.length}</data>
            </header>
            <div className="planner-period__items">
              {segment.items.map((item) => {
                const viewItem: PlannerViewItem = {
                  ...item,
                  position: 0,
                  queuePosition: 0,
                };
                return (
                  <article className="planner-period-card" key={item.appId}>
                    <PlannerItemSummary item={item} />
                    <PlannerItemEditor item={viewItem} onSave={onEdit} />
                  </article>
                );
              })}
              {!segment.items.length && (
                <span className="planner-period__empty">Sin juegos programados</span>
              )}
            </div>
          </section>
        ))}
      </div>
      {period.unscheduled.length > 0 && (
        <footer className="planner-unscheduled">
          <strong>{period.unscheduled.length} sin programar</strong>
          <span>
            Asigna una fecha desde el lápiz de cualquier juego para incluirlo en el calendario.
          </span>
        </footer>
      )}
    </section>
  );
}
