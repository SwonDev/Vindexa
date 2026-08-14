# Manual de usuario de Vindexa

Vindexa organiza una biblioteca real de Steam en el equipo. Steam aporta catálogo,
instalación y metadatos disponibles; estados, notas, etiquetas, planificación y sesiones
pertenecen al usuario y se guardan localmente.

> [!IMPORTANT]
> Vindexa no incluye datos demo. Si la biblioteca está vacía, importa manifiestos locales o
> vincula Steam. Nunca introduzcas la contraseña o Steam Guard dentro de Vindexa.

## Índice

- [Primera apertura](#primera-apertura)
- [Biblioteca](#biblioteca)
- [Steam Family](#steam-family)
- [Ficha de un juego](#ficha-de-un-juego)
- [Arrastrar, mover y deshacer](#arrastrar-mover-y-deshacer)
- [Colecciones](#colecciones)
- [Planificador](#planificador)
- [Seguimiento y descubrimiento](#seguimiento-y-descubrimiento)
- [Ajustes](#ajustes)
- [Atajos y teclado](#atajos-y-teclado)
- [Copias, actualización y desinstalación](#copias-actualización-y-desinstalación)
- [Solución de problemas](#solución-de-problemas)

## Primera apertura

La aplicación crea SQLite, estados y columnas iniciales. No consulta Keychain ni inicia una
sincronización oculta.

### Importar juegos instalados

1. Abre **Ajustes → Steam**.
2. Pulsa **Explorar bibliotecas locales**.
3. Espera al recuento de bibliotecas y manifiestos.

Esta ruta no necesita cuenta ni Web API Key. Permite conocer título, AppID, ruta, tamaño,
build e instalación cuando Steam los expone. No puede recuperar perfil ni tiempo jugado.

### Vincular y sincronizar Steam

1. Pulsa **Continuar con Steam**.
2. Completa el acceso en el navegador oficial.
3. Si no tienes Web API Key, pulsa **Obtener una Web API Key** para abrir la página oficial.
4. Pega la clave en Vindexa y usa **Guardar y sincronizar**.

La clave se guarda en el almacén seguro; la interfaz no puede volver a leer su valor. Para
detalles y privacidad de la biblioteca, consulta [STEAM_SETUP.md](./STEAM_SETUP.md).

## Biblioteca

La biblioteca combina barra lateral, toolbar y contenido virtualizado. El número junto al
título corresponde al filtro actual, no necesariamente al total global.

### Buscar

El buscador encuentra títulos, notas, checkpoints y próximas acciones. La consulta usa
debounce y FTS5. Borra el campo con el botón **Limpiar búsqueda**.

### Cambiar vista

- **Cuadrícula:** prioriza portadas 2:3 y metadatos esenciales.
- **Lista:** muestra más títulos por pantalla con icono y columnas compactas.

Las imágenes reservan su geometría, cargan de forma diferida y usan caché local validada.
Un fallback con iniciales no significa que el juego sea ficticio: indica que Steam no ha
entregado arte utilizable o todavía está cargando.

### Ordenar

El selector incluye 17 órdenes:

| Grupo | Órdenes |
| --- | --- |
| Personal | Prioridad manual, aleatorio. |
| Actividad | Jugados recientemente, añadidos recientes. |
| Título | A–Z, Z–A. |
| Lanzamiento | Más reciente, más antiguo. |
| Tiempo | Más horas, menos horas. |
| Instalación | Instalados primero, no instalados primero, mayor tamaño, menor tamaño. |
| Organización | Mayor progreso, mejor valoración, objetivo más próximo. |

La elección se guarda. **Aleatorio** mantiene una semilla estable durante la vista actual;
volver a elegirlo genera otra mezcla.

### Filtrar

**Filtros** abre grupos combinables que se aplican en SQLite antes de paginar:

- estado, instalación, nunca jugado, Early Access, gratuito y procedencia;
- horas, progreso, valoración y porcentaje de logros;
- género, categoría, etiqueta y colección;
- lanzamiento, última vez jugado y fecha objetivo;
- seguimiento, Steam Deck y duración estimada de sesión.

Los chips superiores muestran filtros activos y permiten quitarlos individualmente. Un
filtro basado en metadatos solo puede encontrar juegos cuyo dato exista. Steam Deck aparece
deshabilitado o sin resultados cuando no hay una fuente fiable.

### Seleccionar y actuar en lote

Selecciona un juego o añade varios con la interacción modificadora de la plataforma. La
barra de selección permite:

- mover todos a un estado;
- añadir todos a una colección manual;
- limpiar la selección.

El backend cambia todo el lote o nada. Una selección no convierte juegos de catálogo
familiar no confirmado en biblioteca personal.

## Steam Family

La barra lateral muestra **Steam Family** como catálogo separado cuando existen resultados.
Una sincronización explícita puede consultar transitoriamente los demás miembros detectados
por el cliente local.

Cada título tiene una señal:

- **Disponibilidad por confirmar:** Steam lo expuso al grupo, pero no hay evidencia local.
- **Confirmado localmente:** existe caché o manifiesto local para ese AppID.

La señal no es una licencia. Steam decide si el juego puede compartirse o lanzarse. Vindexa
no conserva identidad, nombre, rol ni tiempo jugado de otros miembros. Un título familiar
confirmado puede incorporarse como `family_shared`; el resto permanece solo en el catálogo
y ofrece acceso a la tienda.

## Ficha de un juego

Abre una portada o fila para mostrar una ficha amplia. El hero se desplaza con un parallax
contenido al hacer scroll; con **Reducir movimiento** la transformación se desactiva.

### Cabecera y acciones

La cabecera muestra arte, título, horas, última sesión, logros, instalación y procedencia.
Según el estado local ofrece:

- **Jugar:** entrega `steam://run/<app-id>` al cliente Steam.
- **Instalar:** entrega `steam://install/<app-id>`.
- **Desinstalar:** solicita `steam://uninstall/<app-id>` después de la confirmación
  configurada; Steam revisa y completa la operación.
- **Mostrar instalación:** abre una ruta canonicalizada dentro de una biblioteca detectada.
- **Tienda integrada:** abre la tienda oficial en una ventana aislada de Vindexa. En macOS y
  Linux la página solo se carga después de instalar reglas nativas de contenido; la ruta
  Linux todavía no está validada en una sesión gráfica Bazzite real.

Vindexa comunica que la solicitud se entregó, no que Steam terminó una instalación o
desinstalación.

### Acerca de e información

La descripción, estudio, editor, géneros, categorías, lanzamiento, gratuidad y Early Access
se enriquecen progresivamente desde la ficha pública. Vindexa prioriza primero los juegos
visibles, después los instalados y finalmente el resto; la cola persiste entre aperturas y
respeta caché, límites y reintentos de Steam. **Reintentar metadatos** fuerza una carga
fallida o no disponible. Los logros se consultan por separado con **Sincronizar logros**
porque necesitan la Web API Key y privacidad suficiente.

Los estados **Sin datos**, **No disponible** y **Error** son intencionados: Vindexa no
rellena información ausente con valores inventados.

### Plan personal

Edita estado, valoración, progreso, prioridad, fijado, seguimiento, duración estimada,
fecha objetivo, checkpoint, próxima acción, notas y colecciones. El indicador de guardado
señala progreso, éxito o error; cierra la ficha solo después de confirmar que el cambio se
guardó.

### Registro

La pestaña **Registro** agrupa:

- **Fechas personales:** inicio, finalización o abandono. Finalización y abandono son
  excluyentes y no pueden preceder al inicio.
- **Etiquetas:** crea, renombra, recolorea, asigna o elimina etiquetas. El nombre admite
  1–40 caracteres, el color usa `#RRGGBB` y cada juego admite hasta 64.
- **Sesiones:** inicio, final opcional, progreso antes/después y nota. El final no puede
  preceder al inicio y la nota admite hasta 2.000 caracteres. Se cargan 50 por página; usa
  **Cargar más** para consultar el resto.

Editar o eliminar una sesión requiere acción explícita. Una sincronización de Steam no
sobrescribe fechas, etiquetas ni sesiones.

### Actividad

La pestaña **Actividad** muestra la línea temporal local de cambios. Las entradas sirven
como trazabilidad y no se envían a Steam.

## Arrastrar, mover y deshacer

Usa el asa **Arrastrar _juego_** de una portada o fila y muévela hacia:

- un estado de la barra lateral;
- una colección manual.

Si el juego activo pertenece a la multiselección, se mueve el lote completo. Las
colecciones inteligentes no aceptan drop: se actualizan mediante reglas. El destino válido,
prohibido y activo se distingue visualmente y se anuncia al lector de pantalla.

Después de un éxito aparece **Deshacer**. El recibo restaura exactamente estados u orden
anteriores solo si nadie modificó el destino desde entonces. Si hubo otro cambio, Vindexa
rechaza el recibo para no borrar trabajo posterior.

Alternativas sin puntero:

- usa la barra de multiselección para elegir estado o colección;
- enfoca el asa **Arrastrar _juego_**, pulsa Espacio o Enter para levantar, usa las flechas
  para elegir destino y vuelve a pulsar Espacio o Enter para soltar; Escape cancela.

El botón principal de la portada o fila queda separado del asa: clic, Enter o Espacio abren
la ficha exactamente una vez. Un gesto de puntero solo inicia el arrastre desde el asa y
después de superar 8 px, para que un pequeño movimiento sobre la carátula no secuestre la
apertura.

## Colecciones

### Manual

Crea una colección con nombre, descripción, color e icono. Después puedes editar esos
cuatro atributos sin perder su membresía. Añade juegos desde la selección, la ficha o drag
and drop. Al eliminarla se borra la pertenencia, no los juegos ni datos personales.

### Inteligente

Selecciona **Inteligente**, define si deben coincidir **todas (AND)** o **cualquiera (OR)**
y añade reglas. La vista previa consulta SQLite sin mutar la colección. Guarda solo cuando
el contador y la lógica sean correctos. Al editar, Vindexa carga primero las reglas
persistidas; si esa lectura falla, guardar queda bloqueado para no sobrescribirlas y puedes
reintentar.

El tipo manual/inteligente queda fijo después de crearla. Una colección inteligente no
tiene orden interno manual ni acepta drop directo. Las tarjetas de colección sí se pueden
reordenar entre sí con el asa, el teclado o los botones subir/bajar; el orden se guarda en
SQLite.

## Planificador

El planificador dispone de cuatro vistas:

- **Kanban:** mueve tarjetas entre columnas, reordena y respeta límites WIP.
- **Cola:** orden lineal independiente de las columnas.
- **Semana:** agrupa lo planificado en el periodo semanal visible.
- **Mes:** agrupa por semanas del mes visible.

Cada juego puede tener objetivo, periodo planificado, fecha objetivo y horas estimadas. El
panel de capacidad configura minutos semanales y mensuales; Vindexa compara la suma estimada
con ese presupuesto y muestra sobrecarga sin impedir que el usuario decida.

En **Ajustes → Organización** puedes crear, reordenar o eliminar columnas. Al eliminar una
columna, el diálogo explica la columna de reasignación antes de confirmar. La interfaz
actual muestra los límites WIP configurados, pero todavía no ofrece un editor de nombre,
color o límite para una columna existente.

## Seguimiento y descubrimiento

**Seguimiento** utiliza solo datos propios y metadatos observados:

- elige 30, 60 o 120 minutos y un tipo de experiencia;
- pulsa **Elige por mí** para obtener un título con razones visibles;
- descarta una recomendación o restáurala desde el historial;
- consulta seguidos, recordatorios, olvidados y casi terminados;
- crea recordatorios, complétalos o posponlos siete días;
- observa próximos lanzamientos y cambios de Early Access/fecha cuando existen dos
  observaciones fiables.
- consulta publicaciones recientes del feed oficial de Steam para juegos seguidos, sin
  introducir ni leer tu Web API Key;
- revisa lanzamientos futuros relacionados dentro de la biblioteca cuando coinciden
  exactamente el desarrollador o el editor normalizados.

El método público de Steam no expone una señal contractual de importancia. Por eso Vindexa muestra
**Publicaciones oficiales recientes**, con juego, feed y fecha, pero no las etiqueta como
«importantes». **Lanzamientos relacionados** muestra siempre el criterio y el título de
referencia; no usa IA ni similitud temática. Compara individualmente cada estudio/editor
persistido y solo cuenta metadatos cuya consulta terminó con estado `success` y títulos
familiares todavía confirmados; si faltan fechas o empresas verificadas, el estado vacío explica qué
evidencia no existe. La actualización automática respeta caché y
backoff; **Actualizar** o **Reintentar** nunca fuerza una ráfaga fuera de esa cadencia.

## Ajustes

### Steam

Vincula/desvincula cuenta, guarda/elimina/comprueba la clave, sincroniza y explora
bibliotecas. Desvincular detiene futuras sincronizaciones y limpia el catálogo familiar,
pero conserva biblioteca y organización personal.

### Organización

Los estados se pueden crear, renombrar, recolorear y reordenar. Los predeterminados no se
eliminan; al eliminar uno personal, Vindexa confirma y reasigna sus juegos a **Sin
clasificar**. Para las columnas, consulta los límites indicados en [Planificador](#planificador).
Los contadores hacen visible el impacto antes de borrar un destino usado.

### Apariencia

- **Densidad:** compacta o cómoda.
- **Confirmar desinstalación:** controla el diálogo previo de Vindexa; Steam mantiene su
  propio flujo.
- **Sincronización periódica:** manual, 30 minutos, una hora, seis horas o un día. Solo
  funciona mientras la ventana permanece abierta.

### Atajos

Pulsa **Cambiar**, presiona la combinación y espera al guardado. Las colisiones se rechazan
y **Restaurar predeterminados** recupera el mapa original. Los atajos no se ejecutan dentro
de campos, áreas editables o selectores.

### Datos y copias

Consulta ruta, tamaño, WAL, integridad y versión de esquema; exporta/restaura SQLite y vacía
la caché de imágenes. El diálogo de restauración exige confirmación y Rust valida el archivo
antes de sustituir nada.

### Acerca de

**Buscar actualizaciones** consulta la configuración del build. La versión actual no tiene
endpoint ni clave pública: mostrará **no configurado** y no descargará ni ejecutará nada.

## Atajos y teclado

| Acción | Predeterminado |
| --- | --- |
| Biblioteca | `⌘1` / `Ctrl+1` |
| Planificador | `⌘2` / `Ctrl+2` |
| Colecciones | `⌘3` / `Ctrl+3` |
| Seguimiento | `⌘4` / `Ctrl+4` |
| Buscar | `⌘K` / `Ctrl+K` |
| Sincronizar | `⌘⇧S` / `Ctrl+Mayús+S` |
| Cerrar panel o selección | `Esc` |
| Ajustes | `⌘,` / `Ctrl+,` |

Tab y Shift+Tab recorren controles; Enter/Espacio activan; Escape cierra el contexto más
reciente. El atajo de sincronización solo funciona con cuenta, marcador de clave válido y
sin verificación pendiente; anuncia éxito o error.

## Copias, actualización y desinstalación

### Exportar antes de cambiar de versión

1. Abre **Ajustes → Datos y copias**.
2. Confirma `integrity = ok` y WAL activo.
3. Exporta una copia fuera del directorio de datos.
4. Cierra Vindexa antes de sustituir el bundle.

### Desinstalar un juego

La acción solo aparece para instalaciones registradas. Vindexa valida el AppID y solicita
la desinstalación al cliente oficial. No borra carpetas ni manifiestos directamente.

### Desinstalar Vindexa

Eliminar el bundle no elimina SQLite, backups, caché ni Keychain. Sigue la sección
Desinstalar de [INSTALL.md](./INSTALL.md) y el procedimiento de borrado completo de
[PRIVACY.md](./PRIVACY.md).

## Solución de problemas

### Faltan juegos

- Ejecuta importación local y sincronización remota: cubren datos distintos.
- Revisa la visibilidad de **Detalles de juegos** en Steam.
- Para Steam Family, confirma que el cliente local contiene el grupo y que otros perfiles
  son consultables; algunos títulos quedan excluidos por Steam.
- No confundas **catálogo familiar** con biblioteca personal confirmada.

### Keychain pide contraseña

La apertura normal no lo consulta. Comprueba si acabas de guardar, comprobar, eliminar o
sincronizar. En `tauri dev`, una recompilación cambia la firma ad hoc del binario y macOS puede
volver a pedir permiso. Si ocurre antes de una acción explícita, conserva el código visible,
no la clave, y consulta [STEAM_SETUP.md](./STEAM_SETUP.md).

### No carga arte o descripción

- Comprueba conexión y que Steam responda.
- Reintenta metadatos desde la ficha.
- Vacía caché solo si hay un archivo persistentemente inválido; volverá a descargarse.
- **Sin datos** puede ser correcto si Steam no ofrece el campo.

### No se puede deshacer

Otro cambio modificó el estado u orden después del drop. Vindexa invalida el recibo para no
sobrescribirlo; mueve de nuevo el lote al destino deseado.

### La tienda integrada bloquea una navegación

Solo se permite el host superior exacto de la tienda. Para comunidad, cuenta, soporte o
login abre el navegador oficial. La ventana integrada no es un navegador general. En macOS
y Linux, un fallo o timeout al activar la protección cierra la ventana deliberadamente. La
lista es acotada y no debe interpretarse como un adblock completo.

### Bazzite

La compilación y ejecución real en Bazzite siguen sin certificarse desde el entorno macOS.
Consulta el gate explícito de [INSTALL.md](./INSTALL.md) antes de declarar soporte.
