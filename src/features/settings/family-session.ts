import { formatRelativeDate } from "@/lib/format";
import type { FamilySessionStatus, FamilySyncReport } from "@/lib/types";

/**
 * Frases del vínculo con la sesión de Steam para el catálogo de Familia.
 *
 * Viven fuera del componente porque son las reglas de honestidad de esta
 * pantalla —qué se puede afirmar y qué no— y esas se comprueban con datos
 * fijos, sin montar un diálogo entero.
 */

/** Estado del vínculo, sin adornar lo que no se sabe. */
export function describeFamilyStatus(status?: FamilySessionStatus): string {
  if (!status) return "Comprobando si hay una sesión de Steam vinculada…";
  if (!status.linked) {
    return "Sin sesión vinculada. La sincronización normal sólo ve los juegos de los miembros que tengan su biblioteca pública.";
  }
  // Un fallo reciente manda sobre el recuento anterior: decir «última lectura,
  // 3000 juegos» cuando el último intento acaba de fallar da por buena una cifra
  // que puede estar caducada.
  if (status.lastErrorCode) {
    return "Sesión vinculada. El último intento falló; el mensaje está justo debajo.";
  }
  if (status.lastSyncAt === undefined || status.lastAppCount === undefined) {
    return "Sesión vinculada. Todavía no se ha traído el catálogo.";
  }
  const cuando = formatRelativeDate(status.lastSyncAt);
  const cuantos =
    status.lastAppCount === 1 ? "1 juego" : `${status.lastAppCount.toLocaleString("es-ES")} juegos`;
  return `Sesión vinculada. Última lectura ${cuando}: ${cuantos} en el catálogo.`;
}

/** Resumen de lo que trajo una sincronización, contando también los huecos. */
export function describeFamilySync(report: FamilySyncReport): string {
  if (report.noFamily) {
    return "Tu cuenta de Steam no pertenece a ninguna Familia, así que no hay catálogo compartido que traer.";
  }
  const partes = [
    report.imported === 1
      ? "1 juego en el catálogo de tu Familia"
      : `${report.imported.toLocaleString("es-ES")} juegos en el catálogo de tu Familia`,
  ];
  // Los huecos se cuentan: son la mitad honesta del informe y lo que explica
  // por qué la cifra no cuadra con la que enseña el cliente de Steam.
  if (report.withoutTitle > 0) {
    partes.push(
      report.withoutTitle === 1
        ? "1 sin nombre publicado, que no se ha guardado"
        : `${report.withoutTitle} sin nombre publicado, que no se han guardado`,
    );
  }
  if (report.unusable > 0) {
    partes.push(`${report.unusable} entradas que Steam devolvió sin identificador`);
  }
  return `${partes.join("; ")}.`;
}
