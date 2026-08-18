/**
 * Compatibilidad de un juego con el sistema en el que corre Vindexa.
 *
 * El dato viene de la tienda y **puede faltar**. Que falte no significa que el
 * juego sea incompatible: significa que no se sabe. Esa diferencia es la razón
 * de que este módulo exista en lugar de un booleano, porque presentar «no se
 * sabe» como «no puedes instalarlo» sería afirmar algo que nadie ha comprobado.
 */

import type { GameSummary } from "@/lib/types";

export type HostPlatform = "windows" | "mac" | "linux" | "unknown";

/** Compatibilidad del juego con esta máquina. */
export type PlatformSupport =
  /** La tienda dice que sí lo ofrece para este sistema. */
  | "supported"
  /** La tienda dice que no lo ofrece para este sistema. */
  | "unsupported"
  /** No hay dato: ni se afirma ni se niega. */
  | "unknown";

/** Sistema en el que corre la aplicación, deducido del agente de usuario. */
export function detectHostPlatform(userAgent: string): HostPlatform {
  if (/Macintosh|Mac OS X/i.test(userAgent)) return "mac";
  if (/Windows/i.test(userAgent)) return "windows";
  if (/Linux|X11/i.test(userAgent)) return "linux";
  return "unknown";
}

type PlatformFields = Pick<GameSummary, "platformWindows" | "platformMac" | "platformLinux">;

export function platformSupport(game: PlatformFields, host: HostPlatform): PlatformSupport {
  const declarado =
    host === "mac"
      ? game.platformMac
      : host === "windows"
        ? game.platformWindows
        : host === "linux"
          ? game.platformLinux
          : undefined;
  // `== null` cubre `null` **y** `undefined`. Es deliberado: el backend envía
  // `null` cuando la tienda no ha dicho nada, y compararlo sólo contra
  // `undefined` dejaba pasar el `null` al ternario, donde es falsy y se leía
  // como «no compatible». Un juego sin dato acababa marcado como imposible de
  // instalar, que es justo la afirmación que este módulo existe para evitar.
  if (declarado == null) return "unknown";
  return declarado ? "supported" : "unsupported";
}

const NOMBRES: Record<Exclude<HostPlatform, "unknown">, string> = {
  windows: "Windows",
  mac: "macOS",
  linux: "Linux",
};

/**
 * Frase que explica por qué no se ofrece instalar. Devuelve `undefined` cuando
 * no hay nada que advertir, para que quien la use no tenga que decidirlo.
 */
export function platformWarning(game: PlatformFields, host: HostPlatform): string | undefined {
  if (host === "unknown") return undefined;
  if (platformSupport(game, host) !== "unsupported") return undefined;
  const otras = (
    [
      ["windows", game.platformWindows],
      ["mac", game.platformMac],
      ["linux", game.platformLinux],
    ] as const
  )
    .filter(([sistema, ofrecido]) => ofrecido === true && sistema !== host)
    .map(([sistema]) => NOMBRES[sistema]);

  const cabecera = `Este juego no tiene versión para ${NOMBRES[host]}.`;
  if (otras.length === 0) return cabecera;
  return `${cabecera} La tienda solo lo ofrece para ${otras.join(" y ")}.`;
}
