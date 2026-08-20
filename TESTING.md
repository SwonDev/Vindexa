# Pruebas de Vindexa

La calidad de Vindexa se evalúa en cuatro capas: contratos puros de frontend, dominio y
persistencia Rust, ejecución Tauri real y gates manuales dependientes de Steam o del sistema
operativo. Que una sola capa esté verde no certifica la aplicación completa.

> [!IMPORTANT]
> Las fixtures sintéticas solo existen en procesos de prueba y bases SQLite temporales. La
> aplicación de producción no inserta juegos demo ni datos simulados.

## Índice

- [Comandos principales](#comandos-principales)
- [Cobertura automatizada](#cobertura-automatizada)
- [Matriz manual de escritorio](#matriz-manual-de-escritorio)
- [Persistencia y restauración](#persistencia-y-restauración)
- [Steam y privacidad](#steam-y-privacidad)
- [Rendimiento y biblioteca grande](#rendimiento-y-biblioteca-grande)
- [Pruebas visuales y accesibilidad](#pruebas-visuales-y-accesibilidad)
- [Gate de release](#gate-de-release)

## Comandos principales

Instala primero con el lockfile:

```bash
corepack enable
pnpm install --frozen-lockfile --ignore-scripts
```

Ejecuta el gate combinado:

```bash
pnpm check
pnpm test:rust
```

`pnpm check` encadena, en este orden:

```text
pnpm lint
pnpm test
pnpm build
cargo check --manifest-path src-tauri/Cargo.toml
```

Comandos aislados útiles:

| Comando | Verifica |
| --- | --- |
| `pnpm test` | Tests frontend una vez. |
| `pnpm test:watch` | Tests frontend en modo interactivo. |
| `pnpm test:rust` | Tests unitarios e integración Rust/SQLite. |
| `pnpm test:e2e` | Playwright visual/funcional con IPC determinista, Axe y baselines. |
| `pnpm test:e2e:native` | Smoke Tauri macOS aislado de proceso, ventana y SQLite temporal. |
| `pnpm test:e2e:update` | Regenera las baselines visuales de `visual-and-motion.spec.ts`; revisa el diff de imágenes antes de aceptarlo. |
| `pnpm lint` | Reglas y formato Biome sin escribir. |
| `pnpm format` | Aplica formato/acciones de Biome; revisa el diff después. |
| `pnpm build` | TypeScript estricto y bundle Vite. |
| `cargo fmt --manifest-path src-tauri/Cargo.toml --check` | Formato Rust sin modificar archivos. |
| `cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets --all-features -- -D warnings` | Lints Rust estrictos. |
| `pnpm tauri dev` | Runtime nativo, migraciones, plugins e integraciones. |
| `pnpm tauri build` | Compilación release y empaquetado nativo. |

No uses `pnpm dev` para declarar que una función nativa funciona: el WebView servido por
Vite no sustituye al runtime Tauri.

## Cobertura automatizada

### Frontend

Las pruebas de `src/test/` usan Vitest, jsdom, Testing Library y `user-event`.

| Archivo | Contrato cubierto |
| --- | --- |
| `format.test.ts` | Tiempo, tamaño, fechas e iniciales Unicode. |
| `tauri-api.test.ts` | Nombres de comandos, payloads `camelCase`, operaciones sensibles y normalización de errores. |
| `settings-dialog.test.tsx` | Bootstrap sin Keychain, verificación voluntaria, guardado + sincronización, errores persistidos y diálogos de backup sin rutas. |
| `common-states.test.tsx` | Carga anunciada, vacío accionable y recuperación del ErrorBoundary. |
| `library-toolbar.test.tsx`, `library-filters.test.ts` | Búsqueda, 17 órdenes, filtros combinables y normalización. |
| `library-sidebar.test.tsx`, `library-dnd.test.ts` | Colapso, destinos, multiselección y rechazo de colecciones inteligentes. |
| `library-session.test.ts` | Sesión persistida de búsqueda, filtros, vista y scroll. |
| `game-detail-sheet.test.tsx` | Hero/fallback, metadatos, logros, desinstalación confirmada y registro personal. |
| `app-shell.test.tsx`, `shortcuts.test.ts` | Navegación, sincronización, atajos configurables, colisiones y campos editables. |
| `planner-screen.test.tsx`, `planner-views.test.tsx`, `planner-periods.test.ts` | Kanban, cola, semana/mes, objetivos y capacidad. |
| `discovery-api.test.ts`, `discovery-screen.test.tsx` | Recordatorios, descartes y estados honestos de capacidades. |
| `wishlist-screen.test.tsx`, `wishlist-model.test.tsx`, `wishlist-curated.test.tsx`, `wishlist-videos.test.tsx` | Cubos de intención, agregado que nunca suma monedas distintas, listas curadas y vídeos guardados. |
| `collections-screen.test.tsx`, `collections-design-contract.test.ts` | Mosaicos reales, reglas legibles en la ficha, orden persistido y puerta de diseño sobre la hoja de estilo. |
| `game-detail-priority.test.tsx`, `game-dlc-panel.test.tsx` | Una sola escala de prioridad con su aritmética comprobable, y contenido adicional que no afirma ausencias. |
| `ui-primitives.test.tsx` | Nombre accesible, teclado, switch, diálogo, foco, Escape y tabs. |
| `game-browser-scale.test.tsx` | Una colección de 5.000 juegos monta solo la ventana virtual visible. |
| `artwork.test.tsx`, `density-geometry.test.ts` | Caché/fallback de arte y geometría de densidad. |
| `command-registry.test.ts` | Paridad entre cliente TypeScript y lista cerrada de comandos Rust. |

Estas pruebas no abren un proceso Tauri, no escriben en la base real y no acceden a Steam.
El bridge `invoke` se simula solo para validar el contrato TypeScript.

### Rust y SQLite

Los tests unitarios junto a los módulos cubren:

- migraciones idempotentes y creación de FTS5;
- formato y validación de SteamID64 y Web API Key;
- estructura exacta de callback y claimed ID OpenID;
- campos firmados requeridos;
- allowlist de acciones Steam;
- URLs oficiales de arte, MIME y magic bytes;
- creación, edición, eliminación y reordenación atómica de estados;
- colecciones manuales, orden y pertenencias;
- reglas inteligentes agrupadas, validación y vista previa sin mutación;
- planner, posiciones densas, WIP y reasignación al eliminar una columna;
- preferencias y frecuencias permitidas;
- atajos persistidos, permitidos y sin colisiones;
- etiquetas, fechas y sesiones con validación, paginación y actividad;
- resincronización sin pérdida de organización personal;
- cambio masivo de estado atómico, sin duplicados y limitado a 10.000 juegos;
- drag and drop transaccional y undo rechazado si el recibo quedó obsoleto;
- catálogo Steam Family separado y procedencia heredada corregida conservadoramente;
- metadatos de tienda, observaciones, recordatorios y recomendaciones descartadas;
- solicitud de desinstalación limitada a una instalación conocida;
- ventana de tienda: URL exacta, navegación restringida, reglas nativas fail-closed en macOS
  y Linux, y aislamiento común en plataformas sin filtro adicional;
- comprobación de updates deliberadamente no configurada;
- bootstrap basado en marcador no secreto y login OpenID singleflight;
- límites de OpenID (32 KiB/64 KiB), Steam Web API (32 MiB) y arte (10 MiB);
- restauración con esquema exacto, rechazo de symlinks/hard links y rollback verificado.

Las integraciones de `src-tauri/tests/` añaden:

| Archivo | Contrato cubierto |
| --- | --- |
| `models_contract.rs` | Serialización exacta `camelCase` para filtros, edición, planner y recomendación. |
| `persistence_contract.rs` | Constraints, cascadas, FTS5 y roundtrip de backup con datos personales. |
| `large_library.rs` | Importación, paginación, filtros y búsqueda sobre 5.000 juegos. |
| `advanced_filters.rs` | Filtros combinados en SQL y semántica de datos desconocidos. |
| `library_sorting.rs` | Las 17 ordenaciones, desempates y paginación estable. |
| `legacy_ownership.rs` | Upgrade de procedencia heredada y promoción tras `GetOwnedGames`. |

Los tests remotos no envían una clave real. El parser de `GetOwnedGames` usa una respuesta
JSON embebida para validar deserialización y mapeo; la red oficial requiere el gate manual.

## Matriz manual de escritorio

Ejecuta:

```bash
pnpm tauri dev
```

Parte de una base de prueba separada o de un perfil sin datos importantes. No sustituyas la
base personal del usuario para ejecutar este checklist.

| Caso | Procedimiento | Resultado esperado |
| --- | --- | --- |
| Primera apertura | Iniciar sin base previa. | Ventana usable, estados/columnas creados y biblioteca vacía sin datos demo. |
| Importación local | Ajustes → Steam → Explorar bibliotecas locales. | Cuenta real de bibliotecas/manifiestos; juegos instalados, procedencia y rutas coherentes. |
| Búsqueda | Escribir título, nota y checkpoint. | Debounce breve, total correcto y resultados sin bloqueo. |
| Filtros/orden | Combinar rangos, tags, fechas y procedencia; recorrer las 17 órdenes. | Consulta estable, contador/chips coherentes y limpieza reversible. |
| Vista | Cambiar cuadrícula/lista y desplazarse. | No hay salto de geometría; solo se monta una ventana virtual. |
| Ficha | Hacer scroll, editar plan, cargar metadatos/logros y reabrir. | Parallax sin solape, estados honestos y valores persistidos. |
| Registro | Crear/editar tag, fechas y sesión; cargar más historial. | Validación visible, actividad y lectura idéntica al reabrir. |
| Selección múltiple | Seleccionar varios juegos y cambiar estado. | Una única transacción cambia todos o ninguno; la selección se limpia tras éxito. |
| DnD biblioteca | Abrir ficha por clic/Enter/Espacio; después arrastrar desde el asa por puntero y teclado, soltar lote sobre estado/manual, deshacer y probar smart. | La apertura ocurre una vez y no inicia DnD; destinos claros, smart prohibida, undo exacto o rechazo si quedó obsoleto. |
| Planner | Mover tarjeta dentro y entre columnas con ratón y teclado. | Destino visible, orden persistente y WIP validado por Rust. |
| Vistas planner | Cola, semana y mes; cambiar objetivo/capacidad. | Orden/periodos/carga se conservan sin desbordamiento. |
| Colección manual | Crear, editar identidad, reordenar y eliminar una colección. | Identidad/orden persisten; membresía, juegos y datos personales permanecen al editar o eliminar. |
| Colección inteligente | Añadir/editar reglas, calcular preview y guardar; simular error al cargar reglas existentes. | Preview y contador coinciden; no se muta al previsualizar y el error bloquea guardar hasta reintentar. |
| Deseados | `⌘5`; mover entradas entre los cuatro cubos, anotar precio objetivo en dos monedas y dejar una entrada sin precio. | Las monedas nunca se suman entre sí y la cifra se anuncia como «al menos»; la salvedad completa se lee al pasar el cursor por encima. |
| Contenido adicional | Ficha → Contenido adicional; actualizar desde la tienda y corregir a mano propiedad, instalación y ocultación. | «Sin confirmar» jamás se escribe como ausente; el importe pendiente conserva su «al menos» y la marca manual gana a la comprobación. |
| Organización | Editar/reordenar estados y borrar uno personal usado; reordenar/borrar una columna con juegos. | Built-in no se elimina; estado reasigna a Sin clasificar y columna al destino indicado sin perder datos. |
| Descubrimiento | Recomendar, gestionar recordatorios, actualizar feed y revisar relaciones. | Feed/fecha visibles, caché y backoff persistentes; relaciones solo por empresa/fecha reales y criterio explícito, con la procedencia del método al pasar el cursor por el bloque. |
| Steam Family | Sincronizar con grupo detectable y abrir catálogo. | Unknown/confirmed separados, sin identidad/tiempo de terceros. |
| Steam | Abrir tienda, instalar y jugar. | Rust construye solo la ruta permitida; tienda sin IPC y host limitado. En macOS/Linux, un blocker fallido cierra antes de navegar. |
| Desinstalación | Solicitar para instalado con confirmación activa/inactiva. | Solo abre Steam; juego no instalado se rechaza y UI no afirma borrado. |
| Instalación | Revelar carpeta de un juego instalado. | Solo abre una carpeta existente bajo una biblioteca Steam detectada. |
| Atajos | Ajustes → Atajos: reasignar, provocar colisión, pulsar **Restablecer atajos** y escribir en un input. | Persisten, colisión se rechaza, `⌘,` queda reservado y el input no dispara acciones. |
| Updates | Ajustes → Acerca de → Buscar actualizaciones. | `notConfigured`; ninguna red, descarga o instalación. |
| Diagnóstico | Abrir Datos y copias. | Integridad `ok`, versión de esquema, tamaño, ruta y WAL visibles. |

Para cada fallo, conserva el código y mensaje de UI. No adjuntes la base, el backup ni la
Web API Key.

## Persistencia y restauración

### Reinicio

1. Importa un juego real.
2. Cambia estado y progreso.
3. Escribe un checkpoint Unicode reconocible.
4. Añádelo al planner y a una colección.
5. Cierra Vindexa desde el sistema, no solo la ficha.
6. Abre de nuevo.

El estado, progreso, texto, columna, posición y pertenencia deben ser idénticos. Confirma
también que la base sigue en la misma ruta.

### Resincronización

1. Registra estado, progreso, rating, nota, colección, etiqueta, fechas y sesión de un título.
2. Ejecuta una importación local.
3. Ejecuta una sincronización Web API.
4. Reabre la ficha.

Steam puede actualizar título, arte y tiempo. Ningún campo personal, etiqueta, fecha o
sesión debe cambiar.

### Exportación/restauración

1. Confirma `integrity = ok` en Diagnóstico.
2. Exporta a una ruta distinta de la base activa.
3. Modifica una nota de control.
4. Restaura el backup exportado.
5. Verifica que reaparece el valor anterior.
6. Confirma que la UI comunica éxito sin recibir ni mostrar la ruta seleccionada.
7. Cierra y abre para demostrar que la restauración persiste.

Repite con cancelación de ambos diálogos, un archivo no SQLite, una base con esquema
incompleto, la base activa, un sidecar WAL/SHM/journal y un enlace simbólico: deben cancelar
o rechazarse sin sustituir la base activa. Durante la restauración intenta iniciar una
consulta o sincronización y confirma que queda serializada hasta terminar.

## Steam y privacidad

### OpenID real

El acceso real necesita acción humana en el navegador oficial:

1. pulsa **Continuar con Steam**;
2. confirma que la URL pertenece a `steamcommunity.com`;
3. completa Steam Guard si Steam lo solicita;
4. verifica la página local de éxito y la identidad en Vindexa;
5. repite dejando pasar más de 180 segundos: debe expirar sin vincular;
6. cancela/cierra el navegador: la app debe recuperar el control con un error comprensible.

Nunca automatices la contraseña ni guardes cookies de Steam como fixture.

### Keychain o servicio de secretos

1. inicia sin marcador y confirma que `bootstrap` no muestra ningún aviso del almacén
   seguro;
2. pulsa **Comprobar clave guardada**, autoriza esa lectura explícita y comprueba que el
   marcador se actualiza;
3. cierra y abre la misma build sin recompilar: no debe volver a consultar Keychain;
4. guarda una clave de prueba con forma válida y una cuenta vinculada: debe ejecutarse una
   sola sincronización y el input debe quedar vacío;
5. confirma que la UI solo muestra «configurada» y nunca repuebla el valor;
6. exporta SQLite y comprueba localmente que la clave no aparece, aunque sí exista el
   marcador no secreto;
7. elimina la clave desde la UI y confirma que una sincronización posterior informa que
   falta.

No imprimas el valor durante la prueba.

### Biblioteca privada y rate limit

Estos estados dependen de Steam. Verifica que `401/403`, `429`, respuestas sin `games`,
timeout, tipo MIME incorrecto, más de 32 MiB y JSON inválido se convierten en mensajes
específicos. Una sincronización fallida debe persistir `last_sync_status = failed`, código y
mensaje seguro sin borrar la biblioteca; el error debe reaparecer tras reiniciar y limpiarse
solo después de un éxito.

## Rendimiento y biblioteca grande

`large_library.rs` crea 5.000 juegos en SQLite temporal y exige:

- importación completa en menos de 10 segundos;
- paginación, filtros y búsqueda del conjunto en menos de 3 segundos;
- páginas de 120 sin solapamiento;
- conteo exacto de instalados;
- resultado exacto para una búsqueda concreta.

Ejecuta el test con salida visible:

```bash
cargo test --manifest-path src-tauri/Cargo.toml \
  --test large_library -- --nocapture
```

Los presupuestos son guardarraíles amplios para CI y no sustituyen una medición de la app
release. En una biblioteca real de al menos 5.000 juegos, registra además:

- tiempo de apertura hasta contenido útil;
- duración del primer y siguiente page fetch;
- memoria después de recorrer cuadrícula y lista;
- fluidez del scroll y cantidad de nodos montados;
- duración del import local y de la resincronización;
- tamaño de SQLite y caché.

Evidencia local del 14 de agosto de 2026 en el Mac de desarrollo, ejecutada en perfil de
test/debug y SQLite en memoria: 5.000 upserts en `480 ms` y el bloque de paginación,
filtros y búsqueda en `30 ms`. La medición prueba el contrato del repositorio en ese host;
no predice por sí sola el rendimiento de WebKitGTK o del almacenamiento de una Bazzite real.

### Escala de la interfaz

`tests/e2e/scale.spec.ts` sirve una biblioteca de **1.500 juegos** al frontal a través del
arnés IPC y comprueba lo único que una captura con cuarenta títulos no puede demostrar:

- cuántas fichas monta el virtualizador (debe quedar por debajo de 60, no en 1.500);
- que saltar al fondo de la lista no acumula nodos;
- que agrupar por inicial sigue respondiendo;
- que no aparece desbordamiento horizontal.

```bash
pnpm exec playwright test scale.spec.ts --reporter=line
```

Evidencia local del 18 de agosto de 2026, Mac de desarrollo, servidor de desarrollo de Vite
(no compilación de release):

| Medida | Resultado |
| --- | --- |
| Primera pantalla útil | 641 ms |
| Fichas montadas al abrir | 30 |
| Salto al fondo de 1.500 juegos | 334 ms |
| Fichas montadas tras el salto | 48 |
| Agrupar por inicial | 106 ms |

Los presupuestos del test son deliberadamente amplios (4 s y 2 s): están para detectar una
regresión de orden de magnitud, no para medir la máquina.

## Pruebas visuales y accesibilidad

Valida la ventana Tauri, no solo el servidor Vite, en al menos:

| Tamaño | Objetivo |
| --- | --- |
| 960 × 680 | Mínimo configurado; sin controles inaccesibles ni solapamientos. |
| 1440 × 900 | Tamaño de diseño principal. |
| 1920 × 1080 | Uso eficiente sin tarjetas sobredimensionadas. |
| Ultrapanorámico | Densidad estable y anchos de lectura controlados. |

En cada tamaño captura Biblioteca vacía/cargada, ficha, Planificador, Deseados, Colecciones,
Seguimiento y cada sección de Ajustes. Compara con `DESIGN.md` y la referencia del brief:
densidad, jerarquía, tokens, foco, hover, selección, loading, error y vacío.

Checklist de accesibilidad:

- recorrer todas las acciones con `Tab`, `Shift+Tab`, Enter, Espacio y Escape;
- búsqueda mediante `⌘F`/`Ctrl+F`, paleta de comandos mediante `⌘K`/`Ctrl+K` y ajustes
  mediante `⌘,`/`Ctrl+,`, que es la única combinación reservada;
- navegación por secciones con `⌘1`–`⌘5` (Biblioteca, Planificador, Colecciones, Seguimiento
  y Deseados) y con el resto de atajos configurados; una base heredada que todavía guarde
  `Mod+K` como búsqueda debe migrarse sola a `⌘F` al leerla, o la paleta no abriría nunca;
- foco visible sobre fondos oscuros;
- nombre accesible para botones de icono, artwork y progreso;
- diálogo con foco contenido y retorno al trigger;
- drag de biblioteca y planner con `KeyboardSensor` y anuncio comprensible;
- zoom/texto del sistema sin truncar acciones críticas;
- `prefers-reduced-motion: reduce` sin transformaciones o transiciones no esenciales;
- contraste WCAG 2.2 AA y ningún estado comunicado solo por color.

`pnpm test:e2e` ejecuta el arnés Playwright determinista con IPC Tauri simulado y conserva
baselines en `tests/e2e/__screenshots__/` para 960 × 700, 1440 × 900 y 1920 × 900. Cubre
arranque, carga/error/recuperación, persistencia de interfaz, DnD con deshacer, reinicio,
colecciones, el ámbito de Steam Family a 960 × 700, `prefers-reduced-motion`, integridad de
maquetación y Axe sin incidencias `serious`/`critical`. Sin reintentos: `retries: 0` y un único
worker.

> [!IMPORTANT]
> **Esta suite no corre en el CI y por eso hay que ejecutarla a mano.** Sus baselines son
> capturas de macOS: en otro sistema el texto se rasteriza distinto y no coincidirían. El CI
> ejecuta tipos, estilo, Vitest, `vite build` y todo lo de Rust.
>
> Se comprobó el 20/08/2026 que llevaba tiempo en rojo sin que nadie lo notara: doce
> escenarios fallaban contra una interfaz que ya no existía —la barra de estado que se quitó,
> el conmutador de vista que pasó a ser un grupo de radios, el catálogo familiar que se fundió
> con la biblioteca—. Una suite que no se ejecuta deja de describir la aplicación.

El puerto del servidor de pruebas es el 4173. Si está ocupado —un `vite preview` olvidado, por
ejemplo—, `VINDEXA_E2E_PORT=4271 pnpm test:e2e` usa otro en vez de obligar a matar procesos
ajenos.

`showcase.spec.ts` vive en el mismo directorio, así que `pnpm test:e2e` también lo arranca.
No compara nada: escribe las capturas de `artifacts/showcase/` y **necesita red** para
descargar el arte oficial, de modo que sin conexión falla sin que la regresión esté rota.
Para ejecutar solo la regresión, `pnpm exec playwright test --grep-invert vitrina`.

El número exacto de escenarios cambia con cada tanda, así que no se copia aquí: se cuenta
con `pnpm exec playwright test --list` (y con `--grep-invert vitrina` para la regresión
sola). En el momento de escribir esto eran 31 en total, 19 de regresión y 12 de vitrina.
`pnpm test:e2e:native` añade en macOS un smoke aislado
del binario Tauri: usa el identificador de pruebas `io.vindexa.desktop.e2e`, verifica proceso,
ventana 960 × 700 y creación de SQLite bajo un `HOME` temporal, cierra el proceso y elimina
el perfil. No automatiza el DOM de WKWebView ni incorpora plugins WebDriver test-only; OpenID,
Web API, Keychain, `steam://` y WebKitGTK siguen necesitando los gates nativos autorizados.

## Gate de release

Antes de considerar una versión distribuible deben pasar todos estos bloques:

1. `pnpm check` sin warnings ni fallos.
2. `pnpm test:rust` sin tests ignorados inesperadamente.
3. `cargo fmt --check` y Clippy estricto.
4. `pnpm tauri build` en cada sistema objetivo.
5. Smoke test del artefacto generado, no solo de `tauri dev`.
6. Persistencia después de reinicio.
7. OpenID, Web API, Keychain y manifiestos con datos reales autorizados.
8. Backup y restauración con copia de seguridad previa verificada.
9. Comparación visual y revisión de teclado/reduced motion.
10. `pnpm audit:dependencies`; cualquier advisory transitorio debe documentarse y
    evaluarse por plataforma y alcance, no ignorarse por un exit code global.
11. Capability mínima y CSP comprobadas sobre el bundle generado.
12. Checksum del artefacto final.
13. Documentación de producto completa según el brief: seguridad, contribución, manual de
    usuario, esquema, ADR, changelog y licencia, además de las guías ya presentes.
14. En Bazzite: AppImage/RPM, WebKitGTK, secreto, Steam y persistencia verificados en una
    máquina real.

Si un gate no se ejecutó, la entrega debe indicar **no verificado**; compilar en macOS no
permite inferir el resultado de Bazzite.
