# ADR 0004: Operaciones privilegiadas detrás de Rust

- Estado: Aceptada
- Fecha: 2026-08-14

## Contexto

La UI necesita SQLite, red, archivos, diálogos, Keychain y esquemas `steam://`. Conceder
permisos generales al WebView ampliaría innecesariamente el impacto de un fallo frontend.

## Decisión

La ventana principal solo invoca una lista cerrada de comandos Tauri. Rust valida payloads,
construye URLs desde allowlists, canonicaliza rutas y realiza red/sistema. Las capabilities y
CSP de la ventana se mantienen mínimas; las operaciones SQLite bloqueantes usan la costura
de ejecución y locks de mantenimiento previstos.

## Consecuencias

- La UI queda desacoplada de SQL y detalles nativos.
- Errores cruzan IPC con forma estable y mensajes seguros.
- Añadir una operación requiere contrato TypeScript, comando Rust, validación y tests.
- Abrir una integración remota no justifica ampliar permisos de la ventana principal.
