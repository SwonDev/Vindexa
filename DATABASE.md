# Esquema y persistencia de Vindexa

SQLite es la fuente de verdad de Vindexa. Este documento describe el esquema vigente de la
migración **47** y sus invariantes; los archivos SQL de `src-tauri/migrations/` son la fuente
normativa ejecutable.

## Índice

- [Configuración de conexión](#configuración-de-conexión)
- [Relación principal](#relación-principal)
- [Tablas](#tablas)
- [Campos e invariantes relevantes](#campos-e-invariantes-relevantes)
- [Búsqueda e índices](#búsqueda-e-índices)
- [Migraciones](#migraciones)
- [Copia y restauración](#copia-y-restauración)
- [Ubicación y datos sensibles](#ubicación-y-datos-sensibles)

## Configuración de conexión

Cada conexión activa:

```sql
PRAGMA foreign_keys = ON;
PRAGMA journal_mode = WAL;
PRAGMA synchronous = FULL;
PRAGMA temp_store = MEMORY;
```

Además, Rust configura un `busy_timeout` de cinco segundos. Al iniciar se comprueban
`integrity_check`, `foreign_key_check`, identidad de aplicación, historial de migraciones y
huella del esquema.

## Relación principal

```mermaid
erDiagram
    GAMES ||--|| GAME_PERSONAL : organiza
    GAMES ||--o{ GAME_INSTALLATIONS : instala
    GAMES ||--o{ GAME_TAGS : etiqueta
    TAGS ||--o{ GAME_TAGS : define
    GAMES ||--o{ COLLECTION_GAMES : agrupa
    COLLECTIONS ||--o{ COLLECTION_GAMES : contiene
    COLLECTIONS ||--o{ SMART_RULES : evalua
    GAMES ||--o{ PLANNER_ITEMS : planifica
    PLANNER_COLUMNS ||--o{ PLANNER_ITEMS : contiene
    GAMES ||--o{ GAME_SESSIONS : registra
    GAMES ||--o{ ACTIVITY : genera
    GAMES ||--o{ GAME_REMINDERS : recuerda
    GAMES ||--o{ DISCOVERY_EVENTS : observa
    GAMES ||--o{ GAME_METADATA_OBSERVATIONS : compara
    GAMES ||--o{ RECOMMENDATION_HISTORY : recomienda
    GAMES ||--o{ IMAGE_CACHE : cachea
```

`family_catalog_games` se mantiene separado de `games`: que Steam Family muestre un AppID
no demuestra todavía que sea jugable desde el equipo. Solo la evidencia local confirmada
permite incorporarlo a la biblioteca personal como `family_shared`.

## Tablas

| Grupo | Tablas | Responsabilidad |
| --- | --- | --- |
| Control | `schema_migrations`, `app_settings` | Versiones aplicadas y preferencias no secretas. |
| Cuenta | `steam_accounts`, `openid_nonces`, `sync_runs` | Identidad, resultado de sincronización y replay prevention. |
| Catálogo | `games`, `family_catalog_games` | Metadatos de biblioteca propia/local y catálogo familiar separado. |
| Instalación/arte | `game_installations`, `image_cache` | Rutas, tamaño, build y referencias de caché. |
| Organización | `game_personal`, `statuses`, `tags`, `game_tags` | Estado, progreso, fechas, texto personal y clasificación transversal. |
| Colecciones | `collections`, `collection_games`, `smart_rules` | Grupos manuales, orden y reglas AND/OR. |
| Planificación | `planner_columns`, `planner_items`, `planner_settings` | Kanban, cola, periodos, objetivos y capacidad. |
| Historial | `game_sessions`, `activity`, `recommendation_history` | Sesiones, línea temporal y recomendaciones descartadas. |
| Descubrimiento | `game_reminders`, `game_metadata_observations`, `discovery_events`, `steam_news_items`, `steam_news_fetch_state` | Recordatorios, cambios observados, publicaciones oficiales cacheadas y cadencia/backoff. |
| Ficha enriquecida | `game_media` | Capturas y vídeos oficiales ordenados por juego. |
| Contenido adicional | `game_dlc` | DLC declarados por la tienda, con propiedad e instalación derivadas de evidencia local. |
| Listas curadas | `curated_lists`, `curated_list_items` | Selecciones editoriales propias con orden manual, nota y destacado. |
| Deseados | `wishlist_entries`, `catalog_games`, `catalog_wishlist_entries`, `game_videos` | Cubos de intención de compra —los de la biblioteca y los que aún no se tienen, que viven en el catálogo— y vídeos asociados a un juego. |
| Precios | `game_prices`, `game_price_history`, `game_price_checks` | Último precio observado por moneda, su evolución y la respuesta de la tienda cuando **no** hay precio, que no es lo mismo que no haber preguntado. |
| Ofertas | `store_deals` | Rebajas vigentes de Steam y GOG, con clave `(tienda, identificador)` porque los dos catálogos no comparten numeración. |
| Regalos | `epic_free_offers` | Lo que Epic regala cada semana, con su ventana y lo descartado a mano. |
| Vista rápida | `preview_screenshots`, `preview_screenshot_checks` | Capturas del emergente y el registro de a qué juegos ya se les preguntó, incluida la ausencia. |
| Avisos | `notification_rules`, `notification_events` | Recordatorios programables y eventos oficiales derivados sin duplicar. |
| Prioridad y gustos | `priority_signals`, `taste_weights`, `taste_feedback`, `upcoming_releases`, `upcoming_checks` | Puntuación explicable, modelo local de afinidad, próximos lanzamientos puntuados y a qué deseados ya se les preguntó por su fecha. |
| Tiendas externas | `external_store_accounts`, `external_games` | Detección local y por cuenta de Epic, GOG e itch.io, emparejado y estado DRM. |
| Archivo y vistas | `game_archive`, `saved_views`, `metadata_enrichment_queue` | Juegos apartados sin borrarlos, filtros guardados y la cola de enriquecimiento de fichas. |
| Agentes | `agent_clients`, `agent_audit_log`, `agent_tasks` | Clientes autorizados, registro auditable de cada acción automatizada y encargos programados. |

## Campos e invariantes relevantes

### `games`

- `app_id` es la clave estable y debe ser mayor que cero.
- Tiempo total y reciente nunca son negativos; una sincronización no reduce el total ya
  conocido.
- JSON de géneros/categorías usa arrays incluso cuando no hay datos.
- `metadata_status` y `achievements_status` distinguen `pending`, `success`, `unavailable`
  y `failed`; desconocido no se convierte en cero.
- `ownership_source` distingue `owned`, `family_shared` y `local`.
- `family_availability` distingue `not_applicable`, `unknown` y `confirmed`.

### `game_personal`

- Existe una fila por juego y referencia un estado válido.
- Progreso: 0–100; prioridad: 0–5; valoración opcional: 1–10.
- Instalado, fijado y seguimiento son booleanos restringidos.
- Fecha de finalización y abandono son mutuamente excluyentes y no pueden preceder al
  inicio.
- Steam no sobrescribe estos datos durante un upsert.

### Etiquetas y sesiones

- El nombre de una etiqueta, tras `trim`, tiene 1–40 caracteres y es único.
- El color es hexadecimal `#RRGGBB`.
- Cada juego admite como máximo 64 etiquetas y la sustitución del conjunto es transaccional.
- Una sesión requiere inicio válido; el final, si existe, no puede precederlo.
- Progreso antes/después es opcional y está entre 0 y 100.
- La nota de sesión admite como máximo 2.000 caracteres.
- El historial se ordena por inicio e ID descendentes y se pagina entre 1 y 100 filas por
  solicitud; la UI carga 50 y ofrece continuar.
- Borrar una etiqueta elimina sus asociaciones; borrar una sesión no borra el juego.

### Colecciones y drag and drop

- Una colección es `manual` o `smart`; `match_mode` es `all` o `any`.
- Solo una colección manual admite pertenencias persistidas y drop directo.
- `collection_games.position` conserva el orden estable.
- Un lote de drag and drop acepta hasta 10.000 AppID únicos y se aplica en una transacción.
- El recibo de deshacer contiene el estado u orden previo y solo se aplica si el destino no
  ha cambiado desde la operación original.

### Planificador

- Cada juego puede aparecer una sola vez gracias a `UNIQUE(app_id)`.
- La posición dentro de columna y `queue_position` son independientes.
- `planned_for` representa el periodo elegido; `target_date` la fecha objetivo.
- El objetivo está limitado a 160 caracteres.
- Capacidades semanal y mensual tienen límites positivos definidos en la migración 008.

## Búsqueda e índices

`game_search` es una tabla virtual FTS5 que indexa título, notas, checkpoint y próxima
acción. Triggers reconstruyen el documento al cambiar título o texto personal. Los filtros
de estado, instalación, seguimiento, progreso, valoración, tags, logros, Deck, fechas,
colecciones y planificador disponen de índices específicos antes de paginar.

El orden aleatorio recibe una semilla desde la interfaz para ser estable durante la página
actual. Todas las ordenaciones añaden título/AppID como desempate determinista.

## Migraciones

| Versión | Nombre | Cambio principal |
| ---: | --- | --- |
| 1 | `initial` | Esquema base, organización, planner, actividad y caché. |
| 2 | `indexes` | Índices, FTS5 y triggers de búsqueda. |
| 3 | `steam_sync_diagnostics` | Código y mensaje persistentes de fallo remoto. |
| 4 | `library_sorting` | Índices para ordenaciones de biblioteca. |
| 5 | `store_metadata` | Descripción y estado de metadatos públicos. |
| 6 | `game_hero` | Arte hero. |
| 7 | `steam_metadata_complete` | Procedencia, disponibilidad familiar y estado de logros. |
| 8 | `planner_advanced` | Cola, periodo, objetivo y capacidades. |
| 9 | `tracking_discovery` | Recordatorios, observaciones y eventos de descubrimiento. |
| 10 | `library_filters` | Índices para filtros combinables. |
| 11 | `family_catalog` | Catálogo familiar separado. |
| 12 | `personal_sessions_tags` | Validación e índices de tags, sesiones y fechas. |
| 13 | `legacy_ownership_provenance` | Corrección conservadora de procedencia heredada. |
| 14 | `metadata_enrichment_queue` | Cola persistente, prioridad y reintentos de metadatos. |
| 15 | `discovery_signals` | Caché y estado de refresco del feed oficial de Steam. |
| 16 | `manual_position_index` | Índice de la ordenación manual (posición, fijado, prioridad). |
| 17 | `game_capsule_url` | URL oficial de la cápsula 616×353 para el fallback de carátulas. |
| 18 | `drm_free_detection` | Clasificación DRM con su evidencia, derivada solo de señales oficiales. |
| 19 | `rich_game_metadata` | Descripción estructurada, idiomas, Metacritic, arte de alta resolución y `game_media`. |
| 20 | `game_dlc` | Catálogo de DLC por juego con propiedad, instalación y cola de refresco. |
| 21 | `curated_lists` | Listas curadas y sus entradas con orden manual, nota y destacado. |
| 22 | `wishlist_and_videos` | Deseados por cubo de intención y vídeos por juego. |
| 23 | `notifications` | Reglas programables y bandeja de eventos con deduplicación estable. |
| 24 | `priority_engine` | Puntuación de prioridad explicable, modelo de gustos y próximos lanzamientos. |
| 25 | `external_stores` | Cuentas y juegos detectados de Epic y GOG, con emparejado y confianza. |
| 26 | `agent_bridge` | Clientes de agente autorizados y registro auditable de sus acciones. |

> [!WARNING]
> Una migración ya publicada o aplicada es inmutable: no se edita su SQL. Una corrección se
> añade como nueva versión. Vindexa valida la huella para detectar divergencias.

La migración 13 solo demueve a `local` filas anteriores a la procedencia que carecen de toda
señal exclusiva de `GetOwnedGames`. Una sincronización oficial posterior vuelve a promover
el AppID a `owned`.

`games.drm_state` nunca se adivina: parte de `unknown` y solo cambia con una señal oficial
(`drm_notice`, `ext_user_account_notice`, `legal_notice` o las categorías de la tienda), que
queda registrada en `drm_evidence_json`. La marca es un dato de ficha y no se muestra sobre
las carátulas.

`game_dlc.owned` solo puede subir desde la importación: la única prueba disponible es el
manifiesto local de Steam, así que la ausencia de evidencia significa «sin confirmar», nunca
«no lo tienes». Bajarlo requiere una acción manual explícita.

`game_videos` guarda únicamente el identificador del proveedor. La URL de reproducción la
construye Rust contra `youtube-nocookie.com` tras validar que el identificador cumple el
formato exacto; el frontend jamás concatena esa dirección.

`priority_signals` y `taste_weights` se calculan en local a partir del comportamiento propio
y no salen del equipo. Una prioridad manual anclada (`priority_locked = 1`) nunca la pisa el
cálculo derivado.

`game_price_checks` guarda **la respuesta**, no el precio: cuándo se preguntó y qué contestó
la tienda cuando no publicó ninguno (`no_price` para lo que no está a la venta —sin fecha de
salida, gratuito o retirado— y `unavailable` para un AppID que no reconoce). Sin esa fila,
un juego preguntado sin precio es indistinguible de uno al que nadie preguntó nunca, y la
pantalla acusaba a la aplicación de no haber mirado. La fila se borra en cuanto llega un
precio de verdad.

`store_deals` tiene clave `(store, external_id)`: GOG numera sus productos a su manera y
compartir columna con los AppID de Steam sería apostar a que dos catálogos no coinciden nunca
en un número. `app_id` sólo existe cuando la oferta es de Steam, y es lo que permite cruzarla
con la biblioteca y enseñar sus capturas; en GOG es nulo y no se adivina por el título. Cada
tienda se sincroniza y se limpia por separado: la tanda de una nunca borra las filas de la
otra.

`steam_news_items` solo acepta el feed `steam_community_announcements`. La tabla de estado
conserva último intento/éxito, fallos consecutivos y próximo intento; no contiene la Web API
Key ni mensajes remotos. Un éxito reemplaza atómicamente el lote del juego y programa seis
horas de caché. Las relaciones entre lanzamientos no se guardan: se recalculan desde
`games.release_date`, `developer` y `publisher` para no quedar obsoletas.

## Copia y restauración

La exportación usa SQLite Online Backup y no copia la caché ni la Web API Key. La
restauración valida la base en solo lectura, crea un respaldo previo en el directorio de
datos, sustituye mediante SQLite y vuelve a ejecutar todos los gates. Ante un fallo,
restaura y verifica la base anterior.

No copies solo `vindexa.sqlite3` mientras la aplicación está abierta: WAL puede contener
cambios pendientes. Usa **Ajustes → Datos y copias → Exportar copia**.

## Ubicación y datos sensibles

La ruta real depende del sistema y se muestra en Diagnóstico. La base contiene SteamID64,
rutas, biblioteca, notas, checkpoints, fechas, etiquetas, sesiones y planificación en texto
legible. Protégela como información privada. La Web API Key vive en el almacén seguro y no
forma parte de SQLite.
