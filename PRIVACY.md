# Privacidad y tratamiento de datos

Vindexa es local-first: no opera un servidor propio, no incorpora telemetría y conserva la
organización personal en una base SQLite del equipo. La aplicación sí se conecta a Steam
cuando el usuario inicia OpenID, sincroniza o carga arte oficial.

> [!WARNING]
> Las notas, checkpoints y copias de seguridad SQLite no están cifrados por Vindexa. Trata
> un backup como un archivo privado y no lo publiques ni lo adjuntes a un informe de error.

## Índice

- [Qué se guarda](#qué-se-guarda)
- [Qué sale del equipo](#qué-sale-del-equipo)
- [Qué no hace Vindexa](#qué-no-hace-vindexa)
- [Separación de metadatos y datos personales](#separación-de-metadatos-y-datos-personales)
- [Protección aplicada](#protección-aplicada)
- [Copias de seguridad](#copias-de-seguridad)
- [Control del usuario](#control-del-usuario)
- [Límites por plataforma](#límites-por-plataforma)
- [Informes de privacidad o seguridad](#informes-de-privacidad-o-seguridad)

## Qué se guarda

| Datos | Ubicación | Se incluye en backup SQLite |
| --- | --- | --- |
| SteamID64, nombre, avatar, perfil, visibilidad y última sincronización | `steam_accounts` | Sí |
| Catálogo, tiempo jugado, URLs de arte y metadatos de instalación | `games`, `game_installations` | Sí |
| Catálogo familiar visible y nivel de confirmación local | `family_catalog_games` | Sí |
| Estados, progreso, prioridad, valoración, fechas, checkpoints, próxima acción y notas | `game_personal` | Sí |
| Colecciones, reglas, etiquetas, planner, actividad y sesiones | Tablas SQLite asociadas | Sí |
| Preferencias de densidad, intervalo, confirmación, orden y atajos | `app_settings` | Sí |
| Marcador no secreto de presencia de Web API Key | `app_settings` | Sí |
| Nonces OpenID usados | `openid_nonces` | Sí |
| Web API Key | Almacén seguro del sistema | No |
| Archivos de arte descargados | Directorio de caché Tauri | No |

La ruta activa de SQLite se muestra en
**Ajustes → Datos y copias → Ubicación de datos**. Los directorios concretos dependen del
sistema; Tauri los resuelve a partir del identificador `io.vindexa.desktop`.

## Qué sale del equipo

### Autenticación OpenID

Al pulsar **Continuar con Steam**, Vindexa abre el navegador en
`https://steamcommunity.com/openid/login`. Steam gestiona la contraseña y Steam Guard.
Vindexa recibe una afirmación firmada con SteamID64 mediante un callback temporal en
`127.0.0.1` y la reenvía al mismo proveedor para validarla.

Vindexa no recibe la contraseña, el código Steam Guard ni las cookies del navegador.

### Steam Web API

Al pulsar **Sincronizar ahora**, el backend envía la Web API Key y el SteamID64 mediante
HTTPS a estos endpoints fijos:

- `https://api.steampowered.com/IPlayerService/GetOwnedGames/v0001/`;
- `https://api.steampowered.com/ISteamUser/GetPlayerSummaries/v0002/`.

Si Steam local indica que la cuenta pertenece a un grupo familiar, esa misma acción puede
enviar de forma transitoria a `GetOwnedGames` los SteamID64 de los demás miembros detectados.
Vindexa no persiste esos identificadores, nombres, roles ni tiempos jugados. Sólo conserva
el AppID, título, URLs de arte y una disponibilidad `unknown`/`confirmed` del catálogo
resultante. `confirmed` significa que existe evidencia en la caché o en un manifiesto local;
no acredita una licencia y puede caducar en la siguiente instantánea completa.

Al pulsar **Sincronizar logros** en una ficha, Vindexa envía la clave, el SteamID64 enlazado
y el AppID a `ISteamUserStats/GetPlayerAchievements/v0001/`. No consulta logros al iniciar,
abrir la biblioteca o abrir una ficha. Un resultado privado o no disponible queda como
desconocido, nunca como cero logros.

La respuesta puede contener biblioteca, tiempos de juego y perfil según la visibilidad
configurada en Steam. La política y los términos aplicables son los de Steam; consulta la
[documentación oficial](https://steamcommunity.com/dev).

El inicio y el refresco de Vindexa no leen la Web API Key. Solo guardar, eliminar, comprobar
voluntariamente o sincronizar accede al almacén seguro. La base contiene un booleano de
estado, no el secreto.

### Arte de juegos

Las portadas, cabeceras, iconos y avatares proceden de hosts HTTPS oficiales de Steam. La
interfaz puede solicitarlos directamente al mostrar un juego. El backend también dispone de
una caché que restringe el origen a hosts permitidos y a una ruta que contenga el AppID.
Steam recibe la dirección de red normal de la solicitud y el AppID implícito en la URL.

### Metadatos públicos de la tienda

La actualización bajo demanda de una ficha consulta
`https://store.steampowered.com/api/appdetails` con el AppID y el idioma. Es un endpoint del
dominio oficial de Steam, pero Valve no lo documenta como contrato estable de Steam Web API.
Vindexa limita tamaño, tiempo, tipo de contenido y redirecciones, persiste la fecha de caché
y trata campos ausentes como **Sin datos**. No obtiene compatibilidad Steam Deck mediante
scraping ni la deduce de otros metadatos.

### Acciones externas

Vindexa puede abrir `steam://run/<app-id>`, `steam://install/<app-id>` o
`steam://uninstall/<app-id>`. Estas acciones se entregan al cliente Steam y no incluyen notas
ni organización personal. La desinstalación solo se solicita para una instalación primaria
registrada; Vindexa no borra archivos directamente.

**Tienda integrada** abre una ventana remota distinta de la principal. Usa modo privado, deniega
descargas y popups, no comparte IPC/capabilities y limita la navegación superior a
`https://store.steampowered.com`. El contenido y sus subrecursos siguen perteneciendo a
Steam, que recibe la dirección de red y el AppID. En macOS y Linux se instala antes de
navegar una lista nativa corta que bloquea algunos hosts de analítica y oculta el popup de
cookies; si no puede activarse, la tienda se cierra. La implementación Linux usa WebKitGTK y
todavía no se ha validado en una sesión gráfica Bazzite real. Ninguna plataforma ofrece un
adblock completo.

### Vídeos incrustados

Vindexa **no consulta la API de YouTube** ni ninguna otra API de vídeo. La lista de deseados
guarda únicamente el identificador del vídeo que la persona usuaria pega, validado en Rust
contra el formato exacto de once caracteres.

No se hace ninguna petición a Google hasta que la persona **pulsa reproducir**. En ese
momento se carga un `iframe` de `https://www.youtube-nocookie.com`, la variante sin cookies
de seguimiento, y es el único origen que la política de contenido de la aplicación permite
incrustar (`frame-src`). Las miniaturas alojadas en `i.ytimg.com` se rechazan a propósito:
pintarlas supondría una petición a Google antes de que nadie haya decidido ver nada.

### Tiendas externas

La detección de Epic Games Store y GOG lee **solo ficheros locales** que esos clientes ya
escriben en el disco: sus manifiestos de instalación y la caché de biblioteca que Heroic y
Legendary guardan tras iniciar sesión. Vindexa no pide credenciales de esas tiendas, no
guarda tokens y no llama a sus APIs privadas. Los ficheros de sesión de esos clientes
(`gog_store/auth.json` y `legendaryConfig/legendary/user.json`) **no se abren nunca**: sólo
se comprueba si existen, para poder distinguir «no has iniciado sesión» de «no tienes el
cliente». Si el cliente no está instalado, el resultado es un estado explícito de «no
disponible», no un intento de conexión.

Vindexa no implementa un inicio de sesión propio contra Epic ni contra GOG. Hacerlo
obligaría a suplantar las credenciales OAuth de sus clientes oficiales, algo que las
condiciones de uso de Epic no autorizan y que dejaría a Vindexa custodiando un testigo de
acceso a la cuenta.

### Modelo de gustos y prioridad

La puntuación de prioridad y el modelo de afinidad por género, categoría, estudio o etiqueta
se calculan **íntegramente en el equipo**, a partir del comportamiento local ya guardado. No
se envían a ningún servidor, no alimentan ningún servicio remoto y se pueden borrar con la
base de datos.

## Qué no hace Vindexa

- No solicita ni almacena la contraseña de Steam.
- No guarda la Web API Key en SQLite, `localStorage`, logs propios o backups.
- No envía notas, checkpoints, estados, valoración, planner o colecciones a Steam.
- No ejecuta analítica, anuncios, crash reporting remoto ni telemetría.
- No ofrece una cuenta de Vindexa ni sincronización propia en la nube.
- No vende ni comparte datos con terceros desde el código actual.
- No consulta la API de YouTube: el único tráfico hacia Google ocurre al pulsar reproducir un
  vídeo, y siempre contra `youtube-nocookie.com`.
- No pide credenciales de Epic ni de GOG, ni implementa un inicio de sesión propio contra
  ellas: solo lee ficheros que esos clientes ya han escrito en el disco.
- No abre los ficheros de sesión de Heroic ni de Legendary: solo comprueba si existen.
- No envía fuera del equipo el modelo de gustos ni la puntuación de prioridad.
- No modifica manifiestos ni archivos internos de Steam.
- No instala ni elimina un juego por sí sola: solicita la acción al cliente oficial.

## Separación de metadatos y datos personales

Los metadatos importados viven en `games`; la organización vive en `game_personal` y tablas
relacionadas. El upsert de Steam actualiza título, arte y tiempo, y solo crea una fila
personal cuando aparece un AppID nuevo. Por tanto, una resincronización no sustituye las
decisiones privadas existentes.

La importación local reconstruye la marca de instalación y sus rutas, pero conserva el
resto de `game_personal`. Desvincular Steam elimina `steam_accounts` y el catálogo familiar
derivado; no elimina la biblioteca ni la organización personal ya creada.

## Protección aplicada

- La Web API Key se valida como 32 caracteres hexadecimales y se almacena con `keyring`.
- El frontend solo conoce el marcador y si necesita verificación; no puede pedir el valor
  de la clave.
- OpenID usa estado aleatorio, retorno exacto, campos firmados, verificación con Steam,
  tiempo máximo y prevención de replay mediante nonces.
- Las solicitudes Rust no siguen redirecciones.
- Los AppID, URLs externas, rutas locales y variantes de arte se validan antes de abrir o
  descargar.
- La caché limita una imagen a 10 MiB, valida MIME y firma de archivo y escribe de forma
  temporal antes de renombrar.
- La CSP de Tauri restringe scripts, red y fuentes de imagen.
- El protocolo de assets solo permite `$APPCACHE/steam-art/**`.
- La única capability de la ventana permite abrir la URL exacta de registro de Web API Key;
  no concede acceso genérico a diálogos, archivos, shell o red.
- SQLite usa parámetros y claves foráneas.
- La ventana remota de tienda no recibe capabilities Tauri, desactiva autofill/DevTools y
  deniega navegación superior fuera del host oficial. El bloqueo nativo adicional se
  instala de forma fail-closed en macOS y Linux.

Estas medidas reducen riesgos conocidos; no equivalen a cifrado del contenido personal ni
a una garantía absoluta frente a un equipo ya comprometido.

## Copias de seguridad

**Ajustes → Datos y copias → Exportar copia** pide a Rust que abra el diálogo nativo y crea
una copia SQLite coherente mediante la API Online Backup. La ruta elegida no entra en React
ni cruza IPC. El destino debe superar integridad, historial de migraciones, esquema exacto y
claves foráneas.

Una restauración:

1. abre el origen en modo de solo lectura;
2. comprueba integridad, identidad de aplicación, historial, esquema completo y claves
   foráneas;
3. crea en el directorio de datos un backup previo con nombre
   `vindexa-before-restore-AAAAmmdd-HHMMSS-UUID.sqlite3`;
4. copia el origen sobre la base activa dentro de la operación SQLite;
5. reconfigura y valida el resultado; si algo falla, restaura y verifica automáticamente la
   base anterior.

Las operaciones de mantenimiento toman un lock exclusivo de proceso para que ninguna
lectura, importación o sincronización use la base mientras se restaura. Rust rechaza la base
activa, sus archivos WAL/SHM/journal, enlaces simbólicos, hard links equivalentes y archivos
que no sean copias compatibles.

El backup no contiene la Web API Key ni archivos de caché. Sí contiene SteamID64, rutas de
instalación, perfil, notas y toda la organización personal. Si necesitas transportar una
copia, usa un medio cifrado proporcionado por el sistema operativo.

## Control del usuario

| Objetivo | Acción actual |
| --- | --- |
| Eliminar la Web API Key | **Ajustes → Steam → Eliminar clave** |
| Desvincular Steam | **Ajustes → Steam → Desvincular** |
| Exportar datos | **Ajustes → Datos y copias → Exportar copia** |
| Restaurar datos | **Ajustes → Datos y copias → Restaurar copia** |
| Consultar ubicación e integridad | **Ajustes → Datos y copias → Diagnóstico local** |
| Vaciar arte descargado | **Ajustes → Datos y copias → Vaciar caché de imágenes** |
| Consultar la versión | **Ajustes → Acerca de → Buscar actualizaciones**; no transmite ni descarga mientras no exista endpoint configurado |

No existe todavía una acción **Borrar todos mis datos**. Para una eliminación completa:

1. exporta una copia si quieres conservar algo;
2. elimina la clave desde Vindexa;
3. cierra Vindexa por completo;
4. abre la ruta mostrada en Diagnóstico y mueve la base, sus archivos `-wal`/`-shm`, los
   backups automáticos y la caché de Vindexa a la Papelera usando el gestor de archivos del
   sistema.

No borres un directorio de aplicación amplio mediante un comando recursivo. Verifica que el
identificador y la ruta correspondan exactamente a `io.vindexa.desktop`.

## Límites por plataforma

El almacén seguro depende de los servicios del sistema:

- en macOS, Keychain;
- en Linux, un servicio de secretos compatible y desbloqueado en la sesión de escritorio;
- en Windows, el proveedor de credenciales seleccionado por `keyring`.

El acceso al almacén seguro en Bazzite todavía no se ha validado desde este entorno macOS.
Vindexa falla de forma explícita con el código `secure_storage`; no degrada a texto plano.

## Informes de privacidad o seguridad

Antes de compartir un diagnóstico:

- no adjuntes `vindexa.sqlite3` ni un backup;
- no pegues la Web API Key;
- elimina SteamID64, rutas de usuario, nombre de perfil y títulos privados de capturas;
- describe el código de error visible y el paso que lo produjo.

Vindexa no está afiliada a Valve Corporation. El uso de OpenID y Web API está sujeto a los
términos de Steam.
