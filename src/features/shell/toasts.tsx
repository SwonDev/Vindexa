/**
 * Avisos de la aplicación.
 *
 * # Por qué hace falta un sitio común
 *
 * Lo que se lanza desde un menú contextual no puede contarse en el propio menú:
 * el menú se cierra al elegir. Y lo que tarda —sincronizar una tienda, mover
 * cien juegos— tampoco puede quedarse sin respuesta. Cada pantalla que se topó
 * con eso se inventó su propia tira de aviso, así que había tres maneras
 * distintas de decir «ha ido bien» y ninguna de decir «deshaz esto».
 *
 * Aquí vive una sola: la pila de {@link ToastStack}, que ya sabe apilar, animar
 * la recolocación, respetar `prefers-reduced-motion` y —lo que importa— **no
 * cerrar un error solo**, porque un fallo es información que hay que leer.
 *
 * # Cómo se usa
 *
 * ```tsx
 * const toast = useToast();
 * toast.error("Epic Games Store: la sincronización no terminó.");
 * const id = toast.pending("Sincronizando Epic Games Store…");
 * toast.replace(id, { kind: "success", message: "Listo: 553 juegos." });
 * ```
 *
 * No hay almacén global ni singleton escondido: el estado vive en el proveedor
 * que monta la carcasa, y quien no esté dentro recibe una implementación muda
 * en vez de un error —una pantalla suelta en una prueba no debe romperse por no
 * tener avisos—.
 */

import {
  createContext,
  type ReactNode,
  useCallback,
  useContext,
  useMemo,
  useRef,
  useState,
} from "react";
import { type ToastItem, type ToastKind, ToastStack } from "@/components/motion";

export interface ToastApi {
  /** Añade un aviso y devuelve su identificador, por si hay que sustituirlo. */
  show: (toast: Omit<ToastItem, "id"> & { id?: string }) => string;
  info: (message: string) => string;
  success: (message: string) => string;
  error: (message: string, detail?: string) => string;
  /** Aviso que no se cierra solo porque algo está ocurriendo todavía. */
  pending: (message: string) => string;
  /** Sustituye un aviso en su sitio, sin que la pila dé un salto. */
  replace: (id: string, toast: Omit<ToastItem, "id">) => void;
  dismiss: (id: string) => void;
}

const MUTE: ToastApi = {
  show: () => "",
  info: () => "",
  success: () => "",
  error: () => "",
  pending: () => "",
  replace: () => undefined,
  dismiss: () => undefined,
};

const ToastContext = createContext<ToastApi>(MUTE);

/** Acceso a los avisos. Fuera del proveedor no falla: simplemente no avisa. */
export function useToast(): ToastApi {
  return useContext(ToastContext);
}

export function ToastProvider({ children }: { children: ReactNode }) {
  const [toasts, setToasts] = useState<ToastItem[]>([]);
  // Un contador, no la hora: dos avisos del mismo milisegundo compartirían
  // clave y la animación de entrada se comería uno. Vive en una referencia
  // porque sólo lo tocan los manejadores, nunca el pintado.
  const nextId = useRef(0);

  const dismiss = useCallback((id: string) => {
    setToasts((current) => current.filter((toast) => toast.id !== id));
  }, []);

  const api = useMemo<ToastApi>(() => {
    const push = (toast: Omit<ToastItem, "id"> & { id?: string }): string => {
      nextId.current += 1;
      const id = toast.id ?? `toast-${nextId.current}`;
      // Reusar un identificador sustituye el aviso en vez de duplicarlo: así
      // «Sincronizando…» se convierte en su resultado sin que la pila salte.
      setToasts((current) => [...current.filter((item) => item.id !== id), { ...toast, id }]);
      return id;
    };
    const simple = (kind: ToastKind) => (message: string) => push({ message, kind });
    return {
      show: push,
      info: simple("info"),
      success: simple("success"),
      error: (message: string, detail?: string) =>
        push({ message, kind: "error", ...(detail ? { detail } : {}) }),
      // Mientras algo está en marcha no hay resultado que contar, así que el
      // aviso se queda hasta que lo sustituya el resultado.
      pending: (message: string) => push({ message, kind: "info", autoDismissMs: 0 }),
      replace: (id: string, toast: Omit<ToastItem, "id">) => {
        setToasts((current) => current.map((item) => (item.id === id ? { ...toast, id } : item)));
      },
      dismiss,
    };
  }, [dismiss]);

  return (
    <ToastContext.Provider value={api}>
      {children}
      <ToastStack toasts={toasts} onDismiss={dismiss} position="bottom-right" />
    </ToastContext.Provider>
  );
}
