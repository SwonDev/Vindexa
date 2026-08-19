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
 */

import { IconExternalLink, IconPalette, IconShape } from "@tabler/icons-react";
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
import type { AppBootstrap } from "@/lib/types";

type Collection = NonNullable<AppBootstrap["collections"]>[number];

/**
 * Colores que se ofrecen en el menú rápido.
 *
 * Son los mismos que ya usan las colecciones de ejemplo de la aplicación, para
 * que una colección creada desde aquí no desentone de las que vienen hechas. El
 * editor completo sigue admitiendo cualquier color.
 */
export const QUICK_COLLECTION_COLORS = [
  { value: "#5CAAC1", label: "Azul" },
  { value: "#7EA64B", label: "Verde" },
  { value: "#A4D007", label: "Lima" },
  { value: "#D6A64B", label: "Ámbar" },
  { value: "#C1655C", label: "Rojo" },
  { value: "#9B7EC1", label: "Violeta" },
  { value: "#C15C9B", label: "Magenta" },
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
          <ContextMenuSubContent className="sidebar-swatches">
            {QUICK_COLLECTION_COLORS.map((color) => (
              <ContextMenuItem
                key={color.value}
                aria-label={color.label}
                data-selected={collection.color.toLowerCase() === color.value.toLowerCase()}
                onSelect={() => apariencia.mutate({ color: color.value, icon: collection.icon })}
              >
                <span
                  className="sidebar-swatch"
                  style={{ background: color.value }}
                  aria-hidden="true"
                />
                {color.label}
              </ContextMenuItem>
            ))}
          </ContextMenuSubContent>
        </ContextMenuSub>

        <ContextMenuSub>
          <ContextMenuSubTrigger>
            <IconShape aria-hidden="true" /> Icono
          </ContextMenuSubTrigger>
          <ContextMenuSubContent>
            {collectionIconOptions.map((option) => {
              const OptionIcon = option.icon;
              return (
                <ContextMenuItem
                  key={option.value}
                  data-selected={collection.icon === option.value}
                  onSelect={() =>
                    apariencia.mutate({ color: collection.color, icon: option.value })
                  }
                >
                  <OptionIcon aria-hidden="true" /> {option.label}
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

export function StoreContextMenu({
  storeId,
  storeLabel,
  children,
}: {
  storeId: string;
  storeLabel: string;
  children: ReactNode;
}) {
  const abrir = useMutation({
    mutationFn: () => api.openStoreBrowser(storeId),
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
      </ContextMenuContent>
    </ContextMenu>
  );
}
