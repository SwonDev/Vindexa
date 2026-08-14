# Registro de decisiones arquitectónicas

Los ADR documentan decisiones vigentes que condicionan datos, seguridad y evolución. No
sustituyen el código ni [ARCHITECTURE.md](../../ARCHITECTURE.md).

| ADR | Estado | Decisión |
| --- | --- | --- |
| [0001](./0001-sqlite-local-first.md) | Aceptada | SQLite local como fuente de verdad. |
| [0002](./0002-separar-catalogo-y-organizacion.md) | Aceptada | Separar metadatos Steam de organización personal. |
| [0003](./0003-secretos-y-openid.md) | Aceptada | OpenID oficial y secretos fuera de SQLite/frontend. |
| [0004](./0004-frontera-nativa-tauri.md) | Aceptada | Operaciones privilegiadas detrás de comandos Rust. |
| [0005](./0005-tienda-steam-aislada.md) | Aceptada | Tienda remota en una ventana aislada y limitada. |

## Convención

Un ADR aceptado no se reescribe para ocultar una decisión posterior. Si cambia la
arquitectura, añade otro ADR que lo sustituya y enlaza ambos estados.
