# Registro de cambios

Este archivo sigue [Keep a Changelog](https://keepachangelog.com/es-ES/1.1.0/) y el
proyecto usa [versionado semántico](https://semver.org/lang/es/). Una sección describe el
estado del código, no una publicación, mientras figure como **Sin publicar**.

## [Sin publicar]

## [0.1.0] · 2026-08-18

Primera publicación pública, con instaladores para macOS, Windows y Linux. Los
instaladores todavía no van firmados; los límites conocidos del alcance están
enumerados al final del README.

### Añadido

- **Catálogo completo de Steam Family.** El cliente de Steam enseña los juegos propios más los
  del préstamo familiar; Vindexa sólo veía los propios porque pedía el catálogo compartido con
  una Web API Key, y eso únicamente devuelve lo de los miembros que tengan su biblioteca
  pública. Ahora se piden los mismos servicios que usa el cliente, autenticados con el testigo
  de la sesión abierta en el navegador integrado. El testigo vive en el llavero del sistema, no
  se registra ni se muestra, y cuando caduca se olvida y la pantalla pide volver a vincular.
- **Precios de la lista de deseados por lotes.** Se preguntaban de uno en uno con tres cuartos
  de segundo entre peticiones: mil quinientos deseados eran casi veinte minutos. Ahora son cien
  por petición y unos segundos.
- **«Buscar actualizaciones» comprueba de verdad qué versión hay publicada.** Antes devolvía
  siempre «no configurado» porque no había dónde mirar. Sigue sin descargar ni instalar nada por
  su cuenta —eso exigiría firmar los instaladores y llevar la clave pública dentro— pero avisa y
  ofrece abrir la página. «No he podido comprobarlo» y «no hay con qué comparar» son estados
  propios: ninguno se presenta como «estás al día».
- **«Descartar todos» en la bandeja de avisos**, que no es lo mismo que marcar todo como leído:
  lo primero saca el aviso de la vista de pendientes, lo segundo sólo le quita el resalte.
- **Tira de ofertas vigentes en Deseados**, de mayor descuento a menor, con las que ya cumplen
  el precio objetivo marcadas y aviso de precio caducado.
- **Instaladores para macOS, Windows y Linux.** La integración continua compila y prueba el
  backend en los tres sistemas —el puente con el motor web es distinto en cada uno y un fallo
  ahí no se ve hasta compilar allí— y al etiquetar una versión adjunta los tres instaladores a
  la misma release. La release queda en borrador hasta que las tres plataformas han subido lo
  suyo: una release a medias es peor que ninguna.
- **`scripts/version.sh`** sube la versión en los tres ficheros que la declaran y calcula la
  siguiente según el criterio del proyecto (0.1.0 → … → 0.1.10 → 0.2.0), sustituyendo sólo la
  línea de la versión para no reformatear nada.
- **`scripts/vitrina.sh` y `scripts/marco.sh`** regeneran el material visual del repositorio:
  capturas y vídeos de la aplicación real, enmarcados y optimizados, a partir del escenario de
  vitrina de las pruebas de extremo a extremo.
- **Arrastre desde toda la carátula.** Las tarjetas y filas de la biblioteca y las fichas del
  planificador ya no dibujan un asa de seis puntos: cualquier punto de la superficie arrastra
  con el puntero, y el activador que queda es exclusivamente para teclado y lectores de
  pantalla. El acompañante del cursor muestra la portada real del juego y el número de
  títulos seleccionados, y la línea de inserción del destino es visible en las tres vistas.
- **Fundido de borde al desplazar.** Las carátulas se disuelven bajo la barra de herramientas
  conforme suben, en lugar de cortarse contra un borde duro. La intensidad se escribe
  directamente en el nodo desplazable, sin un renderizado de React por fotograma, y se anula
  con `prefers-reduced-motion`.
- **La barra superior se comporta como barra de título nativa**: arrastra la ventana desde su
  espacio vacío y el doble clic maximiza o restaura, sin secuestrar el gesto sobre controles
  ni sobre texto seleccionado.
- **Menú contextual propio con acciones rápidas.** El clic derecho deja de mostrar el menú del
  motor web y ofrece jugar, instalar, abrir ficha y tienda, revelar la instalación, cambiar
  estado, prioridad y colecciones, fijar, seguir y copiar título o AppID. El resto del cromo
  del navegador —arrastre de imágenes, zoom con Ctrl, «volver» por gesto y selección
  accidental— también desaparece, salvo en campos de texto.
- **Detección de juegos sin DRM** a partir de señales oficiales de la tienda, con la evidencia
  que la motiva. Es un dato de ficha: nunca aparece sobre las carátulas.
- **Gestión de DLC** por juego, con propiedad e instalación derivadas del manifiesto local,
  cola de refresco con espera y agregados que separan lo desconocido de lo contado.
- **Listas curadas** con orden manual, nota y destacado por entrada, independientes de las
  colecciones.
- **Lista de deseados** en cuatro cubos de intención, con precio objetivo agregado por moneda
  y vídeos asociados. El identificador se valida en Rust y la reproducción usa la variante sin
  cookies; Vindexa no consulta la API de YouTube.
- **Avisos programables y eventos oficiales** derivados de las señales que Vindexa ya observa,
  con deduplicación estable y bandeja con contadores.
- **Prioridad dinámica explicable**: terminar un juego baja su puntuación aunque se siga
  jugando, y cada decisión se justifica con las señales que la motivaron. Una prioridad manual
  anclada nunca se pisa.
- **Modelo local de gustos** y próximos lanzamientos puntuados contra él. Se calcula en el
  equipo y no sale de él.
- **Vinculación con Epic Games Store y GOG** leyendo únicamente manifiestos locales, sin
  credenciales ni APIs privadas, con emparejado conservador contra la biblioteca de Steam.
- **Puente para agentes externos** con intenciones tipadas, ámbitos, confirmación humana de lo
  destructivo, deshacer y registro auditable de cada acción.
- **Presupuesto configurable de la caché de arte** (128 MiB – 8 GiB) y depuración manual desde
  Ajustes, con desalojo por uso menos reciente.
- **Agrupación de la biblioteca** por inicial, estado, género, año, estudio o antigüedad de la
  última sesión, con encabezado y recuento por grupo. Con mil quinientos juegos, diecisiete
  ordenaciones seguían produciendo una sola lista sin asideros.
- **Prueba de escala de la interfaz** con 1.500 juegos servidos al frontal: comprueba que el
  virtualizador monta treinta fichas y no mil quinientas, que saltar al fondo no acumula
  nodos y que agrupar sigue respondiendo. Medido en el equipo de desarrollo: primera pantalla
  en 641 ms, salto al fondo en 334 ms y agrupación en 106 ms.
- **Selección por rango con Mayús**, además del clic simple y el clic con Cmd o Ctrl. El ancla
  no se mueve al encadenar rangos, como en cualquier gestor de archivos de escritorio.
- **Bandeja de avisos** en la barra superior, con filtros por ámbito, contador de no leídos y
  derivación de eventos oficiales al abrirla.
- **Sección de deseados** con los cuatro cubos de intención, arrastre entre carriles,
  alternativa completa por teclado, precio objetivo agregado **por moneda** y vídeos por
  juego. El importe se presenta como «al menos» cuando hay entradas sin precio, y jamás se
  suman monedas distintas.
- **Panel de contenido adicional** en la ficha, con filtro, marcado manual y actualización
  desde la tienda. «Sin confirmar» significa que no hay evidencia local, no que no lo tengas:
  la interfaz lo dice con esas palabras y explica el motivo exacto de cada hueco.
- **Explicación de la prioridad** al abrir cualquier ficha: la puntuación, la frase que la
  resume, el desglose de señales con su aporte —positivo o negativo, indicado por signo, por
  la dirección de la barra y por texto, nunca solo por color— y la aritmética completa. Un
  interruptor ancla la prioridad manual y enfrenta ambas cifras en lugar de ocultar el
  cálculo.
- Librería interna de microinteracciones adaptada a la estética del proyecto: cifras animadas,
  esqueletos con geometría real, control segmentado, secciones desplegables, pila de avisos y
  realce de zonas de destino, todo con movimiento contenido y anulable.

### Cambiado

- **Seguimiento abre con la respuesta, no con la explicación.** La pantalla pide su
  recomendación al entrar y muestra el juego y sus razones; antes presentaba un cartel que
  describía lo que haría si se pulsaba un botón.
- **Contraste garantizado por prueba.** Los grises de texto se reducen a dos tokens —
  `--v-muted` y `--v-subtle` — y una prueba recorre cada color literal de la hoja global y
  falla por debajo de 4,5:1. Diez incumplimientos medidos quedaron corregidos.
- El azul de acción se reserva a lo que se puede pulsar: las etiquetas de estado y las razones
  de una recomendación pasan a contorno.
- La acción de desinstalar se presenta como destructiva, con el rojo de error y separada del
  grupo de acciones seguras.
- El panel de ficha oscurece de verdad lo que hay detrás: el velo anterior aportaba menos de
  un 2 % de oscurecimiento.
- El ritmo de la retícula pasa a 12 y 16 píxeles en ambos ejes, sobre la unidad base de 4.

- **Arte oficial en su máxima resolución.** Las portadas pasan de 300×450 reales a 600×900
  (4× en píxeles) y los banners de un fondo de tienda recomprimido a `library_hero` (6,3× en
  píxeles visibles). El archivo se guarda tal cual llega, sin recodificar, y la interfaz
  recibe sus dimensiones reales para reservar el hueco exacto y no saltar al decodificar.
- **El banner de la ficha se ve a color.** El velo dejaba de ser transparente a media altura y
  cerraba opaco contra el fondo de la ficha, y el parallax además lo desvanecía al 68 %. Ahora
  el degradado se concentra en el tercio inferior donde vive el texto, cierra contra el color
  exacto de la barra siguiente y el contraste del título se mide, no se supone.
- **Descripciones reestructuradas**: resumen destacado, descripción larga en bloques seguros,
  especificaciones y medios, con ancho de lectura acotado y plegado sin salto de maquetación.
  El backend entrega la descripción ya saneada: la interfaz nunca inyecta HTML.
- **Geometría cuadrada en toda la interfaz.** Los indicadores circulares pasan a cuadrados de
  1–2 px de radio, y las píldoras de etiquetas, interruptores, barras y controles se alinean
  con los radios de `DESIGN.md`.
- La miniatura de la vista de lista y del planificador prefiere la cápsula apaisada antes que
  recortar una portada vertical, que decapitaba el arte.
- La ficha viaja con sus metadatos enriquecidos en una sola respuesta: abrirla ya no encadena
  dos peticiones ni mueve el contenido a mitad de lectura.

### Corregido

- **La tabla de avisos crecía sin tope** porque nada la podaba. El arranque borra ahora los ya
  descartados de hace más de tres meses.
- **La versión que enseñaba «Acerca de» estaba escrita a mano** y decía 0.1.0 pasara lo que
  pasara. Ahora llega desde el paquete.
- **La ficha de un juego ya no repite la petición a la tienda.** La cola de fondo pedía la
  ficha completa y descartaba la mitad: la descripción, las capturas y los vídeos venían en la
  misma respuesta y se tiraban, así que al abrir el juego había que volver a preguntar. Ese era
  el tirón que se notaba al abrir una ficha. Ahora se persiste el paquete entero de una vez.
- **Siete pruebas de integración estaban en rojo** porque enumeraban a mano las migraciones que
  aplicaban, y cada migración nueva que tocaba `games` las rompía por un motivo sin relación
  con el cambio. Ahora aplican el esquema completo leyéndolo del directorio.

- **El desplazamiento vertical de Seguimiento no funcionaba.** La pantalla no tenía altura
  resuelta, así que su zona desplazable nunca desbordaba y el contenido lo recortaba en
  silencio el contenedor padre. Se corrigió con la misma solución que ya usan Planificador y
  Colecciones, y queda blindado con una prueba de regresión.
- Seguimiento se reorganiza en tres zonas de desplazamiento hermanas, sin barras anidadas
  compitiendo, con estados vacíos y de carga propios por bloque y sin salto de maquetación.
- La escritura de la caché de arte es atómica de verdad y valida la integridad al leer: un
  archivo truncado por un corte se vuelve a pedir en lugar de servirse roto. La revalidación
  con `ETag`/`Last-Modified` deja de descargar lo que no ha cambiado.

### Añadido

- Aplicación de escritorio Tauri 2 con biblioteca Steam local-first y SQLite como fuente
  de verdad.
- Vinculación real mediante Steam OpenID, Web API Key en el almacén seguro y sincronización
  manual o periódica mientras la aplicación está abierta.
- Importación de manifiestos locales, procedencia diferenciada (`owned`, `family_shared` y
  `local`) y catálogo separado de Steam Family con disponibilidad prudente, filtros, órdenes
  y vistas virtualizadas propias.
- Biblioteca paginada y virtualizada, búsqueda FTS5, filtros combinables, 17 ordenaciones,
  vistas de cuadrícula/lista/ultracompacta, multiselección y acciones masivas.
- Arrastre de uno o varios juegos a estados y colecciones manuales, alternativa de teclado,
  transacción atómica y deshacer condicionado a que el destino no haya cambiado.
- Ficha inmersiva con arte oficial, parallax respetuoso con reducción de movimiento,
  descripción pública, enriquecimiento progresivo persistente y logros explícitamente
  sincronizables.
- Cola de metadatos con prioridad visible/instalado/resto, dos peticiones máximas, cadencia,
  TTL, deduplicación, `Retry-After`, backoff y recuperación tras cierre.
- Organización personal con estados, progreso, prioridad, valoración, checkpoints, próxima
  acción, notas, fechas, etiquetas y sesiones de juego.
- Colecciones manuales e inteligentes con reglas AND/OR y vista previa.
- Planificador Kanban, cola, semana y mes con capacidad, objetivos, fechas y límites WIP.
- Seguimiento, recordatorios, cambios observados, publicaciones del feed oficial de Steam
  con caché/backoff, relaciones de lanzamiento verificables y recomendaciones locales
  explicables.
- Acciones validadas para jugar, instalar, solicitar desinstalación, revelar instalación y
  abrir una ventana aislada para la tienda oficial de Steam.
- Atajos configurables, densidad de interfaz, diagnóstico SQLite, caché de arte y copias con
  restauración validada y rollback.
- Recursos de marca originales, configuración de bundle macOS y documentación completa de
  producto. La generación/smoke del artefacto sigue siendo un gate separado.

### Corregido

- El empaquetado de producción ya no parte el código de `node_modules` por tamaño: el
  reparto agresivo de rolldown creaba una dependencia circular entre chunks vendor y la
  aplicación instalada no renderizaba (`t is not a function` al montar React).
- Las carátulas ya no fallan en silencio: el arte se resuelve primero desde la caché local
  del cliente de Steam (`appcache/librarycache`, portada vertical real idéntica a la que
  muestra Steam, sin red), y si no existe, el caché propio prueba una cadena de candidatas
  oficiales (portada, 2x, cápsula, cabecera hasheada del store, icono) en lugar de rendirse
  con el primer 404. El enriquecimiento guarda las URLs oficiales con hash de
  `header_image`/`capsule_image`.
- Los fallos transitorios de red al cargar arte reintentan solos al expirar la caché
  negativa, sin tener que desmontar la tarjeta ni reiniciar la vista.
- `cache_game_art` ya no espera al bloqueo de mantenimiento: las carátulas se descargan
  también durante una sincronización de Steam.
- La ficha del juego muestra el tiempo jugado reciente (2 semanas) y la fecha de
  actualización de logros, acota la descripción larga con un toggle accesible y sus campos
  de texto largos muestran contador y error de validación; el autoguardado ya no descarta
  cambios en silencio cuando la validación rechaza un valor.
- El formulario del plan personal y la pestaña de información de la ficha se agrupan en
  secciones con título.

### Añadido (interacción)

- Drag and drop con feedback completo fuera de la biblioteca: overlay opaco al reordenar
  colecciones, anuncios e instrucciones de lector de pantalla en Colecciones y Planificador,
  indicador de inserción diferenciado (barra lateral en cuadrícula, barra superior en lista)
  y Deshacer junto al feedback al mover tarjetas del plan o reordenar colecciones.
- La sesión de biblioteca (alcance, búsqueda, filtros, orden, vista, desplazamiento y
  secciones) persiste entre reinicios de la aplicación.
- Historial de sincronización en Ajustes: la tabla `sync_runs` por fin registra cada
  sincronización de Steam e importación local (resultado, juegos importados/actualizados y
  error si lo hubo) y se muestra en la sección Steam.

### Eliminado

- `TrackingScreen` y su CSS: pantalla huérfana duplicada por `DiscoveryScreen`.
- Comando IPC `set_collection_games`: superficie nativa sin consumidor en la interfaz.

### Seguridad

- CSP restrictiva, capability mínima de la ventana principal y validación nativa de URLs,
  AppID, rutas, imágenes, respuestas HTTP y copias SQLite.
- OpenID con estado de un solo uso, verificación directa, límites de tamaño/tiempo y
  prevención de replay.
- Ventana de tienda en modo privado, sin descargas, popups, autofill, DevTools ni IPC de
  Vindexa; navegación superior limitada a `https://store.steampowered.com`. En macOS y
  Linux las reglas nativas de contenido se instalan de forma fail-closed antes de navegar.
- Migraciones con huella e historial validados y corrección conservadora de procedencia para
  bibliotecas creadas antes de que existiera `ownership_source`.

### Límites conocidos

- No hay endpoint de releases ni clave pública de firma: **Buscar actualizaciones** informa
  el estado, pero no descarga ni instala nada.
- El bundle debug tiene firma ad hoc verificable; no dispone de Developer ID ni notarización.
- La ejecución, el almacén de secretos y los bundles AppImage/RPM siguen sin certificación
  en una máquina Bazzite real.
- Steam Deck queda como **Sin datos** porque no se consume una API pública documentada para
  esa señal.
