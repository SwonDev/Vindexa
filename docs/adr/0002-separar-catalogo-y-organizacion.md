# ADR 0002: Separar catálogo y organización personal

- Estado: Aceptada
- Fecha: 2026-08-14

## Contexto

Steam puede actualizar títulos, arte, tiempos y metadatos. Vindexa no puede permitir que una
respuesta remota borre estado, progreso, notas, etiquetas, sesiones o planificación.

## Decisión

`games` almacena catálogo y procedencia; `game_personal` y tablas relacionadas almacenan
decisiones del usuario. Los upserts Steam/locales actualizan solo su área y crean la fila
personal inicial si el AppID es nuevo. Steam Family usa además `family_catalog_games` para
no convertir disponibilidad ambigua en propiedad personal.

## Consecuencias

- La resincronización es segura para la organización privada.
- Procedencia propia, familiar y local se puede filtrar y corregir sin cambiar notas.
- Los datos ausentes se modelan como desconocidos, no como cero.
- Los queries combinan tablas, por lo que necesitan índices y contratos de migración.
