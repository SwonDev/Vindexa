# Registro de cambios

Este archivo sigue [Keep a Changelog](https://keepachangelog.com/es-ES/1.1.0/) y el
proyecto usa [versionado semántico](https://semver.org/lang/es/). Una sección describe el
estado del código, no una publicación, mientras figure como **Sin publicar**.

## [Sin publicar]

### Añadido

- **GOG entra en las ofertas.** «Ofertas para ti» sólo miraba Steam. Ahora pregunta también al
  catálogo público de GOG, que está aquí por dos motivos: vende **sin DRM por definición** —que
  es justo lo que esta biblioteca mira— y entrega el género y el estudio con la propia oferta,
  así que sus rebajas se puntúan en el acto contra tu modelo de gustos sin una petición por
  juego. Cada fila dice de qué tienda es, y pulsarla abre esa tienda: el mismo juego no cuesta
  lo mismo en las dos. Medido en una instalación real: 30 ofertas guardadas —24 de GOG y 6 de
  Steam—, todas puntuadas.
- **Un botón para preguntar ahora.** Las tiendas se repasan solas cada seis horas, que es el
  ritmo al que cambian las rebajas, pero una que se acaba no espera. El informe dice lo que
  trajo y, sobre todo, **qué tienda no respondió**: una lista corta con pinta de completa es
  peor que no traer nada.
- **La columna de señales se reparte en tres grupos.** Eran nueve bloques apilados con
  desplazamiento, y lo que llega hoy —un regalo que caduca el jueves— quedaba a la misma altura
  visual que el histórico de descartados. Ahora hay *Oportunidades*, *Novedades* y *Avisos*,
  repartidos por lo que se hace con cada cosa. Ninguna pestaña se queda vacía: cada bloque trae
  su propio «aquí no hay nada» y lo explica.
- **Los juegos regalados van primero y se dicen «Gratis».** Steam y GOG regalan juegos durante
  unos días modelándolo como un descuento del cien por cien, así que ya entraban en la lista,
  pero ordenados por afinidad quedaban los últimos y su precio se escribía «0,00 €». Ahora lo
  que está a cero va por delante de cualquier puntuación.
- **La lista de ofertas dice cuándo se miró.** Una rebaja que terminó ayer sigue guardada hasta
  la siguiente tanda; pasadas doce horas la línea cambia de tono y avisa de que algunas pueden
  haber terminado.
- **Copias automáticas de tu base.** Todo lo que hace valiosa esta base es irrepetible —notas,
  checkpoints, estados, colecciones, el modelo de gustos— y no está en ningún servidor: el
  catálogo se puede volver a bajar, eso no. Sólo había exportación manual, que es lo mismo que no
  tener copias, porque nadie pulsa un botón todos los días. Ahora Vindexa guarda una copia al día
  en una carpeta propia junto a la base, conserva las tres últimas y borra las anteriores; usa la
  misma exportación que el botón, así que **valida** la copia después de escribirla. Ajustes →
  Datos dice cuándo fue la última, cuántas hay, cuánto ocupan y dónde están, y si alguna falló lo
  enseña: una copia que dejó de hacerse en silencio sólo se descubre el día que se necesita.
- **Los próximos lanzamientos por fin descubren algo.** Salían sólo de tu lista de deseados, así
  que la sección recordaba juegos que ya habías marcado tú y no enseñaba nada nuevo. Ahora mira
  también la sección «próximamente» del escaparate público de Steam —la misma petición sin clave
  ni sesión que ya hace el radar de ofertas—, se queda con lo que **es un juego** (ni demos ni
  DLC, que el escaparate mezcla), no tienes y la ficha confirma sin publicar, y lo puntúa contra
  el mismo modelo local. Lo que no venía de tus deseados se marca como **hallazgo**, porque un
  recordatorio y un descubrimiento no son la misma clase de aviso. Medido en tu instalación: de
  los diez destacados entraron ocho, y los dos que se quedaron fuera eran justo la demo y el DLC.
- **Los regalos de Epic también tienen vista rápida.** Guardaban su imagen desde el principio y
  no se pintaba en ninguna parte: ahora, al pasar el ratón, se enseña con lo que Vindexa sabe
  —si ya lo tienes, cuánto queda y de qué va—.
- **La pantalla de Privacidad dice también qué sale del equipo.** Enumeraba cuatro garantías
  sobre lo que se queda y ninguna sobre lo que viaja. Ahora están las dos direcciones: qué se
  pregunta, a dónde, y qué no sale nunca.

### Corregido

- **Un juego a precio completo aparecía bajo «Ofertas para ti».** El escaparate de más vendidos
  de Steam mezcla rebajas con precios normales, y ahí estaba *Mortal Shell II* a 49,99 € sin
  descuento. Una oferta ahora exige un precio de referencia **mayor** que el vigente: sin eso no
  hay rebaja que comprobar.
- **«No a la venta» juntaba lo retirado con lo que aún no ha salido.** Los dos llegan igual desde
  la tienda —«no publica precio»—, pero sólo uno de los dos es una espera. Ahora un deseado que
  está entre los próximos lanzamientos dice «aún no ha salido», y la cobertura los cuenta aparte.
- **«451 juegos sin precio consultado» eran 451 juegos consultados.** La tienda había respondido
  por todos: no publica precio para juegos sin fecha de salida, gratuitos o retirados. Esa
  respuesta ahora se guarda, se cuenta aparte —«no pone a la venta» frente a «sin consultar»—,
  cada fila dice cuál de las dos cosas le pasa, y deja de repreguntarse cada seis horas por algo
  que no va a cambiar hoy.
- **La evidencia del DRM enseñaba el nombre interno del campo.** «extUserAccountNotice → Rockstar
  Games», escondido dentro de un emergente y concatenado con el resto de la explicación. La marca
  de DRM sólo vale si se puede comprobar, y no se comprueba lo que no se entiende: ahora la
  evidencia se lee bajo la insignia, en palabras —«Cuenta externa que pide la ficha»— y con sitio
  para que quepa en una línea.
- **El repaso de DRM se atascaba en los juegos retirados.** Cuando la tienda contesta que un
  juego ya no existe, eso no se apuntaba en ninguna parte, así que seguía en la cola. Y como la
  cola va por AppID ascendente y los juegos viejos son justo los que más se retiran, cada tanda
  gastaba su cabecera preguntando una y otra vez por los mismos juegos que ya no están, cada diez
  minutos. Ahora se apunta **que se preguntó** —el estado sigue siendo «no se sabe», porque no se
  sabe— y la cola avanza.
- **«Sin DRM 604» parecía un total y era un avance.** El repaso pregunta a la tienda juego a
  juego y una biblioteca grande tarda horas. Mientras queden por comprobar, la cabecera dice
  cuántos y aclara que «sin comprobar» no es «lleva DRM».
- **Un permiso del llavero denegado se enseñaba como «sin sesión iniciada».** La sesión de la
  tienda estaba guardada; lo que faltaba era el permiso, y volver a iniciar sesión no arregla un
  permiso. Ahora la tarjeta dice que no se pudo leer y lleva a Acceso a Llaveros nombrando la
  entrada exacta. Además la pantalla deja de preguntar en cada visita: recuerda mientras corre
  los tres datos que enseña —nombre y caducidades, nunca el testigo— y recalcula las caducidades
  contra el reloj de cada consulta.
- **El tráiler de una ficha de GOG disparaba un aviso de destino no permitido.** Es contenido de
  la página que se pidió, no un destino: ahora carga como el marco de la verificación humana,
  sin tocar la barra de direcciones ni el historial, y sólo desde el dominio sin cookies de
  YouTube.
- **Todas las fechas de los próximos lanzamientos salían como aproximadas.** La exactitud se
  preguntaba a un campo que se deja vacío a propósito para todo lo que aún no ha salido, así que
  «19 AGO 2026» —un día concreto— se pintaba «≈ 19 AGO 2026». Ahora se lee la etiqueta: un día se
  ve como una fecha y un «Q4 2026» se sigue viendo como lo que es.
- **Nadie retiraba a un candidato que ya había salido.** La pasada de deseados se saltaba los
  publicados sin borrar los que ya estaban, y un hallazgo del escaparate no está en deseados, así
  que no se revisaba nunca: la sección se habría llenado de juegos con la fecha pasada afirmando
  ser próximos. Ahora se vuelve a preguntar por tandas —primero los que anunciaban un día que ya
  pasó—, los publicados se retiran y los que siguen esperando se refrescan. Retirar no es
  descartar: descartar enseña al modelo que no te interesa, y salir no dice nada de tu gusto.
- **«12 candidatos puntuados» era el tope de la lista, no lo que había.** Con cuarenta y cinco
  guardados, esa cifra leída como total es falsa. Ahora dice «12 de 45, los que más encajan», que
  además explica por qué se recortan.
- **«Steam · al día» lo decía aunque la última sincronización fuera de hace cuatro días.** Que una
  llamada terminara bien no dice nada de lo que ha pasado en tu biblioteca desde entonces, y menos
  con la sincronización periódica en «sólo manual». Pasado un día, el distintivo dice **cuándo**
  fue; sin fecha guardada no afirma ninguna de las dos cosas.
- **Escribiendo en el buscador no funcionaba ningún atajo.** Para ir a otra sección había que
  soltar el teclado y coger el ratón. Ahora pasa el número con modificador —⌘1 a ⌘5—, que dentro
  de un campo no significa nada, y sigue sin pasar lo que sí significa algo: `⌘A` selecciona el
  texto, `⌘C` lo copia y `Ctrl+K` corta hasta el final de la línea.
- **⌘5 no hacía nada y Ajustes lo enseñaba «sin asignar».** Deseados tenía dos dueños: el campo
  que Rust guarda desde siempre y una acción local duplicada, sostenida por un comentario que
  había dejado de ser verdad. La combinación quedaba ocupada por un enlace que nadie escuchaba.
  Ahora hay un solo dueño, y una prueba lee la `struct` de Rust para que los dos lados no puedan
  volver a separarse.
- **Un anuncio de Steam entero llenaba la bandeja de avisos.** Tres publicaciones ocupaban toda
  la bandeja y había que desplazarse para saber si quedaba algo más. Se recortan a tres líneas y
  se despliegan bajo petición, cada una por su cuenta.
- **El llavero denegado ya no pregunta en bucle, y tiene salida.** La negativa se recuerda
  mientras corre la aplicación —antes cada refresco abría dos diálogos de contraseña— y la
  tarjeta ofrece «Volver a intentarlo» para no obligar a reiniciar. itch.io dice ahora lo mismo
  en vez de pedir que generes otra clave cuando la que había seguía guardada.
- **La carátula de una oferta de GOG se guardaba y no se pintaba.** GOG sirve las de su catálogo
  desde `gog-statics.com`, que no estaba declarado en la política de contenido de la ventana: la
  imagen se pedía y el navegador la bloqueaba en silencio, dejando el hueco con las iniciales
  —que es justo lo que se enseña cuando **no hay** carátula—. Ahora se declara, y una dirección
  de un anfitrión que la ventana no va a pintar ya no se guarda.
- **El subrayado de la pestaña activa del planificador flotaba por debajo.** El estilo estaba
  escrito contra un atributo que Radix no pone, así que no pintaba nada y mandaba la utilidad de
  la librería, que dibuja la marca cinco píxeles por debajo del disparador: blanca y separada de
  la pestaña que marcaba. Ahora es cian y va pegada al borde, y una prueba recorre las hojas de
  estilo para que ninguna vuelva a vestir un componente de Radix por un estado que no emite.
- «Sin coincidencia medida» aparecía sobre ofertas que **sí** se habían medido y daban cero; ya
  dice lo que ocurre, que es que no hay señales en común.
- La vista rápida ya no corta la razón de la coincidencia a media palabra, la lista larga de
  ofertas se puede volver a recoger, el deslizador de progreso de la ficha deja de montarse
  sobre su etiqueta al llegar con el teclado, y la lista de privacidad deja de partir cada
  frase en columnas.
- **Trescientos dieciocho juegos esperaban a Steam para siempre.** Un juego de Epic, GOG o
  itch.io no existe en Steam, así que su ficha nunca se pide; su estado se quedaba en «pendiente»
  de por vida y la pantalla lo leía como trabajo en curso: «Cargando la descripción desde
  Steam…», con el girador dando vueltas. Ahora dice de qué tienda viene y que lo suyo es local.
  Lo mismo pasaba con los 1.583 del catálogo de Steam Family que aún no consta que puedas jugar.
- **Abrir un juego de Epic le pedía a Steam un AppID inventado.** Los juegos de otras tiendas
  llevan un identificador que se inventó Vindexa. Abrir su ficha se lo pedía a Steam, Steam
  contestaba que no existe y el juego quedaba marcado «Ficha no publicada» con un botón para
  reintentarlo cada día: una etiqueta que culpaba a Steam de no publicar algo que nunca fue suyo.
  Pasaba igual con los logros y con el contenido adicional. `LOCAL_APP_ID_BASE` ya decía en su
  documentación que ninguna consulta a Steam debe llevar uno de esos identificadores —«fichas,
  precios, logros, arte»—; sólo el arte lo cumplía.
- **«Olvidados 12» eran 280.** Las listas del radar traen doce de cada montón y la cabecera decía
  «12 elementos en esta vista». Con 280 juegos parados de verdad, es el mismo recuento que
  contestó «20» cuando había 215. Ahora dice «12 de 280», el número se puede recorrer con una
  tanda más al final de la lista, y las señales de Novedades hacen lo mismo cuando enseñan cuatro
  de doce.
- **«336 sin comprobar» eran 62.** El repaso de DRM le pregunta a la ficha de Steam, así que un
  juego de Epic o de itch.io no entra: no hay a quién preguntarle. La cifra los contaba igual, y
  la nota prometía una cuenta atrás que se quedaba en 274 para siempre.
- **La barra de estado prometía completar 317 fichas que nadie iba a pedir.** El recuento de
  pendientes usaba una condición distinta de la que encola el trabajo.
- **Los deseados que ya tienes se pueden esconder.** La lista existe para decidir una compra y
  65 de sus 1.410 entradas son juegos ya comprados; con el orden por descuento ocupaban la
  cabecera. El interruptor va apagado, y al encenderlo el recuento pasa a «1.345 de 1.410».

## [0.1.4] · 2026-08-19

### Añadido

- **El arte se completa solo mientras no haces nada.** La rejilla ya adelantaba las carátulas de
  la página cargada, pero eso sólo cubría lo que se había llegado a mirar: el resto se descargaba
  al desplazarse hasta él, con retraso, y sin conexión no aparecía. Ahora, con la biblioteca
  abierta, el arte que falta se va guardando en local en los ratos de reposo del navegador.
  - **Se para antes de llenar la caché.** No basta con confiar en que el mantenimiento desaloje
    lo menos usado: si el arte completo ocupa más que el techo configurado, adelantar y desalojar
    entran en un tira y afloja que gasta red y disco sin dejar nada. Medido sobre una biblioteca
    real: la caché **bajaba** de tamaño mientras la precarga corría. Se detiene al 85 % del
    presupuesto, y el margen que queda es para lo que pides al mirar, que siempre va primero.
  - Si quieres que toda tu biblioteca esté en local, sube el presupuesto de la caché en
    Ajustes → Datos y copias: con miles de juegos, el arte completo puede pedir bastante más que
    los 512 MiB de fábrica.
- **Los próximos lanzamientos ya tienen de dónde salir.** El motor que aprende tus gustos y
  puntúa candidatos existía, y la pantalla que los enseña también, pero nadie rellenaba la tabla
  de candidatos: la sección quedaba vacía por muy bien que funcionara todo lo demás. Ahora salen
  de **tu lista de deseados**, que es la única fuente honesta a mano —son juegos que marcaste tú,
  no novedades que a Vindexa se le ocurran—: se revisa por tandas cuáles siguen sin publicarse,
  según lo que declara su ficha oficial, y se guardan con la fecha tal cual la publica la tienda.
  Cuando sólo dice «Q4 2026», eso es lo que se guarda, marcado como no exacta: inventar una fecha
  sería peor que no tenerla. Medido sobre una lista real: de los primeros 60 deseados revisados,
  19 seguían sin salir.
- **Apartado de Agentes en Ajustes.** El puente que permite a un agente externo ordenar tu
  biblioteca —registrar sesiones, cambiar estados, crear colecciones, planificar— llevaba tiempo
  escrito en Rust, pero no había forma de emitirle un testigo, así que no se podía usar. Ahora se
  emite desde Ajustes, con permisos por ámbito, y se ve lo que cada agente ha hecho, con deshacer.
  Las cuatro barreras siguen intactas: no se abre ningún puerto de escucha, el testigo se enseña
  una sola vez —Vindexa guarda su huella con sal—, un agente sólo puede hacer aquello para lo que
  se le dio permiso y no puede aprobar sus propias acciones destructivas.
- **La biblioteca acusa el tacto.** La librería de microinteracciones del proyecto —inspirada en
  el catálogo de SmoothUI y reescrita para la estética densa y sin rebote de `DESIGN.md`— ya se
  usaba en Colecciones y en Listas curadas, pero no en la biblioteca. Ahora las tarjetas y las
  filas de la barra lateral suben un píxel al pasar el puntero y se hunden al pulsar, y los
  recuentos cuentan hacia su nuevo valor en vez de saltar. Nada altera la caja de su contenedor,
  así que la rejilla virtualizada sigue midiendo igual, y `prefers-reduced-motion` lo desactiva
  entero.
- **Vídeos del juego en su propia ficha.** El panel que ya existía en Deseados —con su
  reproductor sin seguimiento— pasa a estar también en la ficha de cualquier juego de la
  biblioteca, en una pestaña propia. La dirección del marco la construye Rust y apunta a
  `youtube-nocookie.com`, el único origen que la política de contenido de la ventana admite:
  reproduce sin el rastreo habitual y sin que la página pueda llevarte a ningún otro sitio.
- **Acciones rápidas con el botón derecho en la barra lateral.** Una colección cambia de color y
  de icono desde su propio menú, sin abrir el editor; y sobre una tienda, «Abrir en el navegador
  integrado» la abre con la sesión que ya tengas iniciada en ella, porque cada una guarda la suya
  aparte. Editar y eliminar siguen pidiendo confirmación o abriendo el editor: un menú rápido no
  puede cambiar lo que una colección significa. La orden que envía sólo escribe el color y el
  icono, así que las reglas de una colección inteligente no pueden perderse por un descuido.

### Cambiado

- **La barra de estado sólo aparece cuando tiene algo que decir.** Repetía tres cosas que ya
  están en pantalla —el recuento de la biblioteca, que lo dice la barra lateral; el estado de
  Steam, que lo dice su ficha de la cabecera; y el detalle de SQLite, que no es noticia mientras
  funcione— y a cambio se comía una fila de alto en todas las pantallas. Ahora enseña el trabajo
  en curso y lo que ha salido mal, que es lo único que no se ve en ningún otro sitio, y cuando no
  hay ni una cosa ni otra desaparece y esa altura vuelve a las carátulas.
- **La ventana abre ocupando el espacio disponible.** El ajuste `maximized` no basta en macOS,
  donde «maximizar» es el *zoom* del sistema y lleva la ventana al tamaño que la aplicación
  declara como preferido: quedaba en 1440×870 sobre una pantalla útil de 1512×982. Ahora se mide
  el hueco real —descontando la barra de menús y el Dock— y se ocupa entero. Después la ventana
  es de quien la usa: si la redimensionas, nadie te lo deshace.
- **El arrastre y el doble clic de la barra de título pasan a manos de Tauri.** Estaban resueltos
  a mano y el doble clic no llegaba a dispararse: pedir el arrastre al pulsar hace que el sistema
  se quede con el puntero y el segundo clic no llegue nunca. Declarar la barra como zona de
  arrastre deja ese trabajo en el mecanismo del framework, que ya contempla el orden de eventos
  de cada sistema.

### Corregido

- **La insignia de versión del README no seguía a las publicaciones.** Estaba escrita a mano, así
  que se quedó anclada a la 0.1.2 mientras el proyecto avanzaba. Ahora la genera GitHub a partir
  de la última release. Por lo mismo, tres documentos que decían describir «Vindexa 0.1.0» dejan
  de atarse a un número: se mantienen con el código y describen lo que hay hoy.
- **La marca DRM-Free de las tiendas no llegaba a la ficha.** Al integrar Epic, GOG e itch.io en
  la biblioteca se quedó por el camino un dato que su catálogo sí tenía: GOG declara que todo lo
  que vende va sin DRM, y aun así 44 de sus juegos figuraban como «sin dato». Ahora ese dato se
  propaga, pero **sólo donde no se sabía nada**: un aviso oficial de la ficha de Steam es
  evidencia del juego concreto y una política general de tienda no puede pisarla. Sigue sin
  dibujarse sobre las carátulas: es un dato de ficha y se enseña con la evidencia que lo motiva.
- **La barra superior se solapaba consigo misma en ventanas estrechas.** Cinco pestañas con
  rótulo no bajan de 540 px y su columna sólo tenía garantizados 400, así que lo que sobraba se
  pintaba encima de la tarjeta de cuenta: se llegaban a leer dos textos superpuestos. Ahora la
  navegación recorta lo que no cabe y, según va faltando sitio, se retira primero el rótulo de la
  cuenta y después el de las pestañas —que conservan icono, tooltip y etiqueta accesible—.

## [0.1.3] · 2026-08-19

### Añadido

- **Los juegos de Epic, GOG e itch.io entran en la biblioteca y se organizan como los demás.**
  Vivían en una tabla de catálogo sin ficha personal, así que no había dónde guardarles un
  estado, una colección, una prioridad ni una nota: se miraban y nada más, sin arrastre ni
  planificador. Ahora tienen su sitio en la biblioteca, con el mismo listado, los mismos filtros
  y la misma ficha. Es el mismo camino que recorrió el préstamo familiar en la 0.1.1.
  - Entran como **propios**, porque lo son: se compraron. Cuentan en «Todos los juegos».
  - Un juego que tienes en dos tiendas **no se duplica**: apunta a la fila que ya existía y
    aparece igualmente al mirar cualquiera de las dos.
  - Como `games` se indexa por AppID de Steam y estos juegos no tienen ninguno, se les asigna uno
    local a partir de 2.000.000.000. Ese número marca además una frontera: por encima de él, el
    juego no existe para Steam, y ninguna consulta a su API lo lleva.
- **Su arte también se guarda en local.** La caché sólo aceptaba imágenes de Steam, así que las
  carátulas de las demás tiendas se volvían a descargar en cada arranque. Ahora se guardan como
  las de Steam y se pintan sin esperar.

### Corregido

- **Ni Epic ni itch.io aparecían en la barra lateral por muchos juegos que trajeran.** Sus
  ámbitos salen de los recuentos del arranque, y sincronizar una tienda no los invalidaba: había
  que cerrar y volver a abrir la aplicación, y mientras tanto parecía que la sincronización no
  había servido de nada. GOG sí aparecía sólo porque se había sincronizado antes de abrirla.
- **Las carátulas de las tiendas que no son Steam no cargaban.** La política de contenido de la
  ventana permitía imágenes de los dominios de Steam y de ninguno más, así que sólo se veía la de
  los juegos emparejados con Steam. Se añaden los dominios de GOG, Epic e itch.io, medidos sobre
  una biblioteca real.
- **Las pruebas pedían la contraseña del llavero.** Alguna abría el llavero de verdad, y como el
  binario de pruebas cambia de firma en cada compilación, macOS lo trata como un programa nuevo y
  vuelve a preguntar: «Permitir siempre» no servía de nada. Además hacía que el resultado
  dependiera de qué secretos tuviera esa máquina. Ahora todo el acceso pasa por un único módulo
  que, al compilar para pruebas, guarda en memoria.

- **El banner de todas las fichas se veía gris azulado.** Steam publica dos imágenes anchas por
  juego: `library_hero`, que es la ilustración a color, y el fondo de la página de la tienda, que
  viene ya oscurecido y difuminado porque está pensado para escribir encima. La caché de arte
  ordena sus candidatas de mejor a peor, pero para decidir el puesto de la que pedía la interfaz
  miraba el nombre del archivo, y el fondo de la tienda no se pide por nombre sino por ruta
  (`/images/storepagebackground/app/1337760`). Al no reconocerlo, lo colocaba en el primer puesto
  —«esto es lo mejor que hay»— y **ninguna** ficha llegaba a intentar `library_hero`, aunque
  existiera y respondiera. Ahora esa ruta se traduce a su peldaño real, el más bajo de los tres.
  La prueba que cubría esta escalera pedía las candidatas sin fuente elegida, que es justo el
  único camino que la aplicación no recorre; la nueva usa el que sí.
- **Una base de datos vacía borraba toda la caché de arte.** El mantenimiento elimina la carpeta
  de cada juego que no encuentra en la base, y una base sin juegos no significa «ninguno vale»
  sino «todavía no sé nada»: el primer arranque, una restauración a medias o una cuarentena
  reciente pasan por ese estado con toda normalidad. Ocurrió: tras una cuarentena, la aplicación
  arrancó en vacío y el barrido se llevó cientos de megas de arte ya descargado. Ahora una base
  sin juegos no borra nada; si de verdad sobra algo, el barrido siguiente lo encontrará igual.

- **La importación de itch.io no llegaba a guardar ni un juego.** Cuando se agotan las claves de
  descarga, itch.io no devuelve una lista vacía sino una tabla vacía —su servidor corre sobre
  Lua, donde una tabla sin contenido no distingue entre lista y diccionario—, y esa última
  página rompía el análisis. La importación se caía **después** de haber leído bien todas las
  anteriores, así que el resultado era siempre el mismo: «itch.io respondió con un formato que
  Vindexa no reconoce» y cero juegos, por muchas veces que se reintentara. Ahora las dos formas
  se leen igual, y `null` tampoco es un fallo.
- **El inicio de sesión de Epic y de GOG se quedaba girando para siempre.** Los dos incrustan su
  verificación humana en un marco de un tercero —hCaptcha en Epic, reCAPTCHA en GOG—, y el motor
  web entrega también las navegaciones de los marcos a la política de la ventana, que sólo
  conocía los dominios de la tienda. El captcha se cancelaba antes de arrancar y la comprobación
  del correo no volvía nunca. Esos dos proveedores tienen ahora un permiso propio y acotado:
  cargan el marco pero no se convierten en la página de la ventana, así que ni aparecen en la
  barra de direcciones ni entran en el historial.
- **Un marco sin dirección se tomaba por un destino prohibido.** El captcha crea varios marcos
  vacíos, a veces con un fragmento detrás, y sólo se aceptaba la cadena `about:blank` exacta.
  Las páginas internas del motor (`about:config` y demás) siguen cerradas.

### Cambiado

- **Los avisos de bloqueo del navegador integrado dicen qué se bloqueó.** Un rechazo por esquema
  ya nombra el esquema, como el de destino ya nombraba el anfitrión. Nunca la dirección
  completa: la de un `data:` lleva el documento dentro y la de un retorno de sesión lleva el
  código de autorización.

## [0.1.2] · 2026-08-18

### Corregido

- **El título de una columna del planificador se partía en dos líneas.** «A continuación» no
  cabía en su cabecera de 42 px y se salía de la fila, descolocando el recuento, el límite y el
  botón de añadir respecto a las demás columnas. Ahora se recorta con puntos suspensivos y el
  nombre completo queda en el rótulo del elemento.
- **La compilación fallaba en Windows y en Linux.** Al retirar el permiso de código muerto a
  nivel de módulo de la sesión de deseados, el margen de espera y el error del evaluador
  quedaron sin llamador fuera de macOS: sólo los usa la implementación de WKWebView, y en las
  otras plataformas no hay evaluador al que esperar. Es el mismo patrón que ya se corrigió en el
  bloqueador de contenido.

## [0.1.1] · 2026-08-18

### Añadido

- **Los juegos del préstamo familiar se pueden organizar como los tuyos.** Vivían en una tabla
  de catálogo sin ficha personal, así que no había dónde guardarles un estado, una colección,
  una prioridad ni una nota: se podían mirar y nada más. Ahora entran en la biblioteca como
  compartidos —sin contarse en «Todos los juegos», porque tenerlos a la vista no es tenerlos— y
  Steam Family usa el listado de la biblioteca: filtros, diecisiete órdenes, agrupación, vistas
  guardadas, selección múltiple, arrastre, menú contextual y ficha.
- **Epic, GOG e itch.io salen de Ajustes** y son ámbitos de la biblioteca, con su recuento en la
  barra lateral y el mismo listado. Antes eran una lista de texto sin portadas.
- **Un solo listado para los catálogos.** La rejilla, la virtualización, el cálculo de columnas,
  el fundido de borde y la carga anticipada de portadas viven en un componente compartido; cada
  catálogo sólo traduce sus datos.

### Corregido

- **El recuento de Steam Family no se veía sin entrar**, porque salía de una consulta que sólo
  se ejecutaba dentro de esa sección: había que entrar para ver el número que te dice que ahí
  hay algo. En una biblioteca con 1801 juegos prestados, eso se lee como que faltan.
- **El recuento de cada estado volvía a no coincidir con su listado** al dar ficha personal a
  los prestados: la barra ofrecía juegos que la pantalla escondía. Los recuentos de estados y
  colecciones aplican ahora las mismas exclusiones que el listado, y los archivados dejan de
  inflarlos.
- **Las carátulas del catálogo de Family** seguían apuntando a la variante pequeña, que mide
  300×450 y para muchos títulos ni existe. La migración que las subió a la grande se había
  dejado fuera esa tabla.
- **Dos aspas de borrar en el campo de búsqueda**: WebKit dibuja la suya dentro de
  `input[type="search"]` junto a la del proyecto.
- **El pulgar del interruptor no cabía en su carril** —dos píxeles más alto que su hueco— y se
  quedaba en el color de fondo de la aplicación, porque las variantes `dark:` de shadcn nunca se
  aplican: la aplicación es oscura de raíz y nada añade esa clase al documento.
- **El elemento arrastrado temblaba**: llevaba la traslación del arrastre además del acompañante
  que sigue al cursor, y dentro de una fila virtualizada esa suma peleaba contra la posición
  recalculada en cada medida.

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
