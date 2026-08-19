# Cómo se trabaja en Vindexa

Estas reglas no son estilo. Están escritas después de que un fallo concreto se
colara hasta la máquina de quien usa esto, y cada una nombra el fallo que la
motivó. Si alguna sobra, se borra explicando por qué; mientras esté, se cumple.

---

## 1. Que exista no es que funcione

**El fallo**: los cuatro menús contextuales estaban escritos, montados y con sus
acciones. Ninguno abría. Una guarda global cancelaba `contextmenu` en fase de
captura y Radix, que compone sus manejadores con `checkForDefaultPrevented`, se
saltaba el suyo. Compilaba, las pruebas pasaban y el código estaba ahí.

Al lado había un comentario afirmando que no interfería. Era falso, y esa frase
es lo que sostuvo el fallo durante toda la vida del archivo.

**La regla**: una función no está hecha hasta que se ha visto hacer lo suyo.
`grep` demuestra que hay código; no demuestra que haya comportamiento. Antes de
decir que algo está terminado hay que haber ejecutado el camino completo: la
prueba que lo monta entero, la aplicación instalada, o las dos.

Al informar, separar siempre tres cosas: **lo comprobado ejecutándolo**, **lo
comprobado leyendo** y **lo que no se ha comprobado**. Lo tercero se dice, no se
omite.

## 2. Lo que se pisa no está en ninguna de las dos piezas

**El fallo**: el mismo. Ni la guarda ni el menú estaban mal por separado; el
fallo era la convivencia, y por eso ninguna prueba de una pieza podía verlo.

**La regla**: donde haya un manejador global —`document`, `window`, un
interceptor, un middleware— tiene que existir una prueba que monte **el global y
la función juntos**. Si el global cancela algo, la prueba demuestra que lo que
debía seguir funcionando sigue funcionando.

Vive en `src/test/webview-guards-composition.test.tsx`. Cualquier guarda nueva
entra ahí el mismo día.

## 3. Un comentario que afirma algo lo demuestra o no lo afirma

**El fallo**: «Radix no consulta `defaultPrevented`, así que adelantarnos no
interfiere». Nadie lo comprobó nunca.

**La regla**: un comentario que afirma cómo se comporta un tercero necesita, al
lado, una prueba que lo verifique, o la frase se escribe como lo que es —una
suposición— o no se escribe. Los comentarios que explican **por qué** se decidió
algo son bienvenidos y no caducan; los que afirman **cómo se comporta otro
código** caducan solos y mienten en silencio.

## 4. Lo que se decide para toda la aplicación se aplica a toda la aplicación

**El fallo**: arreglado el clic derecho, seguía sin hacer nada en Colecciones,
en el Planificador, en Deseados, en Seguimiento, en el modo salón y en las
vistas guardadas. Los menús existían **sólo en la biblioteca**, mientras que
cancelar el menú nativo de WebKit se hacía en todas partes. Media aplicación se
quedó sin menú de ninguna clase, y desde dentro parecía terminado porque la
pantalla que se miraba funcionaba.

**La regla**: cuando una decisión se toma a nivel de aplicación —cancelar un
gesto del sistema, interceptar una tecla, apagar un comportamiento del motor—,
lo que la sustituye tiene que llegar a todas las pantallas, y hace falta una
prueba que enumere dónde. Está en
`src/test/context-menu-coverage.test.tsx`: una pantalla nueva sin menú falla, y
una que lo pierda también.

Y el corolario: cuando quien usa esto señala un fallo en un sitio, la pregunta
no es «¿está arreglado ahí?» sino «¿dónde más pasa lo mismo?».

## 5. Un dato que no se sabe no se rellena

**El fallo**: preguntado por Telegram cuántos juegos había en Backlog, el agente
contestó «20». Había 215. La consulta devolvía una página recortada y ningún
total, así que contó las filas que le llegaron.

**La regla**: toda respuesta con límite declara cuántos hay de verdad
(`matched`), cuántos van (`shown`) y si se ha recortado. Un recuento que no
coincide con la pantalla a la que lleva es peor que no dar ninguno.

Lo mismo con los huecos: `null` es «no se sabe», y no se traduce a cero, a vacío
ni a una estimación. Si no se puede leer la memoria del sistema, no se recomienda
un tamaño de modelo. Si Steam no publica una fecha, no se inventa.

## 6. Un fallo así rara vez está en un solo sitio

**El fallo**: tras arreglar el recuento de la biblioteca, el mismo error estaba
en sesiones y en auditoría. Y la comprobación de «esto es local» que se corrigió
en el agente estaba repetida, mal, en el dictado, escrita después.

**La regla**: arreglado un fallo, se busca el patrón en el resto del repositorio
**antes** de darlo por cerrado. Y lo que se corrige se extrae a una función
compartida, porque copiar una comprobación es copiar su próxima versión rota.

## 7. Comparar el principio de una cadena no es validar

**El fallo**: `base_url.starts_with("http://127.0.0.1:")` deja pasar
`http://127.0.0.1:8080@servidor.ajeno.tld/`. Lo de delante de la arroba son
credenciales; la petición viaja a otro servidor. Con la biblioteca dentro, y en
el dictado, con la voz de quien la usa.

**La regla**: una URL se analiza y se mira el anfitrión de verdad, más el
esquema, las credenciales, la ruta, la consulta y el fragmento. Los destinos se
componen con `join` sobre la URL ya analizada. Lo mismo vale para rutas de
archivo, identificadores y cualquier frontera: se interpreta el valor, no su
prefijo.

## 8. Salir con código cero no es haber hecho el trabajo

**El fallo**: `hermes mcp add` terminaba con éxito sin registrar nada, porque una
pregunta interactiva se cancelaba sola. Y un `pnpm tauri build` que falló al
empaquetar el DMG dejó instalar el binario anterior sin que se notara.

**La regla**: después de una operación externa se comprueba **el efecto**, no el
código de salida: que el servidor aparezca en la lista, que el archivo esté donde
se le va a buscar, que la versión instalada lleve el cambio. Y en una cadena de
comandos, un fallo en medio no puede dejar seguir a los siguientes.

## 9. Los datos de quien usa esto no son un banco de pruebas

**El fallo**: una batería de pruebas registró servidores MCP en el Hermes real de
la máquina donde corría.

**La regla**: las pruebas no tocan la configuración, los servicios ni los datos
de nadie. Cuando una necesita el sistema de verdad, se apaga por defecto y se
enciende con una variable de entorno explícita (`VINDEXA_SIN_AGENTES`,
`VINDEXA_STT`). Y lo que se toque para comprobar algo se deja como estaba.

## 10. Las credenciales no pasan por la pantalla

Un testigo no se imprime, no se escribe en un mensaje, no entra en un comando
cuyo texto queda registrado y no se guarda en SQLite: va al llavero del sistema
o al archivo del programa que lo necesita, escrito sin eco.

Y antes de guardar uno, se comprueba **a qué pertenece**: un testigo pegado de
un mensaje antiguo apuntaba a otro bot distinto, y usarlo habría secuestrado el
que ya funcionaba.

## 11. Lo que se apaga solo, se dice

Cuando algo se degrada en silencio —una precarga que se detiene, un enlace que no
se pudo rehacer, un encargo que no corrió porque no había modelo— tiene que
quedar dicho en algún sitio que se mire: la pantalla, la auditoría o el registro.
Una función que falla callada es indistinguible de una que nadie ha usado.

---

## Antes de decir que algo está terminado

1. `pnpm test` y `cargo test --manifest-path src-tauri/Cargo.toml` en verde.
2. `cargo clippy --all-targets -- --deny warnings` sin nada.
3. `pnpm exec biome check src` limpio.
4. La aplicación **construida, firmada, instalada y abierta**, y el camino nuevo
   recorrido en ella. Si no se ha podido recorrer —hay gestos que no se pueden
   inyectar en un WebView—, se dice cuál y por qué.
5. Una prueba que falle si el fallo vuelve. Escrita antes de arreglar, si se
   puede: así se sabe que prueba lo que se cree.
