# Configurar Steam en Vindexa

Vindexa ofrece dos importadores independientes: manifiestos locales para saber qué está
instalado y Steam Web API para identificar la cuenta, importar juegos visibles y recuperar
tiempo jugado.

> [!WARNING]
> Introduce la contraseña y los códigos de Steam Guard únicamente en el navegador oficial
> de Steam. Vindexa no necesita, solicita ni almacena esas credenciales.

## Índice

- [Elegir el tipo de importación](#elegir-el-tipo-de-importación)
- [Importar una instalación local](#importar-una-instalación-local)
- [Vincular el SteamID64](#vincular-el-steamid64)
- [Añadir la Web API Key](#añadir-la-web-api-key)
- [Sincronizar la biblioteca](#sincronizar-la-biblioteca)
- [Acciones de Steam](#acciones-de-steam)
- [Solucionar problemas](#solucionar-problemas)

## Elegir el tipo de importación

| Operación | Cuenta vinculada | Web API Key | Instalación local |
| --- | --- | --- | --- |
| Detectar juegos instalados, ruta, tamaño y build | No | No | Sí |
| Obtener SteamID64 | Sí, mediante OpenID | No | No |
| Obtener nombre, avatar y URL del perfil | Sí | Sí | No |
| Obtener juegos visibles y tiempo jugado | Sí | Sí | No |
| Consultar logros del perfil vinculado | Sí | Sí | No |
| Detectar el catálogo de Steam Family | Sí | Sí | Sí, para detectar el grupo |
| Abrir tienda, pedir instalación o desinstalación | No | No | Cliente Steam recomendado |
| Ejecutar un juego | No | No | Cliente Steam y juego instalado |

Los dos importadores se pueden ejecutar en cualquier orden. La coincidencia se realiza por
AppID y una actualización de Steam no sustituye los estados, progreso, prioridad,
valoración, seguimiento, fechas, checkpoints, notas, colecciones ni posición del usuario.

## Importar una instalación local

1. Abre **Ajustes** con el botón de engranaje o `⌘,` en macOS.
2. En **Steam**, localiza **Bibliotecas instaladas**.
3. Pulsa **Explorar bibliotecas locales**.

El backend usa `steamlocate` para descubrir las instalaciones de Steam y leer sus
manifiestos. Para cada AppID válido conserva, cuando el manifiesto lo expone:

- nombre del juego;
- biblioteca e instalación canonicalizable;
- tamaño en disco;
- build ID;
- última actualización del manifiesto.

El escaneo reconstruye qué títulos están instalados. Un juego que ya no aparezca en ningún
manifiesto legible se conserva en la biblioteca de Vindexa, pero deja de marcarse como
instalado. No se modifica ningún manifiesto ni archivo de Steam.

La importación local no puede conocer tiempo jugado, perfil, avatar ni visibilidad. Para
esos datos continúa con OpenID y la Web API.

## Vincular el SteamID64

1. Abre **Ajustes → Steam**.
2. Pulsa **Continuar con Steam**.
3. Completa el acceso en `steamcommunity.com` dentro del navegador externo.
4. Cuando el navegador confirme **Steam conectado**, vuelve a Vindexa.

El flujo implementado es Steam OpenID 2.0:

1. Vindexa abre un listener temporal en `127.0.0.1` y un puerto aleatorio.
2. Genera un estado de un solo uso y abre el endpoint oficial en el navegador.
3. Espera el callback durante un máximo de 180 segundos.
4. Comprueba método, ruta, estado, namespace, endpoint, `return_to`, identidad y campos
   firmados.
5. Envía la afirmación a Steam con `openid.mode=check_authentication` y exige
   `is_valid:true`.
6. Valida la forma `https://steamcommunity.com/openid/id/<steamid>`, la fecha del nonce y
   que ese nonce no se haya usado.
7. Guarda solo el SteamID64 en SQLite.

La documentación oficial de Steam describe el proveedor y el formato del SteamID64 en
[Steam Web API Documentation](https://steamcommunity.com/dev).

Si el acceso tarda más de tres minutos, Vindexa cierra el listener. Repite el flujo; no hace
falta revocar nada en Steam.

## Añadir la Web API Key

OpenID identifica la cuenta, pero `GetOwnedGames` necesita una Web API Key personal.

1. Inicia sesión en Steam desde el navegador.
2. Abre el [registro oficial de Steam Web API Key](https://steamcommunity.com/dev/apikey).
3. Lee y acepta los términos de Steam antes de crear la clave.
4. Copia la clave hexadecimal de 32 caracteres.
5. En Vindexa, abre **Ajustes → Steam → Web API Key**.
6. Introduce la clave. Sin cuenta vinculada pulsa **Guardar de forma segura**; con cuenta
   vinculada pulsa **Guardar y sincronizar**.

Vindexa valida que la clave contenga exactamente 32 caracteres hexadecimales y la guarda en
el almacén seguro del sistema con:

| Campo | Valor |
| --- | --- |
| Servicio | `io.vindexa.desktop` |
| Cuenta | `steam-web-api-key` |
| Valor | La clave personal de 32 caracteres |

La clave no se escribe en SQLite, preferencias web, archivos de configuración ni backups.
La interfaz nunca recibe el valor guardado. SQLite conserva únicamente un marcador no
secreto para indicar si la última operación confirmó su existencia.

Vindexa no abre Keychain durante `bootstrap`, al iniciar la aplicación ni al refrescar sus
datos. Si una instalación existente todavía no tiene ese marcador, aparece **Comprobar
clave guardada**: es una acción voluntaria que consulta una vez el almacén seguro y actualiza
solo el marcador. Guardar, eliminar, comprobar y sincronizar son las únicas acciones que
acceden a la credencial.

Pulsa **Eliminar clave** para borrarla del almacén seguro. El SteamID64 y los datos ya
importados permanecen en SQLite.

## Sincronizar la biblioteca

Con la cuenta vinculada y la clave configurada, pulsa **Sincronizar ahora**. Guardar una
clave con una cuenta ya vinculada ejecuta el mismo flujo una sola vez inmediatamente; si
falla, la clave permanece guardada y Vindexa muestra la causa comunicada por el backend.

Vindexa realiza dos solicitudes HTTPS sin seguir redirecciones:

- `IPlayerService/GetOwnedGames/v0001/` con `include_appinfo=1` e
  `include_played_free_games=1`;
- `ISteamUser/GetPlayerSummaries/v0002/` para el perfil vinculado.

Cada respuesta debe ser `application/json`, no puede superar 32 MiB y debe llegar en menos
de 30 segundos. El timeout de conexión es de 10 segundos.

El comportamiento de `GetOwnedGames` y sus restricciones de visibilidad están descritos en
la [referencia oficial de IPlayerService](https://partner.steamgames.com/doc/webapi/IPlayerService).

Si el cliente local contiene un bloque `FamilyGroup`, la sincronización manual consulta
transitoriamente `GetOwnedGames` para los demás miembros detectados. Vindexa no conserva
sus SteamID64, nombres ni roles. Los títulos visibles se guardan en un catálogo familiar
separado: **disponibilidad por confirmar** por defecto y **confirmado localmente** sólo
cuando el cliente mantiene caché o manifiesto para ese AppID. Esta señal no es una licencia
ni una garantía de lanzamiento: Steam decide la elegibilidad final y algunos juegos quedan
excluidos de Steam Families.

El catálogo aparece separado en la barra lateral. Solo un título `confirmed` puede entrar
en la biblioteca personal con procedencia `family_shared`; los títulos `unknown` no reciben
tiempo jugado de terceros ni se presentan como propiedad. Juegos locales anteriores a la
procedencia se mantienen como `local` salvo que una respuesta `GetOwnedGames` los confirme
como propios.

### Datos que se actualizan

- AppID y título;
- hash/URL del icono cuando Steam entrega un hash válido;
- URL oficial calculada de portada y cabecera;
- minutos totales y de las últimas dos semanas;
- última ejecución cuando Steam entrega un timestamp;
- nombre visible, avatar, URL de perfil y estado de visibilidad.

Vindexa nunca reduce el tiempo total ya guardado si una respuesta posterior entrega un
valor inferior. El tiempo reciente sí refleja la respuesta más nueva.

Una sincronización fallida no borra juegos ni organización: conserva en `steam_accounts`
`last_sync_status = failed`, un código y un mensaje seguro que Ajustes muestra incluso tras
reiniciar. El siguiente éxito actualiza la fecha, marca `success` y limpia el error.

### Metadatos y logros

Al abrir una ficha, Vindexa puede actualizar bajo demanda descripción, estudio, editor,
géneros, categorías, fecha de lanzamiento, gratuidad y Early Access desde la ficha pública
de Steam. Esta consulta usa `store.steampowered.com/api/appdetails`, un endpoint del dominio
oficial que Valve no documenta como contrato estable de Steam Web API; por eso tiene caché,
estados `pendiente/error/sin datos` y nunca bloquea la apertura de la biblioteca.

**Sincronizar logros** es una acción explícita que usa el método oficial
`ISteamUserStats/GetPlayerAchievements/v0001`. Si la privacidad del perfil, el juego o Steam
no permiten obtenerlos, Vindexa muestra **Sin datos** en vez de convertir lo desconocido en
cero. Esta acción sí consulta la Web API Key; abrir una ficha no lo hace.

Steam Deck permanece **Sin datos**: Valve publica la compatibilidad en el cliente y la
tienda, pero no documenta un método público de Steam Web API que Vindexa pueda consumir con
este contrato. Vindexa no infiere la insignia ni raspa una página HTML.

### Biblioteca privada o vacía

Steam solo devuelve juegos si la biblioteca es visible para la clave que realiza la
consulta. Cuando la respuesta no contiene el array `games`, Vindexa marca el resultado como
posible biblioteca privada y no elimina los juegos locales ni la organización personal.

Revisa en Steam la visibilidad de **Detalles de juegos** y vuelve a sincronizar. Una
biblioteca realmente vacía también puede producir cero resultados; por eso el mensaje es
deliberadamente prudente.

### Sincronización manual y periódica

**Sincronizar ahora** ejecuta el flujo inmediatamente. En **Ajustes → Apariencia y
comportamiento** también puedes elegir 30 minutos, una hora, seis horas o un día. Con una
cuenta vinculada, Vindexa ejecuta el mismo comando al vencer el intervalo, evita dos
sincronizaciones simultáneas y refresca la interfaz al terminar.

El temporizador pertenece a la ventana: solo avanza mientras Vindexa está abierta. No es
una tarea del sistema, no despierta la aplicación cerrada y necesita que la Web API Key siga
disponible. No se inicia si todavía está pendiente **Comprobar clave guardada**.

## Acciones de Steam

La ficha y el menú contextual pueden solicitar acciones al cliente de Steam:

| Acción | Destino construido por Rust |
| --- | --- |
| Jugar | `steam://run/<app-id>` |
| Instalar | `steam://install/<app-id>` |
| Desinstalar | `steam://uninstall/<app-id>` después de validar una instalación primaria |
| Tienda integrada | Ventana privada con `https://store.steampowered.com/app/<app-id>` |
| Mostrar instalación | Carpeta canonicalizada bajo una biblioteca Steam detectada |

El AppID debe ser distinto de cero. No se aceptan esquemas o URLs proporcionados por el
usuario. Revelar una carpeta falla si desapareció o si ya no pertenece a una biblioteca
Steam detectada.

Vindexa nunca elimina directamente carpetas, manifiestos o archivos. **Desinstalar** solo
aparece para un juego instalado; según la preferencia pide confirmación y entrega la URL al
cliente Steam. El feedback indica que la solicitud se abrió, no que Steam haya terminado.

**Tienda integrada** reutiliza una ventana aislada sin IPC de Vindexa, en modo privado, sin
autofill, descargas, popups ni DevTools. La navegación superior se limita al host exacto
`store.steampowered.com`. En macOS y Linux Vindexa instala primero reglas nativas acotadas
para bloquear algunos hosts de analítica y ocultar el popup de cookies; si no puede, cierra
la ventana. macOS usa `WKContentRuleList` y Linux la API nativa de WebKitGTK. No es un
navegador general ni un adblock completo; para comunidad, cuenta, soporte o login usa el
navegador oficial. La ruta Linux todavía requiere validación runtime en Bazzite real.

## Solucionar problemas

### «No se encontró una instalación local de Steam»

- Confirma que Steam está instalado para el mismo usuario que ejecuta Vindexa.
- Abre Steam al menos una vez para que existan sus archivos de biblioteca.
- Si las bibliotecas están en volúmenes externos, móntalos antes del escaneo.
- El importador solo procesa bibliotecas y manifiestos que `steamlocate` puede leer.

### El navegador no vuelve a Vindexa

- Termina el acceso antes del límite de 180 segundos.
- No cierres Vindexa mientras la pestaña está abierta.
- Un firewall local debe permitir la conexión del navegador a `127.0.0.1`.
- Si aparece un callback distinto de `/steam/openid`, Vindexa lo rechaza por diseño.

### «Steam rechazó la clave Web API»

- Sustituye la clave desde el registro oficial; no pegues espacios.
- Comprueba que sigue activa para la cuenta vinculada.
- Si Steam responde con limitación de solicitudes, espera unos minutos antes de reintentar.

### La sincronización no muestra juegos

- Comprueba la visibilidad de los detalles de juegos en Steam.
- Confirma que OpenID vinculó la misma cuenta que registró la clave.
- Usa el importador local para conservar al menos el inventario instalado mientras revisas
  la privacidad remota.

### El almacén seguro no está disponible

El crate `keyring` utiliza el proveedor de credenciales del sistema. En macOS debe estar
disponible Keychain. En Linux debe existir y estar desbloqueado un servicio de secretos
compatible con el entorno de escritorio. El backend devuelve `secure_storage` si no puede
acceder a él; no cae a un archivo plano.

### Keychain solicita la contraseña repetidamente en macOS

La apertura normal de Vindexa no consulta Keychain. Comprueba primero qué acción precedió
al aviso:

- **Comprobar clave guardada**, **Guardar**, **Eliminar** y **Sincronizar** sí necesitan
  acceder al elemento seguro y macOS puede pedir autorización;
- un reinicio o un refresh sin esas acciones no debería mostrar el aviso;
- en `tauri dev`, recompilar cambia el binario sin firmar que accede al elemento y macOS
  puede no reutilizar la autorización de una build anterior.

Tras una comprobación o guardado correcto, cierra y abre la misma build sin recompilar. Si
el aviso aparece antes de cualquier acción de Steam, registra el momento exacto y el código
visible: no introduzcas la clave en logs ni borres el llavero completo.

### Desvincular la cuenta

**Ajustes → Steam → Desvincular** elimina la identidad de `steam_accounts` y detiene futuras
sincronizaciones asociadas. La biblioteca, estados, progreso, planner, colecciones,
checkpoints y notas permanecen. La Web API Key es una credencial separada: usa **Eliminar
clave** si también quieres borrarla.
