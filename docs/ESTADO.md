# Qué hay hecho, y cómo comprobarlo

Este documento existe porque «está hecho» no significa nada si no se puede
verificar. Cada punto dice **dónde vive** y **qué comando o consulta lo
demuestra** en una instalación real, sin fiarse de la palabra de nadie.

Las cifras son las medidas en la máquina de esta casa el 19 de agosto de 2026.
En otra instalación cambiarán; lo que no cambia es la forma de comprobarlo.

---

## 1. Microinteracciones

Librería propia en `src/components/motion/`: once componentes inspirados en el
catálogo de [SmoothUI](https://smoothui.dev/docs/components/) pero reescritos
para la estética de Vindexa —densa, oscura, radios de 0 a 3 px, sin rebote—.
Las reglas que cumple todo lo de esa carpeta están en su `README.md`, y hay
pruebas que las verifican: duraciones, curvas, muelles sobreamortiguados y que
`prefers-reduced-motion` **desactiva** el movimiento en vez de atenuarlo.

Se usan en dieciocho pantallas. En Biblioteca y Listas curadas, concretamente:

| Dónde | Qué hace |
|---|---|
| Recuentos de la barra lateral y la barra de herramientas | `AnimatedNumber` |
| Tarjetas de la cuadrícula y de las listas | `PressableSurface` |
| Lista de estados de la barra lateral | `StaggerList` al desplegar |
| Filtros activos | `StaggerList` al aplicarse |
| Detalle de una lista curada | `RevealOnScroll` al cambiar de lista |
| Conmutador de vista | `SegmentedControl`, con indicador deslizante |
| Cargas | `ShimmerSkeleton` |
| Arrastres | `DragFeedbackSurface` |

No hay movimiento en las tarjetas de colección ni en las de listas al arrastrar,
y es deliberado: el arrastre usa la misma transformación CSS y las dos se pelean.
Romper un arrastre que funciona por una animación sería un mal negocio.

```
ls src/components/motion/*.tsx | wc -l        # 11
grep -rl "@/components/motion" src/features | wc -l   # 18
```

## 2. YouTube sin publicidad

Los vídeos se sirven sólo desde `youtube-nocookie.com`, y no es una convención:
Rust lo impone en la lista de orígenes admitidos, así que una URL de otro sitio
no se puede ni guardar.

```
grep -n "youtube-nocookie" src-tauri/src/db/wishlist.rs
```

Se ve en la ficha de un juego (`GameDetailSheet`) y en Deseados
(`WishlistBoard`), a través de `GameVideoPanel`.

## 3. Próximos lanzamientos con aprendizaje

Tres piezas: traer de la lista de deseados lo que aún no ha salido
(`steam::upcoming`), aprender del historial qué te gusta
(`db::priority::learn_taste`) y puntuar los candidatos contra ese modelo.

Corría sólo al pulsar un botón que nadie sabía que había que pulsar; ahora se
repasa solo cada doce horas.

```sql
SELECT (SELECT COUNT(*) FROM upcoming_releases) AS lanzamientos,
       (SELECT COUNT(*) FROM taste_weights)     AS pesos_aprendidos;
-- medido: 18 | 534
```

## 4. Precios de lo que deseas

Se piden a `appdetails` en lotes de cien, con pausa entre lotes, y se repasan
solos cada seis horas. Cada precio guarda su moneda, su descuento y **cuándo se
miró**: un importe sin fecha no es un dato utilizable.

El fallo que lo tenía parado no estaba en la tienda: `record_observation` exigía
que el juego estuviera en `games`, y los deseados importados de Steam viven en
`catalog_games` mientras no se compren. El primero de ellos devolvía «no está en
la biblioteca» y el error abortaba la tanda entera.

```sql
SELECT COUNT(DISTINCT app_id) FROM game_prices;   -- medido: 1105, antes 153
```

## 5. Ofertas

Dos secciones distintas, y la diferencia importa:

| Dónde | Qué enseña |
|---|---|
| Deseados | Lo rebajado **de tu lista**, con tu precio objetivo al lado |
| Seguimiento · «Ofertas para ti» | Lo rebajado en la tienda que **no** es tuyo, puntuado contra tu modelo de gustos |

Lo segundo sale de `featuredcategories` —sin sesión ni clave— y de una ficha por
juego para conocer sus géneros. Lo que ya tienes o ya deseas no aparece: lo
primero no es una oferta y lo segundo ya tiene su sección.

```sql
SELECT COUNT(*), COUNT(match_score) FROM store_deals;  -- medido: 9 | 9
```

## 6. Juegos gratis de Epic

`freeGamesPromotions` es público: ni clave ni sesión. Se guarda lo vigente y lo
anunciado, se cruza con la biblioteca para decir **si ya lo tienes** y se avisa
una sola vez cuando empieza la promoción.

Vindexa no lo reclama por ti: conducir tu sesión por un flujo de compra es otra
cosa distinta de avisar. Lo que hace es llevarte a la ficha exacta en el
navegador integrado, donde ya estás identificado.

## 7. Vista rápida al pasar el ratón

Al detenerse sobre un juego —biblioteca, deseados, ofertas, lanzamientos,
colecciones, plan y radar— aparece un emergente con sus capturas pasando y lo
que Vindexa sabe y la tienda no: el estado, lo jugado, el precio, el carril.

Las capturas se piden una vez por juego (medido: 588 bytes para diez) y quedan
guardadas, también para los mil trescientos deseados que no están en la
biblioteca. Un juego sin capturas queda marcado como preguntado para no repetir
la consulta en cada pasada del ratón.

```
pnpm test src/test/context-menu-coverage.test.tsx
```

La cobertura se vigila con una prueba: una lista nueva sin vista rápida falla.

## 8. Persistencia del arte

Las imágenes se guardan en disco con su fila en `image_cache`, se completan
solas en los ratos de reposo y **no tienen techo**: el arte de una biblioteca
local cabe entero en el disco de quien la usa. Lo único que lo frena es no
comerse el espacio libre que el sistema necesita.

```sql
SELECT COUNT(*) FROM image_cache;   -- medido: 2298
```
```
du -sh ~/Library/Caches/io.vindexa.desktop/steam-art   # medido: 635M
```

## 9. DRM

Detección en `steam::drm` a partir de lo que publica la tienda, propagación de
lo que declara GOG, filtro propio en el panel de filtros y marca «Sin DRM» en la
lista, junto a «Instalado» y «Early Access».

**Nunca sobre la carátula**: una carátula es del juego, no de cómo se distribuye.

Lo que faltaba: la clasificación llegó **después** de que media biblioteca
estuviera enriquecida, y 1.788 juegos se quedaron sin veredicto con la evidencia
vacía. Una pasada ligera los completa en segundo plano pidiendo sólo los avisos
—`filters=basic,categories`, 8.402 bytes en vez de 17.118—.

Y un fallo que sólo se ve contra la tienda de verdad: los avisos llegan en el
idioma que se pide, y la lista de marcas reconocía dos de cada cuatro cuentas de
terceros en español. Ahora la señal es que el campo `ext_user_account_notice`
**exista**, que es lo que significa.

Hay acceso directo en la barra lateral, con su recuento, y una prueba de que esa
cifra es exactamente lo que sale al pulsarla.

```sql
SELECT drm_state, COUNT(*) FROM games WHERE drm_state <> 'unknown' GROUP BY drm_state;
-- medido: drm_free|393 · third_party_drm|9, y subiendo
```

## 10. Clic derecho

Cancelar el menú nativo de WebKit es una decisión de toda la aplicación, así que
ofrecer uno propio también lo es. Está en todas las pantallas:

| Sobre qué | Qué ofrece |
|---|---|
| Un juego, en cualquier pantalla | Jugar o instalar, ficha, tienda, estado, prioridad, colecciones, fijar, seguir, copiar |
| Una colección (barra lateral y pantalla) | Color e icono en rejilla (42 iconos, 12 colores), editar, eliminar con confirmación |
| Un juego dentro de una colección | Lo de un juego, más «Quitar de …» — sólo en las manuales |
| Un estado | Color en rejilla, editor de estados |
| Una tienda | Abrir su navegador integrado con tu sesión, sincronizar ahora |
| Las cabeceras «ESTADOS» y «COLECCIONES» | Plegar, editar estados, nueva colección |
| Una vista guardada | Aplicar o quitar, anclar, actualizar con lo que ves, eliminar |
| Una tarjeta del planificador | Ficha, editar planificación, mover a otra columna, copiar, quitar del plan |
| Una columna del planificador | Color, límite de trabajo, editar columnas |
| Un deseado | Editar nota y precio, abrir la tienda, mover de carril, subir, bajar, quitar |
| Una lista curada | Editar, subir, bajar, eliminar |
| Un juego de una lista curada | Editar la nota, destacar, quitar de la lista |
| Un juego en seguimiento | Lo de un juego, más «Recordármelo en una semana» |
| Un próximo lanzamiento | Me interesa, no me interesa, programar un aviso, descartar |
| Una carátula del modo salón | Jugar o instalar, abrir la tienda, copiar título |
| Un aviso | Marcar como leído, copiar, descartar |

Dos cosas aparecieron por el camino y no estaban en ninguna parte de la
interfaz: **quitar un juego del planificador** —`remove_planner_item` existía en
el backend y no lo llamaba nadie— y **abrir la tienda de un deseado**.

```
pnpm test src/test/context-menu-coverage.test.tsx
```

La cobertura se vigila con una prueba: si una pantalla nueva no monta menú, o
una lo pierde, falla.

## 11. La ventana

Abre ocupando el hueco real de la pantalla —descontando barra de menús y Dock—,
y el doble clic en la parte vacía de la barra hace lo mismo; repetido, devuelve
la ventana a como estaba.

Lo hace Vindexa y no el sistema a propósito: en macOS el gesto nativo es el
*zoom*, que lleva la ventana al tamaño preferido de la aplicación y no al que
cabe. Medido: 1440×870 sobre un área útil de 1512×982. El arrastre sigue siendo
de Tauri, que ya lo hace bien.

`src/features/shell/window-chrome.ts`, con pruebas de qué cuenta como «parte
vacía» —un botón o un campo no cuentan—.

## 12. Agentes

Dos caminos, el mismo puente y las mismas garantías. Está contado entero en
[`HERMES.md`](HERMES.md).

- **Desde fuera**: `vindexa mcp` levanta un servidor MCP por tubería, sin
  puertos. Diecinueve herramientas. Vindexa se da de alta sola en los agentes
  que encuentre y repara el alta si cambia de sitio.
- **Desde dentro**: icono en el pie, contra un modelo que ya esté sirviendo en
  local. Se elige modelo, se le puede dictar y se le pueden dejar encargos que
  repite solos.

```
printf '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}\n{"jsonrpc":"2.0","id":2,"method":"tools/list"}\n' \
  | /Applications/Vindexa.app/Contents/MacOS/vindexa mcp | tail -1 | python3 -c "import sys,json;print(len(json.load(sys.stdin)['result']['tools']),'herramientas')"
```

Comprobado de punta a punta contra el Hermes de esta casa: registra una sesión
en la biblioteca real, la deshace, y contesta con los estados de verdad.

### Telegram

Lo pone Hermes, no Vindexa. Un perfil dedicado (`vindexabot`) aparece como bot
en Bot Mode, lleva el servidor MCP y tiene su propia sesión de Telegram. Meter
aquí un segundo cliente de Telegram sería mantener dos clientes, dos sesiones y
dos sitios donde configurar lo mismo.

```
vindexabot gateway status     # supervisado por launchd
vindexabot mcp list           # vindexa · all · enabled
```

Al clonar un perfil hay una trampa que cuesta ver: **se copia el testigo de
Telegram del original**, y entonces los dos bots se pelean por el mismo. Hay que
vaciarlo antes de nada.

---

## Lo que no está, y por qué

- **Un cliente de Telegram dentro de Vindexa.** Ver arriba.
- **Más movimiento en las tarjetas que se arrastran.** Ver el punto 1.
- **Descargar modelos desde Vindexa.** Se proponen los que caben, preguntando a
  Hugging Face en el momento, y se enseña la orden exacta que instalaría
  llama.cpp; ejecutarla es un botón. Instalar un paquete toca el sistema entero,
  no sólo esta aplicación.
