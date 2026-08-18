"use client";

import { Switch as SwitchPrimitive } from "radix-ui";
import type * as React from "react";

import { cn } from "@/lib/utils";

/**
 * Interruptor de dos estados.
 *
 * # La aritmética, que estaba mal
 *
 * El carril tiene borde de 1 px y relleno de 2 px por lado, así que el hueco
 * interior mide **seis píxeles menos** que el carril. El pulgar tiene que caber
 * en ese hueco y recorrerlo entero:
 *
 * | Tamaño | Carril | Hueco | Pulgar | Recorrido |
 * | --- | --- | --- | --- | --- |
 * | `default` | 34 × 20 | 28 × 14 | 14 | 14 |
 * | `sm` | 26 × 16 | 20 × 10 | 10 | 10 |
 *
 * El pulgar medía 16 y 12: dos píxeles más alto que su hueco en los dos
 * tamaños. Por eso sobresalía por arriba, por abajo y por la derecha al estar
 * encendido, y parecía descentrado.
 *
 * # Por qué el pulgar no usa las variantes `dark:`
 *
 * Los tokens del proyecto se declaran en `:root, .dark`, pero **nada añade la
 * clase `dark` al documento**: la aplicación es oscura de raíz, sin conmutador
 * de tema. Las variantes `dark:` que traen los componentes de shadcn nunca
 * llegan a aplicarse, así que el pulgar se quedaba en `bg-background` —el fondo
 * de la aplicación— y salía un rectángulo oscuro sobre una pista azul.
 *
 * Aquí el color se escribe sin variante: el pulgar es claro en los dos estados,
 * que es lo que lo hace legible tanto sobre la pista apagada como sobre la
 * encendida.
 */
function Switch({
  className,
  size = "default",
  ...props
}: React.ComponentProps<typeof SwitchPrimitive.Root> & {
  size?: "sm" | "default";
}) {
  return (
    <SwitchPrimitive.Root
      data-slot="switch"
      data-size={size}
      className={cn(
        "peer group/switch relative inline-flex shrink-0 items-center rounded-[3px] border border-transparent shadow-xs transition-[background-color,border-color,box-shadow,opacity] outline-none after:absolute after:-inset-x-3 after:-inset-y-2 focus-visible:border-ring focus-visible:ring-3 focus-visible:ring-ring/50 aria-invalid:border-destructive aria-invalid:ring-3 aria-invalid:ring-destructive/20 overflow-hidden p-[2px] data-[size=default]:h-[20px] data-[size=default]:w-[34px] data-[size=sm]:h-[16px] data-[size=sm]:w-[26px] dark:aria-invalid:border-destructive/50 dark:aria-invalid:ring-destructive/40 data-checked:bg-primary data-unchecked:bg-input data-disabled:cursor-not-allowed data-disabled:opacity-50",
        className,
      )}
      {...props}
    >
      <SwitchPrimitive.Thumb
        data-slot="switch-thumb"
        className="pointer-events-none block rounded-[2px] bg-foreground ring-0 transition-transform group-data-[size=default]/switch:size-3.5 group-data-[size=sm]/switch:size-2.5 group-data-[size=default]/switch:data-checked:translate-x-[14px] group-data-[size=sm]/switch:data-checked:translate-x-[10px] group-data-[size=default]/switch:data-unchecked:translate-x-0 group-data-[size=sm]/switch:data-unchecked:translate-x-0"
      />
    </SwitchPrimitive.Root>
  );
}

export { Switch };
