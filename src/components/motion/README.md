# Microinteracciones de Vindexa

Librería interna de movimiento. Inspirada en el catálogo de
[SmoothUI](https://smoothui.dev/docs/components/), pero reescrita para la
estética de Vindexa: densa, técnica, oscura, radios de 0 a 3 px y sin rebote.
De SmoothUI se han tomado **patrones**, nunca código ni estética: su catálogo
está afinado a `spring` con sobreoscilación y a radios de shadcn, justo lo
contrario de lo que pide `DESIGN.md`.

## Reglas que cumple todo lo de esta carpeta

- Duraciones de **120–260 ms**. La única excepción es `ExpandableSection`, con
  un techo de 300 ms por ser un desplegable de altura medida.
- Curvas: `--ease-out` y `--ease-in-out` de `src/index.css`. Ningún
  `cubic-bezier` suelto — hay una prueba que lo verifica.
- Muelles **sobreamortiguados**: `damping` por encima de la amortiguación
  crítica, así que llegan y paran. También hay prueba.
- Solo `transform` y `opacity` en animaciones continuas.
- `prefers-reduced-motion: reduce` **desactiva** el movimiento, no lo atenúa.
  Se comprueba en JavaScript con `useReducedMotion` y, por partida doble, en
  `motion.css`.
- Nada altera la caja de su contenedor, de modo que todo puede vivir dentro de
  una lista de `@tanstack/react-virtual` sin romper la medición de altura.

## Cuándo usar cada componente

### `AnimatedNumber`

Cifras que cambian y conviene ver moverse: número de juegos de una colección,
minutos jugados, tamaño en disco, contadores de la barra de estado.

```tsx
<AnimatedNumber value={library.length} />
<AnimatedNumber value={minutes} format={formatPlaytime} />
```

El ancho lo reserva un doble oculto con el valor final, así que la caja adopta
su tamaño definitivo en el primer fotograma y nada se desplaza mientras cuenta.
**No** lo uses para un valor que el usuario está leyendo y editando a la vez —
ahí el número debe cambiar de golpe.

### `RevealOnScroll` y `StaggerList`

Aparición al entrar en el viewport, de 4 px y 200 ms. `RevealOnScroll` es para
un bloque; `StaggerList` escalona un grupo corto con un techo de 160 ms.

```tsx
<RevealOnScroll asChild>
  <article className="planner-card">…</article>
</RevealOnScroll>

<StaggerList as="ul" className="sidebar-list" itemAsChild>
  {collections.map((c) => <li key={c.id}>…</li>)}
</StaggerList>
```

`asChild` aplica la aparición al propio hijo, sin añadir un nodo al DOM: es la
forma segura de usarlo dentro de una fila virtualizada.

**No uses `StaggerList` en una rejilla virtualizada**: al reciclar filas
volvería a animarlas en cada desplazamiento. Ahí pon un único `RevealOnScroll`
en el contenedor, o desactívalo con `disabled`.

### `PressableSurface`

Superficies pulsables que deben acusar el tacto: tarjetas de juego, botones de
acción de la ficha, filas de la barra lateral. Sube 1 px al pasar el puntero y
se hunde al pulsar; los máximos están recortados a 4 px y a una escala de 1.02.

```tsx
<PressableSurface asChild>
  <button className="game-card__target">…</button>
</PressableSurface>
```

No hay `MagneticButton`: en una lista densa, un elemento que persigue al cursor
convierte un clic en un ejercicio de puntería.

### `ShimmerSkeleton`

Estados de carga por encima de 300 ms, con la geometría real del contenido que
va a llegar.

```tsx
<ShimmerSkeleton aspectRatio="2 / 3" width={142} />
<ShimmerSkeleton count={8} height={34} gapPx={1} label="Cargando la biblioteca" />
```

Sin `label` es decorativo y queda fuera del árbol de accesibilidad, que es lo
correcto cuando ya hay un `role="status"` cerca. Con `label` se anuncia él
mismo.

### `SegmentedControl`

Conmutar **vista, orden o densidad**: opciones excluyentes sin panel asociado.
El indicador se desliza con `layoutId`.

```tsx
<SegmentedControl
  label="Vista de la biblioteca"
  options={[
    { value: "cuadricula", label: "Cuadrícula" },
    { value: "lista", label: "Lista" },
  ]}
  value={view}
  onValueChange={setView}
/>
```

Cada opción es un `input[type=radio]` nativo dentro de su etiqueta: el estado
marcado, el nombre accesible y el orden de tabulación los da el navegador. Las
flechas, `Inicio` y `Fin` se manejan de forma explícita para que el
comportamiento sea idéntico en todos los motores.

Para pestañas **con paneles** usa `@/components/ui/tabs`, que ya está montado
sobre Radix: duplicarlo aquí sería crear un segundo lenguaje de pestañas.

### `ExpandableSection`

Secciones plegables con altura medida: grupos de la barra lateral, bloques de
la ficha de juego, ajustes avanzados.

```tsx
<ExpandableSection title="Sesiones" headerExtra={<span>{sessions.length}</span>}>
  <PersonalJournal … />
</ExpandableSection>
```

- Por defecto el contenido se **desmonta** al cerrarse, para que nunca quede un
  control enfocable dentro de una caja de altura cero.
- Con `keepMounted` se conserva montado e inerte: úsalo cuando dentro haya un
  formulario con estado.
- `onHeightChange` entrega la altura medida para llamar a `virtualizer.measure()`
  cuando la sección vive en una fila de altura dinámica.

### `ToastStack`

Avisos de operaciones que terminan fuera de la vista: sincronización de Steam,
movimientos por lotes, importaciones. Es **controlado**: recibe la lista y
notifica el descarte, sin almacén global escondido.

```tsx
<ToastStack toasts={toasts} onDismiss={dismiss} />
```

Los errores se anuncian como `alert` y **no se cierran solos**; el resto son
`status` y se retiran a los cinco segundos. Para un aviso en línea junto al
control que lo provoca no uses esto: ya existen `.inline-notice` y
`.detail-action-notice` en `src/index.css`.

### `DragFeedbackSurface`

Zonas de destino de arrastre. Habla el mismo idioma que la barra lateral de la
biblioteca: cian para «admite lo que arrastras», lima para «suelta aquí», rojo
con trama para «aquí no».

```tsx
<DragFeedbackSurface state={dropState} count={selection.size} hint="Mover a Pendientes">
  …
</DragFeedbackSurface>
```

No emite anuncios de accesibilidad: de eso se encarga `@dnd-kit` con sus
`announcements`, y duplicarlos haría hablar dos veces al lector.

### `CopyableValue` e `IconMorph`

`CopyableValue` para identificadores y rutas que se copian a menudo: AppID,
ruta de instalación, ruta de la base de datos.

```tsx
<CopyableValue value={String(game.appId)} />
```

`IconMorph` es la pieza que hay debajo: cambia un icono por una confirmación en
120 ms dentro de una caja fija. Úsala para cualquier acción que termine en un
«hecho» visible.

Ambos son puramente visuales para el ojo; la confirmación se anuncia además en
una región viva, porque un cambio de icono no lo percibe un lector de pantalla.

## Preferencia de movimiento

`useReducedMotion()` es la única fuente de verdad. Devuelve `true` cuando hay
que **suprimir** el movimiento.

```tsx
const reduced = useReducedMotion();
```

`MotionPreferencesProvider` permite que un ajuste de la aplicación mande sobre
la preferencia del sistema, sin tocar ningún componente:

```tsx
<MotionPreferencesProvider reduceMotion={settings.reduceMotion ? true : "auto"}>
  <App />
</MotionPreferencesProvider>
```

## Tokens

`motion-tokens.ts` exporta `DURATION`, `DURATION_MS`, `EASE_OUT`,
`EASE_IN_OUT`, `SPRING_SNAP`, `SPRING_STACK`, las transiciones ya compuestas y
`withReducedMotion(transition, reduced)`. Si necesitas animar algo a mano,
parte de ahí en vez de escribir duraciones nuevas.

## Notas de integración

- Cada componente importa `./motion.css`; el empaquetador lo deduplica. Los
  estilos usan la clase global `.sr-only` de `src/index.css`, que la aplicación
  siempre carga.
- Los estilos viven en clases `.vx-*` para no colisionar con nada existente, y
  no declaran ningún color, radio ni curva propios: todo sale de los tokens.
- Los componentes exponen `data-slot` como el resto de primitivas del
  repositorio, de modo que se pueden seleccionar en pruebas y estilos sin
  depender de las clases.
