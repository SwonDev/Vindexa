import {
  IconArrowDown,
  IconArrowUp,
  IconCheck,
  IconColumns3,
  IconLoader2,
  IconPencil,
  IconPlus,
  IconTrash,
  IconX,
} from "@tabler/icons-react";
import { useMutation, useQueryClient } from "@tanstack/react-query";
import { useState } from "react";
import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
  AlertDialogTrigger,
} from "@/components/ui/alert-dialog";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { api, getErrorMessage } from "@/lib/tauri";
import type { AppBootstrap } from "@/lib/types";

interface StatusDraft {
  id: string;
  originalName: string;
  name: string;
  color: string;
}

interface ColumnDraft {
  id: string;
  originalName: string;
  name: string;
  color: string;
  wipLimit: string;
  itemCount: number;
}

export function OrganizationSettings({ bootstrap }: { bootstrap?: AppBootstrap | undefined }) {
  const queryClient = useQueryClient();
  const [statusName, setStatusName] = useState("");
  const [columnName, setColumnName] = useState("");
  const [statusDraft, setStatusDraft] = useState<StatusDraft>();
  const [columnDraft, setColumnDraft] = useState<ColumnDraft>();
  const [message, setMessage] = useState<string>();
  const refresh = () => queryClient.invalidateQueries({ queryKey: ["bootstrap"] });
  const mutation = useMutation({
    mutationFn: (operation: () => Promise<unknown>) => operation(),
    onSuccess: () => {
      setMessage("Organización actualizada.");
      void refresh();
    },
    onError: (error) => setMessage(getErrorMessage(error)),
  });
  const statuses = bootstrap?.statuses ?? [];
  const columns = bootstrap?.planner ?? [];
  const move = (
    ids: string[],
    index: number,
    direction: -1 | 1,
    save: (ids: string[]) => Promise<void>,
  ) => {
    const target = index + direction;
    if (target < 0 || target >= ids.length) return;
    const next = [...ids];
    const currentId = next[index];
    const targetId = next[target];
    if (!currentId || !targetId) return;
    next[index] = targetId;
    next[target] = currentId;
    mutation.mutate(() => save(next));
  };
  return (
    <div className="settings-section">
      <div className="settings-heading">
        <h3>Estados personales</h3>
        <p>
          Personaliza el vocabulario de tu biblioteca. Al eliminar un estado, sus juegos se
          trasladan a “Sin clasificar”.
        </p>
      </div>
      {message && (
        <p className="operation-message" role="status">
          {mutation.isPending ? "Guardando…" : message}
        </p>
      )}
      <div className="organization-list">
        {statuses.map((status, index) => (
          <div
            key={status.id}
            className="organization-row"
            data-editing={statusDraft?.id === status.id}
          >
            {statusDraft?.id === status.id ? (
              <form
                className="organization-inline-editor"
                onSubmit={(event) => {
                  event.preventDefault();
                  if (statusDraft.name.trim().length < 2) return;
                  mutation.mutate(
                    () =>
                      api.saveStatus(statusDraft.id, statusDraft.name.trim(), statusDraft.color),
                    { onSuccess: () => setStatusDraft(undefined) },
                  );
                }}
              >
                <input
                  className="organization-color"
                  type="color"
                  aria-label={`Color del estado ${statusDraft.originalName}`}
                  value={statusDraft.color}
                  onChange={(event) => {
                    const color = event.currentTarget.value;
                    setStatusDraft((current) => (current ? { ...current, color } : current));
                  }}
                />
                <Input
                  autoFocus
                  aria-label={`Nombre del estado ${statusDraft.originalName}`}
                  value={statusDraft.name}
                  maxLength={60}
                  onChange={(event) => {
                    const name = event.currentTarget.value;
                    setStatusDraft((current) => (current ? { ...current, name } : current));
                  }}
                />
                <Button
                  type="submit"
                  variant="ghost"
                  size="icon-xs"
                  aria-label={`Guardar cambios de ${statusDraft.originalName}`}
                  disabled={statusDraft.name.trim().length < 2 || mutation.isPending}
                >
                  {mutation.isPending ? <IconLoader2 className="is-spinning" /> : <IconCheck />}
                </Button>
                <Button
                  type="button"
                  variant="ghost"
                  size="icon-xs"
                  aria-label={`Cancelar edición de ${statusDraft.originalName}`}
                  disabled={mutation.isPending}
                  onClick={() => setStatusDraft(undefined)}
                >
                  <IconX />
                </Button>
              </form>
            ) : (
              <>
                <i style={{ backgroundColor: status.color }} />
                <div>
                  <strong>{status.name}</strong>
                  <span>
                    {status.gameCount} juegos{status.builtIn ? " · Predeterminado" : ""}
                  </span>
                </div>
                <Button
                  variant="ghost"
                  size="icon-xs"
                  aria-label={`Editar ${status.name}`}
                  disabled={mutation.isPending}
                  onClick={() =>
                    setStatusDraft({
                      id: status.id,
                      originalName: status.name,
                      name: status.name,
                      color: status.color,
                    })
                  }
                >
                  <IconPencil />
                </Button>
                <Button
                  variant="ghost"
                  size="icon-xs"
                  aria-label={`Subir ${status.name}`}
                  disabled={index === 0 || mutation.isPending}
                  onClick={() =>
                    move(
                      statuses.map((item) => item.id),
                      index,
                      -1,
                      api.reorderStatuses,
                    )
                  }
                >
                  <IconArrowUp />
                </Button>
                <Button
                  variant="ghost"
                  size="icon-xs"
                  aria-label={`Bajar ${status.name}`}
                  disabled={index === statuses.length - 1 || mutation.isPending}
                  onClick={() =>
                    move(
                      statuses.map((item) => item.id),
                      index,
                      1,
                      api.reorderStatuses,
                    )
                  }
                >
                  <IconArrowDown />
                </Button>
                {!status.builtIn && (
                  <ConfirmOrganizationDeletion
                    aria-label={`Eliminar ${status.name}`}
                    disabled={mutation.isPending}
                    title={`¿Eliminar el estado “${status.name}”?`}
                    description={`${status.gameCount} ${status.gameCount === 1 ? "juego se reasignará" : "juegos se reasignarán"} a “Sin clasificar”. No se eliminarán juegos ni datos personales.`}
                    confirmLabel="Eliminar estado"
                    onConfirm={() =>
                      mutation.mutate(() => api.deleteStatus(status.id, "unclassified"))
                    }
                  />
                )}
              </>
            )}
          </div>
        ))}
      </div>
      <form
        className="organization-create"
        onSubmit={(event) => {
          event.preventDefault();
          if (statusName.trim().length < 2) return;
          mutation.mutate(() => api.saveStatus(undefined, statusName.trim(), "#5CAAC1"), {
            onSuccess: () => setStatusName(""),
          });
        }}
      >
        <Input
          value={statusName}
          onChange={(event) => setStatusName(event.currentTarget.value)}
          placeholder="Nombre del nuevo estado"
          aria-label="Nombre del nuevo estado"
          maxLength={60}
        />
        <Button
          size="sm"
          type="submit"
          disabled={statusName.trim().length < 2 || mutation.isPending}
        >
          {mutation.isPending ? <IconLoader2 className="is-spinning" /> : <IconPlus />} Añadir
        </Button>
      </form>
      <div className="settings-divider" />
      <div className="settings-heading">
        <h3>Columnas del planificador</h3>
        <p>
          Ordena el flujo y edita aquí el nombre, el color y el límite de trabajo en curso de cada
          columna.
        </p>
      </div>
      <div className="organization-list">
        {columns.map((column, index) => {
          const replacement = columns.find((item) => item.id !== column.id);
          const editing = columnDraft?.id === column.id;
          const parsedLimit = columnDraft?.wipLimit.trim()
            ? Number(columnDraft.wipLimit)
            : undefined;
          const validLimit =
            parsedLimit === undefined ||
            (Number.isInteger(parsedLimit) &&
              parsedLimit > 0 &&
              parsedLimit >= column.items.length);
          return (
            <div key={column.id} className="organization-row" data-editing={editing}>
              {editing && columnDraft ? (
                <form
                  className="organization-inline-editor organization-inline-editor--column"
                  onSubmit={(event) => {
                    event.preventDefault();
                    if (columnDraft.name.trim().length < 2 || !validLimit) return;
                    mutation.mutate(
                      () =>
                        api.savePlannerColumn(
                          columnDraft.id,
                          columnDraft.name.trim(),
                          columnDraft.color,
                          parsedLimit,
                        ),
                      { onSuccess: () => setColumnDraft(undefined) },
                    );
                  }}
                >
                  <input
                    className="organization-color"
                    type="color"
                    aria-label={`Color de la columna ${columnDraft.originalName}`}
                    value={columnDraft.color}
                    onChange={(event) => {
                      const color = event.currentTarget.value;
                      setColumnDraft((current) => (current ? { ...current, color } : current));
                    }}
                  />
                  <Input
                    autoFocus
                    aria-label={`Nombre de la columna ${columnDraft.originalName}`}
                    value={columnDraft.name}
                    maxLength={60}
                    onChange={(event) => {
                      const name = event.currentTarget.value;
                      setColumnDraft((current) => (current ? { ...current, name } : current));
                    }}
                  />
                  <Input
                    type="number"
                    inputMode="numeric"
                    min={Math.max(1, columnDraft.itemCount)}
                    max={9999}
                    aria-label={`Límite WIP de ${columnDraft.originalName}`}
                    aria-invalid={!validLimit}
                    placeholder="Sin límite"
                    value={columnDraft.wipLimit}
                    onChange={(event) => {
                      const wipLimit = event.currentTarget.value;
                      setColumnDraft((current) => (current ? { ...current, wipLimit } : current));
                    }}
                  />
                  <Button
                    type="submit"
                    variant="ghost"
                    size="icon-xs"
                    aria-label={`Guardar cambios de ${columnDraft.originalName}`}
                    disabled={
                      columnDraft.name.trim().length < 2 || !validLimit || mutation.isPending
                    }
                  >
                    {mutation.isPending ? <IconLoader2 className="is-spinning" /> : <IconCheck />}
                  </Button>
                  <Button
                    type="button"
                    variant="ghost"
                    size="icon-xs"
                    aria-label={`Cancelar edición de ${columnDraft.originalName}`}
                    disabled={mutation.isPending}
                    onClick={() => setColumnDraft(undefined)}
                  >
                    <IconX />
                  </Button>
                  {!validLimit && (
                    <span className="organization-inline-editor__error">
                      Usa un límite entero igual o superior a {Math.max(1, columnDraft.itemCount)}.
                    </span>
                  )}
                </form>
              ) : (
                <>
                  <i style={{ backgroundColor: column.color }} />
                  <div>
                    <strong>{column.name}</strong>
                    <span>
                      {column.items.length} juegos
                      {column.wipLimit ? ` · Límite ${column.wipLimit}` : " · Sin límite"}
                    </span>
                  </div>
                  <Button
                    variant="ghost"
                    size="icon-xs"
                    aria-label={`Editar ${column.name}`}
                    disabled={mutation.isPending}
                    onClick={() =>
                      setColumnDraft({
                        id: column.id,
                        originalName: column.name,
                        name: column.name,
                        color: column.color,
                        wipLimit: column.wipLimit ? String(column.wipLimit) : "",
                        itemCount: column.items.length,
                      })
                    }
                  >
                    <IconPencil />
                  </Button>
                  <Button
                    variant="ghost"
                    size="icon-xs"
                    aria-label={`Subir ${column.name}`}
                    disabled={index === 0 || mutation.isPending}
                    onClick={() =>
                      move(
                        columns.map((item) => item.id),
                        index,
                        -1,
                        api.reorderPlannerColumns,
                      )
                    }
                  >
                    <IconArrowUp />
                  </Button>
                  <Button
                    variant="ghost"
                    size="icon-xs"
                    aria-label={`Bajar ${column.name}`}
                    disabled={index === columns.length - 1 || mutation.isPending}
                    onClick={() =>
                      move(
                        columns.map((item) => item.id),
                        index,
                        1,
                        api.reorderPlannerColumns,
                      )
                    }
                  >
                    <IconArrowDown />
                  </Button>
                  <ConfirmOrganizationDeletion
                    aria-label={`Eliminar ${column.name}`}
                    disabled={mutation.isPending || !replacement}
                    title={`¿Eliminar la columna “${column.name}”?`}
                    description={`${column.items.length} ${column.items.length === 1 ? "juego se moverá" : "juegos se moverán"} a “${replacement?.name ?? "otra columna"}”. Se conservarán el progreso y todos sus datos.`}
                    confirmLabel="Eliminar columna"
                    onConfirm={() =>
                      replacement &&
                      mutation.mutate(() => api.deletePlannerColumn(column.id, replacement.id))
                    }
                  />
                </>
              )}
            </div>
          );
        })}
      </div>
      <form
        className="organization-create"
        onSubmit={(event) => {
          event.preventDefault();
          if (columnName.trim().length < 2) return;
          mutation.mutate(() => api.savePlannerColumn(undefined, columnName.trim(), "#5CAAC1"), {
            onSuccess: () => setColumnName(""),
          });
        }}
      >
        <Input
          value={columnName}
          onChange={(event) => setColumnName(event.currentTarget.value)}
          placeholder="Nombre de la nueva columna"
          aria-label="Nombre de la nueva columna"
          maxLength={60}
        />
        <Button
          size="sm"
          type="submit"
          disabled={columnName.trim().length < 2 || mutation.isPending}
        >
          <IconColumns3 /> Añadir columna
        </Button>
      </form>
      <p className="settings-note">
        <IconCheck size={13} /> Los cambios de orden se guardan inmediatamente en SQLite.
      </p>
    </div>
  );
}

function ConfirmOrganizationDeletion({
  "aria-label": ariaLabel,
  disabled,
  title,
  description,
  confirmLabel,
  onConfirm,
}: {
  "aria-label": string;
  disabled: boolean;
  title: string;
  description: string;
  confirmLabel: string;
  onConfirm: () => void;
}) {
  return (
    <AlertDialog>
      <AlertDialogTrigger asChild>
        <Button variant="ghost" size="icon-xs" aria-label={ariaLabel} disabled={disabled}>
          <IconTrash />
        </Button>
      </AlertDialogTrigger>
      <AlertDialogContent>
        <AlertDialogHeader>
          <AlertDialogTitle>{title}</AlertDialogTitle>
          <AlertDialogDescription>{description}</AlertDialogDescription>
        </AlertDialogHeader>
        <AlertDialogFooter>
          <AlertDialogCancel>Cancelar</AlertDialogCancel>
          <AlertDialogAction onClick={onConfirm}>{confirmLabel}</AlertDialogAction>
        </AlertDialogFooter>
      </AlertDialogContent>
    </AlertDialog>
  );
}
