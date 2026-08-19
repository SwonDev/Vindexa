/**
 * Acciones rápidas del botón derecho en la barra lateral.
 *
 * La barra lateral es donde más se navega, así que es donde más caro sale tener
 * que ir a otra pantalla para un cambio de un segundo. Aquí viven los dos menús
 * que evitan ese viaje: el de una colección y el de una tienda.
 *
 * # Qué se puede hacer y qué no
 *
 * El menú de una colección cambia **su apariencia**, nada más: color e icono.
 * Renombrarla, describirla o tocar las reglas de una inteligente sigue estando
 * en su editor, que es donde se ve lo que se está cambiando. La orden que se
 * envía tampoco puede hacer más que eso: `set_collection_appearance` escribe
 * dos columnas y ninguna otra, así que un menú rápido no puede llevarse por
 * delante las reglas de una colección por accidente.
 *
 * El de un estado cambia su color por el mismo motivo, y el de una tienda abre
 * su navegador o pide una sincronización. Nada de lo que hay aquí borra ni
 * reescribe datos de la biblioteca: lo que sí lo hace vive en su pantalla, con
 * su confirmación.
 *
 * # Por qué avisan por fuera
 *
 * Un menú contextual se cierra al elegir, así que no puede enseñar el resultado
 * de lo que ha lanzado. Sincronizar una tienda tarda y puede fallar, de modo que
 * el aviso se delega en quien monta el menú a través de `onNotice`: la barra
 * lateral lo pinta donde se ve aunque el menú ya no esté.
 */

import { IconExternalLink, IconPalette, IconRefresh, IconShape } from "@tabler/icons-react";
import { useMutation, useQueryClient } from "@tanstack/react-query";
import type { ReactNode } from "react";
import {
  ContextMenu,
  ContextMenuContent,
  ContextMenuItem,
  ContextMenuLabel,
  ContextMenuSeparator,
  ContextMenuSub,
  ContextMenuSubContent,
  ContextMenuSubTrigger,
  ContextMenuTrigger,
} from "@/components/ui/context-menu";
import { collectionIconOptions } from "@/features/collections/CollectionIcon";
import { api } from "@/lib/tauri";
import type { AppBootstrap, ExternalStoreId } from "@/lib/types";

type Collection = NonNullable<AppBootstrap["collections"]>[number];
type Status = NonNullable<AppBootstrap["statuses"]>[number];

/** Aviso que la barra lateral pinta cuando el menú ya se ha cerrado. */
export interface SidebarNotice {
  kind: "pending" | "success" | "error";
  message: string;
}

export type NotifySidebar = (notice: SidebarNotice | undefined) => void;

function mensajeDeError(cause: unknown): string {
  if (cause && typeof cause === "object" && "message" in cause) {
    const message = (cause as { message?: unknown }).message;
    if (typeof message === "string" && message.trim()) return message;
  }
  return "No se pudo completar la acción.";
}

/**
 * Colores que se ofrecen en el menú rápido.
 *
 * Son los mismos que ya usan las colecciones de ejemplo de la aplicación, para
 * que una colección creada desde aquí no desentone de las que vienen hechas. El
 * editor completo sigue admitiendo cualquier color.
 */
export const QUICK_COLLECTION_COLORS = [
  { value: "#5CAAC1", label: "Azul" },
  { value: "#0D6F9F", label: "Azul intenso" },
  { value: "#4BB8A9", label: "Turquesa" },
  { value: "#7EA64B", label: "Verde" },
  { value: "#A4D007", label: "Lima" },
  { value: "#D6A64B", label: "Ámbar" },
  { value: "#C97A3E", label: "Naranja" },
  { value: "#C1655C", label: "Rojo" },
  { value: "#C15C9B", label: "Magenta" },
  { value: "#9B7EC1", label: "Violeta" },
  { value: "#6E7BC1", label: "Índigo" },
  { value: "#8A939E", label: "Gris" },
] as const;

export function CollectionContextMenu({
  collection,
  children,
  onEdit,
  onDelete,
}: {
  collection: Collection;
  children: ReactNode;
  onEdit: () => void;
  onDelete: () => void;
}) {
  const queryClient = useQueryClient();
  const apariencia = useMutation({
    mutationFn: ({ color, icon }: { color: string; icon: string }) =>
      api.setCollectionAppearance(collection.id, color, icon),
    onSuccess: () => queryClient.invalidateQueries({ queryKey: ["bootstrap"] }),
  });

  return (
    <ContextMenu>
      <ContextMenuTrigger asChild>{children}</ContextMenuTrigger>
      <ContextMenuContent aria-label={`Acciones rápidas de ${collection.name}`}>
        <ContextMenuLabel>{collection.name}</ContextMenuLabel>
        <ContextMenuSeparator />

        <ContextMenuSub>
          <ContextMenuSubTrigger>
            <IconPalette aria-hidden="true" /> Color
          </ContextMenuSubTrigger>
          {/* En rejilla se ve la paleta entera de un vistazo, que es como se
              elige un color: comparándolo con los demás, no leyendo su nombre.
              El nombre sigue estando para quien no distingue el color y para
              quien navega con lector de pantalla. */}
          <ContextMenuSubContent className="sidebar-picker sidebar-picker--colors">
            {QUICK_COLLECTION_COLORS.map((color) => (
              <ContextMenuItem
                key={color.value}
                aria-label={color.label}
                title={color.label}
                data-selected={collection.color.toLowerCase() === color.value.toLowerCase()}
                onSelect={() => apariencia.mutate({ color: color.value, icon: collection.icon })}
              >
                <span
                  className="sidebar-swatch"
                  style={{ background: color.value }}
                  aria-hidden="true"
                />
              </ContextMenuItem>
            ))}
          </ContextMenuSubContent>
        </ContextMenuSub>

        <ContextMenuSub>
          <ContextMenuSubTrigger>
            <IconShape aria-hidden="true" /> Icono
          </ContextMenuSubTrigger>
          {/* Cuarenta y dos iconos en lista serían cuarenta y dos renglones que
              recorrer. En rejilla caben en un panel y se reconocen por su
              forma, que es para lo que sirven. */}
          <ContextMenuSubContent className="sidebar-picker sidebar-picker--icons">
            {collectionIconOptions.map((option) => {
              const OptionIcon = option.icon;
              return (
                <ContextMenuItem
                  key={option.value}
                  aria-label={option.label}
                  title={option.label}
                  data-selected={collection.icon === option.value}
                  onSelect={() =>
                    apariencia.mutate({ color: collection.color, icon: option.value })
                  }
                >
                  <OptionIcon aria-hidden="true" style={{ color: collection.color }} />
                </ContextMenuItem>
              );
            })}
          </ContextMenuSubContent>
        </ContextMenuSub>

        <ContextMenuSeparator />
        {/* Lo que cambia el significado de la colección vive en su editor, que
            es donde se ve entero lo que se está tocando. */}
        <ContextMenuItem onSelect={onEdit}>Editar colección…</ContextMenuItem>
        <ContextMenuItem data-variant="destructive" onSelect={onDelete}>
          Eliminar colección
        </ContextMenuItem>
      </ContextMenuContent>
    </ContextMenu>
  );
}

/**
 * Acciones rápidas de un estado.
 *
 * El color de un estado es su única seña en la barra lateral y en cada tarjeta,
 * así que cambiarlo es el ajuste que más se pide y el que más caro salía: había
 * que abrir el editor de estados para mover un solo valor. Renombrarlo, moverlo
 * de sitio o borrarlo siguen ahí, porque afectan a los juegos que lo llevan.
 */
export function StatusContextMenu({
  status,
  children,
  onEdit,
}: {
  status: Status;
  children: ReactNode;
  onEdit?: (() => void) | undefined;
}) {
  const queryClient = useQueryClient();
  const color = useMutation({
    // El nombre viaja igual porque `save_status` escribe la fila entera; se
    // manda el que ya tiene para no cambiar nada más que el color.
    mutationFn: (value: string) => api.saveStatus(status.id, status.name, value),
    onSuccess: () => queryClient.invalidateQueries({ queryKey: ["bootstrap"] }),
  });

  return (
    <ContextMenu>
      <ContextMenuTrigger asChild>{children}</ContextMenuTrigger>
      <ContextMenuContent aria-label={`Acciones rápidas de ${status.name}`}>
        <ContextMenuLabel>{status.name}</ContextMenuLabel>
        <ContextMenuSeparator />
        <ContextMenuSub>
          <ContextMenuSubTrigger>
            <IconPalette aria-hidden="true" /> Color
          </ContextMenuSubTrigger>
          <ContextMenuSubContent className="sidebar-picker sidebar-picker--colors">
            {QUICK_COLLECTION_COLORS.map((option) => (
              <ContextMenuItem
                key={option.value}
                aria-label={option.label}
                title={option.label}
                data-selected={status.color.toLowerCase() === option.value.toLowerCase()}
                onSelect={() => color.mutate(option.value)}
              >
                <span
                  className="sidebar-swatch"
                  style={{ background: option.value }}
                  aria-hidden="true"
                />
              </ContextMenuItem>
            ))}
          </ContextMenuSubContent>
        </ContextMenuSub>
        {onEdit && (
          <>
            <ContextMenuSeparator />
            <ContextMenuItem onSelect={onEdit}>Editar estados…</ContextMenuItem>
          </>
        )}
      </ContextMenuContent>
    </ContextMenu>
  );
}

export function StoreContextMenu({
  storeId,
  storeLabel,
  children,
  onNotice,
}: {
  storeId: string;
  storeLabel: string;
  children: ReactNode;
  onNotice?: NotifySidebar | undefined;
}) {
  const queryClient = useQueryClient();
  const abrir = useMutation({
    mutationFn: () => api.openStoreBrowser(storeId),
    onError: (cause) => onNotice?.({ kind: "error", message: mensajeDeError(cause) }),
  });
  const sincronizar = useMutation({
    mutationFn: () => api.scanExternalStore(storeId as ExternalStoreId),
    onMutate: () => onNotice?.({ kind: "pending", message: `Sincronizando ${storeLabel}…` }),
    onSuccess: (report) =>
      onNotice?.({
        kind: report.status === "success" ? "success" : "error",
        message:
          report.status === "success"
            ? `${storeLabel}: ${report.discovered.toLocaleString("es-ES")} juegos encontrados, ${report.matched.toLocaleString("es-ES")} emparejados con Steam.`
            : `${storeLabel}: ${report.errorMessage ?? "la sincronización no terminó."}`,
      }),
    onError: (cause) =>
      onNotice?.({ kind: "error", message: `${storeLabel}: ${mensajeDeError(cause)}` }),
    onSettled: () => queryClient.invalidateQueries({ queryKey: ["bootstrap"] }),
  });

  return (
    <ContextMenu>
      <ContextMenuTrigger asChild>{children}</ContextMenuTrigger>
      <ContextMenuContent aria-label={`Acciones rápidas de ${storeLabel}`}>
        <ContextMenuLabel>{storeLabel}</ContextMenuLabel>
        <ContextMenuSeparator />
        {/* Cada tienda tiene su propio almacén de datos, así que la ventana se
            abre con la sesión que ya tengas iniciada en ella. */}
        <ContextMenuItem disabled={abrir.isPending} onSelect={() => abrir.mutate()}>
          <IconExternalLink aria-hidden="true" /> Abrir en el navegador integrado
        </ContextMenuItem>
        {/* Trae lo comprado desde la última vez sin pasar por Ajustes. No borra
            nada: añade lo nuevo y vuelve a emparejar lo que ya estaba. */}
        <ContextMenuItem disabled={sincronizar.isPending} onSelect={() => sincronizar.mutate()}>
          <IconRefresh aria-hidden="true" /> Sincronizar ahora
        </ContextMenuItem>
      </ContextMenuContent>
    </ContextMenu>
  );
}
