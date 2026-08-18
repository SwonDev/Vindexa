import { createContext, type ReactNode, useContext, useSyncExternalStore } from "react";

const REDUCED_MOTION_QUERY = "(prefers-reduced-motion: reduce)";

/**
 * `"auto"` delega en el sistema operativo. `true` o `false` fuerzan el valor,
 * para que un ajuste de la aplicación («Reducir animaciones») pueda mandar sobre
 * la preferencia del sistema sin tocar cada componente.
 */
export type ReduceMotionSetting = "auto" | boolean;

const MotionPreferencesContext = createContext<ReduceMotionSetting>("auto");

interface MotionPreferencesProviderProps {
  reduceMotion?: ReduceMotionSetting | undefined;
  children: ReactNode;
}

export function MotionPreferencesProvider({
  reduceMotion = "auto",
  children,
}: MotionPreferencesProviderProps) {
  return (
    <MotionPreferencesContext.Provider value={reduceMotion}>
      {children}
    </MotionPreferencesContext.Provider>
  );
}

function readMediaQuery(): MediaQueryList | undefined {
  if (typeof window === "undefined" || typeof window.matchMedia !== "function") return undefined;
  return window.matchMedia(REDUCED_MOTION_QUERY);
}

function subscribe(onStoreChange: () => void): () => void {
  const query = readMediaQuery();
  if (!query) return () => undefined;
  if (typeof query.addEventListener === "function") {
    query.addEventListener("change", onStoreChange);
    return () => query.removeEventListener("change", onStoreChange);
  }
  // Safari antiguo y algunos entornos empotrados solo exponen la API obsoleta.
  query.addListener(onStoreChange);
  return () => query.removeListener(onStoreChange);
}

function getSnapshot(): boolean {
  return readMediaQuery()?.matches ?? false;
}

function getServerSnapshot(): boolean {
  // Sin ventana no hay forma de saberlo: se asume la opción conservadora.
  return true;
}

/**
 * Devuelve `true` cuando hay que suprimir el movimiento por completo — no
 * atenuarlo. Es la única fuente de verdad de esta carpeta: todos los
 * componentes la consultan antes de animar nada.
 */
export function useReducedMotion(): boolean {
  const override = useContext(MotionPreferencesContext);
  const system = useSyncExternalStore(subscribe, getSnapshot, getServerSnapshot);
  return override === "auto" ? system : override;
}
