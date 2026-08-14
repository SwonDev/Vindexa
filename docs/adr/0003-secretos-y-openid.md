# ADR 0003: OpenID oficial y secretos fuera de SQLite

- Estado: Aceptada
- Fecha: 2026-08-14

## Contexto

Vindexa necesita un SteamID64 y, para ciertos endpoints, una Web API Key. No debe recibir la
contraseña, imitar el login ni filtrar la clave a React o backups.

## Decisión

La identificación usa Steam OpenID en navegador externo, callback loopback temporal,
verificación directa y prevención de replay. La Web API Key se guarda con `keyring`; SQLite
solo conserva un marcador no secreto. `bootstrap` no abre el almacén seguro.

## Consecuencias

- Steam gestiona contraseña, cookies y Steam Guard.
- La clave no viaja en backups ni payloads de lectura.
- Guardar, comprobar, sincronizar y eliminar pueden provocar el prompt legítimo del sistema.
- Si el almacén seguro no está disponible, la operación falla; no existe fallback plano.
- Un binario de desarrollo recompilado puede requerir nueva autorización de Keychain.
