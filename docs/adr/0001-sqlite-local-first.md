# ADR 0001: SQLite local como fuente de verdad

- Estado: Aceptada
- Fecha: 2026-08-14

## Contexto

Vindexa organiza datos privados, debe funcionar sin un servicio propio y preservar miles de
juegos, posiciones y textos durante reinicios y sincronizaciones.

## Decisión

SQLite, gestionado exclusivamente por Rust, es la fuente de verdad. Se usan migraciones
versionadas, claves foráneas, WAL, durabilidad `FULL`, índices, FTS5, transacciones y SQLite
Online Backup. El frontend consulta y muta mediante IPC tipado; almacenamiento web no es
persistencia principal.

## Consecuencias

- La aplicación funciona localmente y no necesita cuenta Vindexa.
- Backup/restauración puede ser portable entre builds compatibles.
- El usuario debe proteger el archivo porque el contenido personal no está cifrado por la
  aplicación.
- Cada cambio de esquema exige una migración aditiva y pruebas de upgrade.
- Sin un servicio propio no hay sincronización entre equipos ni recuperación en la nube.
