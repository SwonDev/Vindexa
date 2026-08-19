# Puente de agentes de Vindexa

Especificación pública de la capa de control que permite a un agente externo
—Hermes, o cualquier otro cliente local— conducir Vindexa con frases en lenguaje
natural. Cubre la versión de esquema **026** y el código de `src-tauri/src/agent/`.

Este documento describe **la capa de control, no el agente**. Vindexa no habla
con ningún modelo, no envía nada a la red y no interpreta lenguaje natural: eso
lo hace el agente, que traduce la frase a una llamada tipada de las que aquí se
describen. Para las fronteras generales de seguridad consulta
[SECURITY.md](../SECURITY.md); para el tratamiento de datos,
[PRIVACY.md](../PRIVACY.md).

## Índice

- [Principios](#principios)
- [Transporte](#transporte)
- [Sobre de la petición](#sobre-de-la-petición)
- [Catálogo de intenciones](#catálogo-de-intenciones)
- [Resolución de juegos por nombre](#resolución-de-juegos-por-nombre)
- [Ámbitos](#ámbitos)
- [Confirmación humana](#confirmación-humana)
- [Deshacer](#deshacer)
- [Registro de auditoría](#registro-de-auditoría)
- [Emisión y revocación de tokens](#emisión-y-revocación-de-tokens)
- [Límite de frecuencia](#límite-de-frecuencia)
- [Códigos de error](#códigos-de-error)
- [Frases de ejemplo y su llamada](#frases-de-ejemplo-y-su-llamada)
- [Qué queda fuera de esta fase](#qué-queda-fuera-de-esta-fase)

## Principios

1. **Catálogo cerrado.** El agente solo puede pedir lo que está en esta página.
   No hay una intención genérica de «ejecuta esto», ni SQL, ni borrado de
   juegos, colecciones, listas o estados.
2. **Ante la duda, se pregunta.** Un nombre que no es inequívoco devuelve
   candidatos y no modifica nada. Un agente que toca el juego equivocado hace un
   daño silencioso; uno que pregunta solo cuesta un mensaje.
3. **Nada ocurre en silencio.** Cada petición deja exactamente una fila en
   `agent_audit_log`, y el cambio y su fila se escriben en la misma transacción.
4. **Todo se puede deshacer.** Cada escritura devuelve un `undoToken` de un solo
   uso que restaura el estado anterior, y que caduca si alguien tocó ese juego
   después.
5. **La confirmación es humana.** Lo destructivo y lo masivo se aprueba en
   Vindexa, no en el agente.
6. **Sin red.** El puente no abre puertos ni contacta con ningún servicio.

## Transporte

El puente es una **API de proceso**, no un servidor. La ruta recomendada, y la
única sin puertos de escucha, es un comando Tauri invocado desde la ventana
principal de Vindexa, que a su vez habla con el agente por entrada y salida
estándar de un proceso acompañante que lanza la propia Vindexa.

`SECURITY.md` describe una frontera en la que la ventana principal solo puede
invocar los comandos registrados en `lib.rs` y toda la red, el sistema de
archivos y SQLite viven en Rust. Un socket TCP en `127.0.0.1` rompería esa
frontera: sería un puerto permanente accesible para **cualquier** proceso local
del usuario y para cualquier página web capaz de hacer una petición al bucle
local. Por eso no existe.

Si en el futuro hiciera falta un transporte fuera del proceso, la única forma
compatible con el modelo de amenazas sería un socket de dominio Unix con
permisos `0600` bajo el directorio de datos de la aplicación, nunca TCP. El
token seguiría siendo obligatorio: el socket es una barrera adicional, no un
sustituto de la autenticación.

## Sobre de la petición

```json
{
  "token": "vdx_<uuid-del-cliente>_<64 caracteres hexadecimales>",
  "utterance": "Acabo de estar 2 horas jugando a DragonsWord Awakening y voy por el 40 %",
  "intent": { "intent": "registrar_sesion", "…": "…" }
}
```

| Campo | Tipo | Obligatorio | Notas |
|---|---|---|---|
| `token` | `string` | sí | Nunca se registra ni se devuelve en ninguna respuesta |
| `utterance` | `string` | no | Frase original; se guarda en la auditoría, recortada a 2 000 caracteres |
| `intent` | objeto | sí | Una de las dieciocho intenciones del catálogo |

La respuesta es una de estas formas:

```json
{
  "outcome": "applied",
  "auditId": "6f1c…",
  "undoToken": "9ab3…",
  "affected": [{ "appId": 367520, "title": "Hollow Knight" }],
  "summary": "Sesión de 120 minutos registrada y progreso actualizado al 40 %."
}
```

```json
{
  "outcome": "needs_game_choice",
  "auditId": "6f1c…",
  "query": "Dragon Age",
  "candidates": [
    { "appId": 47810, "title": "Dragon Age: Origins", "score": 0.91 },
    { "appId": 1222690, "title": "Dragon Age: Inquisition", "score": 0.89 }
  ]
}
```

```json
{
  "outcome": "pending_confirmation",
  "auditId": "6f1c…",
  "reason": "La acción afecta a 7 juegos, por encima del umbral de 5.",
  "affected": [{ "appId": 367520, "title": "Hollow Knight" }],
  "summary": "La acción espera confirmación en Vindexa."
}
```

```json
{ "outcome": "answer", "auditId": "6f1c…", "data": { "games": [] } }
{ "outcome": "rejected", "auditId": "6f1c…" }
{ "outcome": "undone", "auditId": "6f1c…", "restored": 1 }
```

Un error devuelve la forma habitual de Vindexa, `{ "code": "…", "message": "…" }`,
y **también** deja su fila en la auditoría.

## Catálogo de intenciones

Los nombres de intención van en castellano; los nombres de campo siguen el
`camelCase` en inglés del resto del IPC de Vindexa. Todos los campos marcados
como opcionales pueden omitirse.

### Selector de juego

Aparece en casi todas las intenciones. Hay que indicar **exactamente uno** de
los dos campos.

```json
{ "appId": 367520 }
{ "name": "Hollow Knight" }
```

`appId` es la vía inequívoca. `name` activa la resolución tolerante, que puede
devolver `needs_game_choice`.

### Selector de colección o de lista

```json
{ "id": "5e2a…" }
{ "name": "Pendientes" }
```

El nombre se compara normalizado (sin distinguir mayúsculas, tildes ni
puntuación) y debe coincidir con **una sola** colección o lista.

### 1. `registrar_sesion` · ámbito `sesiones:escribir`

```json
{
  "intent": "registrar_sesion",
  "game": { "name": "DragonsWord Awakening" },
  "minutes": 120,
  "startedAt": "2026-08-18T19:00:00Z",
  "progress": 40,
  "note": "Capítulo 3"
}
```

| Campo | Tipo | Obligatorio | Reglas |
|---|---|---|---|
| `game` | selector | sí | |
| `minutes` | entero | sí | 1 – 1 440 |
| `startedAt` | ISO-8601 | no | Si falta, se calcula como «ahora menos `minutes`» |
| `progress` | entero | no | 0 – 100; actualiza el progreso del juego |
| `note` | texto | no | ≤ 2 000 caracteres |

Escribe una fila en `game_sessions`, una en `activity` y, si hace falta,
actualiza `progress` y `started_at` de la ficha personal.

### 2. `marcar_terminado` · ámbito `biblioteca:escribir`

```json
{
  "intent": "marcar_terminado",
  "game": { "name": "Stardew Valley" },
  "completedOn": "2026-08-18",
  "keepPlayable": true,
  "priority": 1
}
```

| Campo | Tipo | Obligatorio | Reglas |
|---|---|---|---|
| `game` | selector | sí | |
| `completedOn` | `AAAA-MM-DD` | no | Por defecto, hoy |
| `keepPlayable` | booleano | no | `true` conserva el estado actual; `false` (por defecto) mueve a «Completado» |
| `priority` | entero | no | 0 – 5; permite resolver «bájale la prioridad» en la misma llamada |

Fija `progress` a 100, escribe `completed_at` y limpia `abandoned_at`.

### 3. `cambiar_estado` · ámbito `biblioteca:escribir`

```json
{ "intent": "cambiar_estado", "game": { "appId": 367520 }, "statusId": "playing" }
```

`statusId` debe existir en `statuses`. Los estados de fábrica son
`unclassified`, `playing`, `next`, `backlog`, `paused`, `completed`,
`abandoned`, `recurring`, `multiplayer`, `waiting_update` y
`waiting_early_access`; la persona usuaria puede haber creado más. Usa
`consultar` con `{"kind":"estados"}` para descubrirlos.

### 4. `ajustar_prioridad` · ámbito `biblioteca:escribir`

```json
{ "intent": "ajustar_prioridad", "game": { "appId": 367520 }, "delta": -2 }
{ "intent": "ajustar_prioridad", "game": { "appId": 367520 }, "priority": 5 }
```

Hay que indicar **exactamente uno**: `priority` (0 – 5, absoluto) o `delta`
(−5 – 5, distinto de cero, relativo y recortado al rango válido).

### 5. `fijar` · ámbito `biblioteca:escribir`

```json
{ "intent": "fijar", "game": { "appId": 367520 }, "pinned": true }
```

### 6. `seguir` · ámbito `biblioteca:escribir`

```json
{ "intent": "seguir", "game": { "appId": 367520 }, "tracking": true }
```

### 7. `valorar` · ámbito `biblioteca:escribir`

```json
{ "intent": "valorar", "game": { "appId": 367520 }, "rating": 9 }
```

`rating` es 1 – 10, o `null` para borrar la valoración.

### 8. `anotar` · ámbito `biblioteca:escribir`

```json
{ "intent": "anotar", "game": { "appId": 367520 }, "note": "Texto", "append": true }
```

`append: true` añade la nota en una línea nueva al final de la existente. El
resultado no puede superar 4 000 caracteres.

### 9. `fijar_proxima_accion` · ámbito `biblioteca:escribir`

```json
{ "intent": "fijar_proxima_accion", "game": { "appId": 367520 }, "action": "Terminar el pantano" }
```

`action` admite hasta 280 caracteres, o `null` para borrarla.

### 10. `fijar_checkpoint` · ámbito `biblioteca:escribir`

```json
{ "intent": "fijar_checkpoint", "game": { "appId": 367520 }, "checkpoint": "Antes del jefe" }
```

### 11. `crear_coleccion` · ámbito `colecciones:escribir`

```json
{
  "intent": "crear_coleccion",
  "name": "Pendientes de 2026",
  "description": "",
  "color": "#5CAAC1",
  "icon": "folder",
  "games": [{ "appId": 367520 }]
}
```

Solo colecciones **manuales**: un agente no crea reglas inteligentes. `name`
tiene 1 – 80 caracteres y debe ser único; `color` es `#RRGGBB`; `games` puede ir
vacío.

### 12. `añadir_a_coleccion` · ámbito `colecciones:escribir`

```json
{
  "intent": "añadir_a_coleccion",
  "collection": { "name": "Pendientes" },
  "games": [{ "appId": 367520 }]
}
```

Los juegos que ya estuvieran dentro se ignoran; si no entra ninguno nuevo, la
petición falla con `validation`.

### 13. `quitar_de_coleccion` · ámbito `colecciones:escribir`

```json
{
  "intent": "quitar_de_coleccion",
  "collection": { "id": "5e2a…" },
  "games": [{ "appId": 367520 }]
}
```

**Siempre exige confirmación humana**, aunque afecte a un solo juego: es la
única acción sustractiva del catálogo.

### 14. `crear_lista_curada` · ámbito `listas:escribir`

```json
{
  "intent": "crear_lista_curada",
  "name": "Joyas",
  "description": "",
  "kind": "showcase",
  "accent": "violet",
  "icon": "list",
  "pinned": true
}
```

`kind` ∈ `manual`, `wishlist`, `backlog`, `showcase`. `accent` ∈ `cyan`, `blue`,
`teal`, `lime`, `amber`, `rose`, `violet`, `slate`.

### 15. `añadir_a_lista` · ámbito `listas:escribir`

```json
{
  "intent": "añadir_a_lista",
  "list": { "name": "Joyas" },
  "games": [{ "appId": 367520 }],
  "note": "Imprescindible",
  "highlight": true
}
```

`note` admite hasta 500 caracteres. El juego debe tener ficha personal en la
biblioteca.

### 16. `planificar` · ámbito `planificador:escribir`

```json
{
  "intent": "planificar",
  "game": { "appId": 367520 },
  "columnId": "next",
  "targetDate": "2026-09-01",
  "estimatedMinutes": 600
}
```

Columnas de fábrica: `playing`, `next`, `month`, `later`, `paused`, `done`. Un
juego solo puede estar en una columna; colocarlo lo saca de la anterior y lo
añade al final de la nueva.

### 17. `programar_aviso` · ámbito `avisos:escribir`

```json
{
  "intent": "programar_aviso",
  "game": { "appId": 367520 },
  "dueAt": "2026-09-01T10:00:00Z",
  "note": "Retomarlo"
}
```

### 18. `consultar` · ámbito `biblioteca:leer`

Solo lectura. No devuelve `undoToken`.

```json
{ "intent": "consultar", "query": { "kind": "biblioteca", "text": "hollow", "statusId": "playing", "limit": 20 } }
{ "intent": "consultar", "query": { "kind": "juego", "game": { "appId": 367520 } } }
{ "intent": "consultar", "query": { "kind": "estados" } }
{ "intent": "consultar", "query": { "kind": "colecciones" } }
{ "intent": "consultar", "query": { "kind": "listas" } }
{ "intent": "consultar", "query": { "kind": "planificador" } }
{ "intent": "consultar", "query": { "kind": "avisos" } }
{ "intent": "consultar", "query": { "kind": "sesiones", "game": { "appId": 367520 }, "limit": 10 } }
{ "intent": "consultar", "query": { "kind": "auditoria", "limit": 20 } }
```

`limit` va de 1 a 200. La búsqueda de `biblioteca` usa el mismo motor de
similitud que la resolución de nombres, así que tolera erratas.

## Resolución de juegos por nombre

Es la pieza más delicada del puente y está pensada para equivocarse hacia el
lado seguro.

**Normalización.** Minúsculas, sin tildes ni diéresis, `ñ` → `n`, signos de
puntuación convertidos en separadores, espacios colapsados, `&` → `and` y
números romanos sueltos convertidos a cifras (`vii` → `7`; la `i` aislada se
respeta porque en inglés casi nunca es un número).

**Puntuación.** Combina tres señales:

| Señal | Peso | Qué captura |
|---|---|---|
| Distancia de edición con transposiciones, normalizada, medida también sobre las cadenas sin espacios | 0,45 | Erratas de tecleo y palabras pegadas o separadas |
| Cobertura de palabras, con emparejado difuso palabra a palabra y penalización suave por las palabras del título que la consulta no menciona | 0,55 | Consultas parciales |
| Refuerzo por prefijo (0,88) o por contención (0,80) | máximo | Consultas que son el principio literal de un título |

La transposición cuenta como **un** error, no como dos: sin eso, «Knigth» por
«Knight» quedaría fuera de umbral, y es la errata más común que existe.

**Decisión.**

| Condición | Resultado |
|---|---|
| Mejor puntuación ≥ 0,86 **y** ventaja sobre la segunda ≥ 0,08 | `applied` con el juego resuelto |
| Alguna candidata ≥ 0,52 | `needs_game_choice` con hasta cinco candidatas ordenadas |
| Ninguna llega a 0,52 | error `not_found` |

Ejemplos comprobados en la batería de pruebas:

| Consulta | Biblioteca | Resultado |
|---|---|---|
| `DragonsWord Awakening` | `Dragon's Word: Awakening` | resuelto |
| `Hollow Knigth` | `Hollow Knight` | resuelto |
| `pokemon rojo` | `Pokémon Rojo` | resuelto |
| `Final Fantasy VII` | `Final Fantasy 7` | resuelto |
| `portal` | `Portal`, `Portal 2` | resuelto a `Portal` |
| `dragon age` | `Dragon Age: Origins`, `Dragon Age: Inquisition` | se pregunta |
| `contabilidad trimestral` | cualquiera | no encontrado |

Cuando la respuesta es `needs_game_choice`, el agente debe enseñar las
candidatas a la persona usuaria y repetir la petición con `{"appId": …}`. No
debe elegir por su cuenta.

## Ámbitos

Los ámbitos son un conjunto cerrado, **sin comodín**. Se comprueban después de
autenticar y **antes** de resolver nombres o escribir nada.

| Ámbito | Intenciones |
|---|---|
| `biblioteca:leer` | `consultar` |
| `biblioteca:escribir` | `marcar_terminado`, `cambiar_estado`, `ajustar_prioridad`, `fijar`, `seguir`, `valorar`, `anotar`, `fijar_proxima_accion`, `fijar_checkpoint` |
| `sesiones:escribir` | `registrar_sesion` |
| `colecciones:escribir` | `crear_coleccion`, `añadir_a_coleccion`, `quitar_de_coleccion` |
| `listas:escribir` | `crear_lista_curada`, `añadir_a_lista` |
| `planificador:escribir` | `planificar` |
| `avisos:escribir` | `programar_aviso` |

Un `scopes_json` ilegible o con un ámbito desconocido deja el cliente **sin
ningún** ámbito. Se prefiere denegar todo a conceder algo por descuido.

## Confirmación humana

Una intención queda en estado `pending` cuando:

- es `quitar_de_coleccion` (siempre), o
- afecta a **más de cinco** juegos.

En ese caso no se escribe nada más que la fila de auditoría, que conserva la
intención ya resuelta. La respuesta es `pending_confirmation` con su `auditId`.

**La confirmación la da la persona usuaria desde Vindexa, no el agente.** Un
agente no puede aprobar sus propias acciones destructivas; si pudiera, la
confirmación no sería una barrera. Al aprobar, el esquema se vuelve a validar
—la fila puede llevar días esperando— y la acción se aplica con los
identificadores ya resueltos, sin volver a interpretar nombres. Al rechazar, la
fila pasa a `rejected` y no se toca nada.

Una fila `pending` solo se puede resolver una vez.

## Deshacer

Toda escritura devuelve un `undoToken` de un solo uso. El recibo asociado guarda
el estado **anterior** y el estado **aplicado**; al deshacer se comprueba
primero que lo que hay en la base sigue siendo exactamente lo aplicado. Si algo
cambió entre medias —la persona editó el juego, otra automatización pasó por
encima— el deshacer se rechaza con `agent_stale` en lugar de sobrescribir
trabajo posterior. Es el mismo enfoque que el recibo de arrastrar y soltar de la
biblioteca (`db::library_dnd`).

| Acción | Qué restaura el deshacer |
|---|---|
| Cambios en la ficha personal | La fila completa anterior |
| `registrar_sesion` | Borra la sesión y su actividad, y devuelve el progreso |
| Cambios de pertenencia a una colección | El orden anterior completo |
| `crear_coleccion` | Borra la colección, si sigue teniendo el mismo contenido |
| `crear_lista_curada` | Borra la lista, si sigue vacía |
| `añadir_a_lista` | Quita los juegos añadidos y renumera |
| `planificar` | Devuelve el juego a su columna y posición anteriores |
| `programar_aviso` | Borra el aviso, si sigue pendiente |

La persona usuaria puede deshacer cualquier acción. Un agente solo puede
deshacer las que él mismo aplicó; intentarlo con la acción de otro devuelve
`agent_scope`.

## Registro de auditoría

Tabla `agent_audit_log` (migración 026). Cada petición deja **exactamente una**
fila.

| Columna | Contenido |
|---|---|
| `id` | UUID; es el `auditId` de las respuestas |
| `client_id` | Cliente; queda a `NULL` si se revoca, para que el historial sobreviva |
| `intent` | Nombre de la intención |
| `utterance` | Frase original, recortada a 2 000 caracteres |
| `arguments_json` | Intención normalizada, con los identificadores ya resueltos |
| `result` | `pending`, `applied`, `rejected`, `failed` o `undone` |
| `affected_json` | Objeto con `games`, `command` y `receipt` (ver abajo) |
| `undo_token` | Token de deshacer; se pone a `NULL` al usarlo |
| `error_message` | Motivo del fallo, en la forma `código: mensaje` |
| `created_at`, `completed_at` | Marcas de tiempo |

Semántica de `result`:

| Valor | Significado |
|---|---|
| `pending` | Esperando confirmación humana |
| `applied` | Aplicada (o respondida, en el caso de `consultar`) |
| `rejected` | Una persona la rechazó |
| `failed` | No se pudo ejecutar: token, ámbito, frecuencia, validación o ambigüedad |
| `undone` | Aplicada y después deshecha |

**Forma de `affected_json`.** La migración 026 no reservó columna para el
recibo, así que esa columna guarda un objeto:

```json
{
  "games": [{ "appId": 367520, "title": "Hollow Knight" }],
  "command": { "intent": "quitar_de_coleccion", "…": "…" },
  "receipt": { "kind": "reminder", "operationId": "…", "reminderId": "…" }
}
```

`command` solo existe mientras la fila está `pending`; `receipt` solo aparece
tras aplicar. El `CHECK` de la tabla no restringe la forma de la columna, así
que el objeto es válido para el esquema vigente. La forma definitiva sería una
columna `receipt_json` propia en una migración posterior.

El token del agente **nunca** aparece en el registro, ni en `arguments_json`, ni
en `utterance`, ni en `error_message`.

El registro conserva las 5 000 entradas más recientes; las más antiguas ya
cerradas se recortan. Las filas `pending` nunca se recortan: siguen esperando
una decisión.

## Emisión y revocación de tokens

Un **cliente agente** es una identidad local con nombre, tipo (`hermes` o
`generic`), token propio y ámbitos. Como máximo puede haber dieciséis.

**Emisión.** Vindexa genera un identificador y un secreto de 32 bytes del CSPRNG
del sistema, y devuelve el token en claro **una sola vez**:

```
vdx_<uuid-del-cliente>_<64 caracteres hexadecimales>
```

El identificador viaja en claro a propósito: permite localizar una sola fila en
lugar de recorrer la tabla comparando resúmenes. El secreto son los 32 bytes
finales.

**Almacenamiento.** `agent_clients.token_hash` guarda una cadena
autodescriptiva, con el mismo espíritu que el formato PHC:

```
pbkdf2-sha256$120000$<sal de 16 bytes en hexadecimal>$<resumen de 32 bytes en hexadecimal>
```

La sal es distinta para cada cliente. El token en claro no se persiste en
ningún sitio: si se pierde, se rota.

**Rotación.** Genera un secreto nuevo con una sal nueva e invalida el anterior
de inmediato.

**Desactivación.** Deja el cliente en la base pero rechaza su token, con el
mismo error que un token inexistente. Es reversible.

**Revocación.** Borra la fila. La auditoría sobrevive porque la clave foránea es
`ON DELETE SET NULL`.

> [!IMPORTANT]
> Un token de agente da acceso de escritura a la biblioteca dentro de sus
> ámbitos. Trátalo como una credencial: no lo pegues en un chat, no lo guardes
> en un repositorio y no lo compartas entre integraciones. Si un agente lo
> conecta a Telegram, quien controle ese bot controla el token.

### Sobre la criptografía de esta fase

`Cargo.toml` no incluye ninguna dependencia de hash ni de derivación de claves.
SHA-256, HMAC-SHA256 y PBKDF2 están implementados dentro de
`src-tauri/src/agent/crypto.rs` y verificados contra los vectores oficiales de
FIPS 180-4, RFC 4231 y RFC 7914 en la batería de pruebas. Los límites, dichos
sin adornos:

- **Es criptografía escrita a mano.** Correcta frente a los vectores, pero sin
  auditoría de terceros ni endurecimiento frente a canales laterales más allá de
  la comparación en tiempo constante.
- **PBKDF2 no es memory-hard.** Argon2id resistiría mejor un ataque por GPU. Lo
  que domina aquí el coste de un ataque no es el KDF sino los 256 bits de
  entropía real del secreto, que no es una contraseña humana.
- La recomendación para una fase posterior es sustituir el módulo por las crates
  `argon2` y `subtle`.

## Límite de frecuencia

Ventana deslizante en memoria del proceso: **30 peticiones por minuto y
cliente**, contadas por identificador. Una petición rechazada no se contabiliza,
así que el agente que respeta el límite recupera su cupo en cuanto la ventana
avanza.

El límite se aplica **antes** de la autenticación, usando el identificador que
viaja dentro del token. Eso protege también al derivador de claves, que es la
parte cara. Un token con formato inválido cuenta contra una clave común
`anonymous`, con el mismo cupo. Se vigilan como máximo 64 identificadores a la
vez.

El rechazo por frecuencia también deja su fila en la auditoría.

## Códigos de error

| Código | Cuándo |
|---|---|
| `agent_token` | Token malformado, cliente inexistente, cliente desactivado o secreto equivocado. **Los cuatro dan el mismo error**, para no revelar cuál es |
| `agent_scope` | Falta el ámbito, o un agente intenta deshacer lo de otro |
| `agent_rate_limit` | Cupo agotado |
| `agent_stale` | El recibo de deshacer ya no cuadra con la base |
| `agent_receipt` | El recibo o la acción pendiente están incompletos |
| `validation` | El esquema de argumentos no se cumple |
| `not_found` | El juego, estado, colección, lista, columna o acción no existe |
| `database` | Error de SQLite, sin detalles internos |

## Frases de ejemplo y su llamada

**«Acabo de estar 2 horas jugando a DragonsWord Awakening y voy por el 40 % de
la historia.»**

```json
{
  "token": "vdx_…",
  "utterance": "Acabo de estar 2 horas jugando a DragonsWord Awakening y voy por el 40 % de la historia",
  "intent": {
    "intent": "registrar_sesion",
    "game": { "name": "DragonsWord Awakening" },
    "minutes": 120,
    "progress": 40
  }
}
```

El nombre mal escrito se resuelve solo. Se crea la sesión con `progressBefore`
igual al progreso anterior y `progressAfter` a 40, se actualiza la ficha
personal y se devuelve un `undoToken`.

**«Stardew Valley ya me lo he pasado pero seguiré jugando: bájale la
prioridad.»**

```json
{
  "intent": "marcar_terminado",
  "game": { "name": "stardew valley" },
  "keepPlayable": true,
  "priority": 1
}
```

Una sola llamada: progreso a 100, fecha de finalización de hoy, el estado se
queda como estaba —el juego sigue siendo jugable— y la prioridad baja. El agente
también puede mandar dos llamadas (`marcar_terminado` y `ajustar_prioridad`) si
le resulta más natural.

**«Mete estos siete en Pendientes.»**

```json
{
  "intent": "añadir_a_coleccion",
  "collection": { "name": "Pendientes" },
  "games": [{ "appId": 1 }, { "appId": 2 }, { "appId": 3 }, { "appId": 4 },
            { "appId": 5 }, { "appId": 6 }, { "appId": 7 }]
}
```

Siete juegos superan el umbral: la respuesta es `pending_confirmation` y no se
escribe nada hasta que una persona lo apruebe en Vindexa.

**«Sube la prioridad de Dragon Age.»**

Con `Dragon Age: Origins` y `Dragon Age: Inquisition` en la biblioteca, la
respuesta es `needs_game_choice` con las dos candidatas. Ningún juego cambia.

**«¿Qué tengo empezado?»**

```json
{ "intent": "consultar", "query": { "kind": "biblioteca", "statusId": "playing", "limit": 20 } }
```

## Qué queda fuera de esta fase

Con honestidad, porque la lista importa tanto como lo construido.

**No está hecho:**

- **La integración de Telegram.** Vindexa no habla con Telegram ni con ningún
  servicio de mensajería, y no está previsto que lo haga: rompería el carácter
  local del proyecto. Ese puente es responsabilidad del agente, que corre fuera.
- **La entrada de audio.** No hay transcripción, ni captura de micrófono, ni
  nada relacionado.
- **La elección entre modelos locales y de API.** Vindexa no invoca ningún
  modelo. Qué modelo interpreta la frase, dónde corre y con qué coste es una
  decisión enteramente del agente. El contrato de esta página es JSON tipado y
  no cambia según el modelo.
- **El proceso acompañante.** Vindexa no lanza ni supervisa ningún proceso de
  agente. Es la pieza que falta para que un agente que corre fuera pueda
  hablar con el puente sin abrir un puerto; el plan está en
  [`HERMES.md`](HERMES.md).
- **La verificación contra Hermes.** No se ha podido comprobar qué es Hermes
  exactamente ni qué convención de llamada a herramientas usa. El contrato se ha
  diseñado deliberadamente genérico —un objeto JSON con `intent` y sus
  argumentos— para que encaje tanto en un esquema de `tools` al estilo de las
  APIs de función como en cualquier envoltorio propio. Puede hacer falta una
  capa de adaptación fina en el lado del agente.

**Hecho desde que se escribió esta lista:**

- **El cableado.** `lib.rs` y `commands.rs` exponen los once comandos del
  puente: despacho, confirmación, deshacer, emisión y rotación de tokens,
  ámbitos, activación, revocación, listados de clientes y de auditoría.
- **La pantalla.** Ajustes → Agentes emite el testigo, reparte permisos por
  ámbito, enseña el registro de lo que cada agente ha hecho y deja deshacerlo.

**Limitaciones conocidas de lo que sí está hecho:**

- El límite de frecuencia vive en memoria: reiniciar Vindexa lo reinicia.
- No hay intención de borrar colecciones, listas, juegos ni estados. Es
  deliberado: un agente puede crear y organizar, pero destruir estructuras
  creadas a mano sigue siendo cosa de la persona usuaria desde la interfaz.
- Las colecciones inteligentes no admiten cambios del agente, porque calculan su
  contenido con reglas.
- La resolución de nombres carga los títulos de la biblioteca en memoria en cada
  petición que use `name`. Con bibliotecas de decenas de miles de juegos
  convendría añadir una caché.
- El deshacer es de un solo paso por acción: no hay una pila de deshacer global.
