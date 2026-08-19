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

## 4. Persistencia del arte

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

## 5. DRM

Detección en `steam::drm` a partir de lo que publica la tienda, propagación de
lo que declara GOG, filtro propio en el panel de filtros y marca «Sin DRM» en la
lista, junto a «Instalado» y «Early Access».

**Nunca sobre la carátula**: una carátula es del juego, no de cómo se distribuye.

```sql
SELECT drm_state, COUNT(*) FROM games WHERE drm_state <> 'unknown' GROUP BY drm_state;
-- medido: drm_free|200 · third_party_drm|3
```

## 6. Clic derecho

Cuatro menús contextuales:

| Sobre qué | Qué ofrece |
|---|---|
| Un juego | Jugar, instalar, ficha, tienda, fijar, seguir, estado, prioridad, colecciones, copiar |
| Una colección | Color e icono en rejilla (42 iconos, 12 colores), editar, eliminar |
| Un estado | Color en rejilla, editor de estados |
| Una tienda | Abrir su navegador integrado con tu sesión, sincronizar ahora |

```
grep -n "export function.*ContextMenu" src/features/library/SidebarContextMenus.tsx
```

## 7. La ventana

Abre ocupando el hueco real de la pantalla —descontando barra de menús y Dock—,
y el doble clic en la parte vacía de la barra hace lo mismo; repetido, devuelve
la ventana a como estaba.

Lo hace Vindexa y no el sistema a propósito: en macOS el gesto nativo es el
*zoom*, que lleva la ventana al tamaño preferido de la aplicación y no al que
cabe. Medido: 1440×870 sobre un área útil de 1512×982. El arrastre sigue siendo
de Tauri, que ya lo hace bien.

`src/features/shell/window-chrome.ts`, con pruebas de qué cuenta como «parte
vacía» —un botón o un campo no cuentan—.

## 8. Agentes

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
