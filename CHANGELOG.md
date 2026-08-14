# Registro de cambios

Este archivo sigue [Keep a Changelog](https://keepachangelog.com/es-ES/1.1.0/) y el
proyecto usa [versionado semántico](https://semver.org/lang/es/). Una sección describe el
estado del código, no una publicación, mientras figure como **Sin publicar**.

## [Sin publicar]

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

## [0.1.0]

Reservado para la primera publicación validada. No existe todavía una release pública
certificada desde este repositorio.
