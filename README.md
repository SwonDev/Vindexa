# Vindexa

Vindexa es una aplicación de escritorio local-first para importar, ordenar, planificar y
seguir una biblioteca personal de Steam sin entregar a Steam la organización privada del
usuario.

> [!IMPORTANT]
> Vindexa está en versión `0.1.0` y todavía no es una distribución firmada con Developer ID
> ni notarizada. El bundle debug usa una firma ad hoc verificable para evaluación local; la
> instalación y la
> persistencia en Bazzite deben verificarse en una máquina Bazzite real antes de declarar
> soporte de producción.

## Inicio rápido

Requisitos: Node.js con Corepack, `pnpm`, Rust estable y las dependencias de sistema de
[Tauri 2](https://v2.tauri.app/start/prerequisites/) para el sistema anfitrión.

```bash
corepack enable
pnpm install --frozen-lockfile --ignore-scripts
pnpm tauri dev
```

La primera apertura crea la base SQLite y sus migraciones. Para cargar juegos reales, usa
una de estas rutas:

1. Abre **Ajustes → Steam → Explorar bibliotecas locales** para leer los manifiestos de la
   instalación local de Steam. No requiere cuenta ni Web API Key.
2. Pulsa **Continuar con Steam** para vincular el SteamID64 mediante el navegador oficial.
3. Si todavía no tienes una clave, usa **Obtener una Web API Key** para abrir la página
   oficial de Steam. Al vincular una cuenta, **Guardar y sincronizar** conserva la clave y
   ejecuta inmediatamente una sincronización; también puedes usar **Sincronizar ahora**.

Vindexa no incluye juegos de demostración ni portadas falsas. Un estado vacío indica que
todavía no se ha importado una biblioteca real.

## Qué incluye la aplicación

- Biblioteca paginada y virtualizada en cuadrícula o lista, con búsqueda FTS5, filtros
  combinables y 17 órdenes similares a Steam, persistentes.
- Catálogo separado de Steam Family, con disponibilidad por confirmar o confirmada
  localmente y sin conservar identidad de otros miembros.
- Ficha inmersiva con hero oficial, parallax desactivable por accesibilidad, descripción,
  metadatos enriquecidos progresivamente, logros explícitos y feedback recuperable.
- Cola persistente de metadatos que prioriza juegos visibles e instalados, limita la red,
  deduplica trabajos y respeta TTL, `Retry-After` y backoff sin frenar la biblioteca.
- Organización personal con estado, progreso, prioridad, valoración, fechas, etiquetas,
  sesiones, checkpoint, próxima acción, notas y seguimiento.
- Selección múltiple, acciones masivas y drag and drop a estados/colecciones manuales con
  teclado, transacción y deshacer seguro.
- Planificador persistente con Kanban, cola, semana, mes, objetivos, capacidad y límites
  WIP.
- Colecciones manuales e inteligentes respaldadas por reglas validadas en Rust y vista
  previa antes de guardar.
- Seguimiento con recordatorios, olvidados, casi terminados, próximos lanzamientos,
  publicaciones recientes del feed oficial de Steam y relaciones por estudio/editor
  verificables. El método público no expone importancia y Vindexa no inventa esa clasificación.
- Importación de manifiestos locales y sincronización oficial con Steam Web API.
- Sincronización periódica mientras Vindexa permanece abierta, con intervalo persistente.
- Diagnóstico persistente del último fallo de Steam, con código y mensaje seguro hasta que
  una sincronización posterior termina correctamente.
- Acciones controladas para jugar, instalar, solicitar desinstalación, revelar una carpeta
  validada y abrir la tienda oficial en una ventana remota aislada.
- Caché local validada de portadas, cabeceras e iconos oficiales, con carga diferida y
  fallback accesible.
- SQLite con WAL, durabilidad `FULL`, claves foráneas, migraciones, búsqueda FTS5, índices,
  copia, restauración con rollback y comprobaciones de integridad y esquema.
- Clave de Steam Web API en el almacén seguro del sistema, separada de SQLite y de las
  copias de seguridad.
- Diálogos nativos de exportación/restauración: el frontend solicita la operación, pero no
  recibe ni proporciona rutas de archivos.
- Densidad y atajos configurables; comprobación manual de updates que permanece desactivada
  hasta existir endpoint HTTPS y clave pública de firma.

## Comandos de desarrollo

| Comando | Uso |
| --- | --- |
| `pnpm tauri dev` | Ejecuta la aplicación de escritorio con recarga durante el desarrollo. |
| `pnpm dev` | Ejecuta solo el frontend Vite; los comandos nativos no están disponibles. |
| `pnpm test` | Ejecuta las pruebas de frontend con Vitest y jsdom. |
| `pnpm test:rust` | Ejecuta unidades y contratos de Rust/SQLite. |
| `pnpm lint` | Analiza el repositorio con Biome. |
| `pnpm build` | Comprueba TypeScript y genera `dist/`. |
| `pnpm audit:dependencies` | Audita dependencias de producción JS y el lockfile Rust. |
| `pnpm check` | Ejecuta lint, frontend tests, frontend build y `cargo check`. |
| `pnpm tauri build` | Compila release y genera los bundles admitidos por el anfitrión. |
| `pnpm tauri:build:debug` | Genera `.app` y DMG debug con firma ad hoc local verificable. |

`pnpm dev` sirve para trabajar en componentes puros. Para validar persistencia, diálogos,
Keychain, URLs `steam://`, OpenID o acceso al sistema de archivos hay que usar
`pnpm tauri dev`.

## Estructura

```text
src/
├── app/                  # Proveedores y raíz React
├── components/           # Estados comunes y primitivas shadcn/Radix
├── features/             # Biblioteca, planificador, colecciones, seguimiento y ajustes
├── hooks/                # Comportamiento React compartido
├── lib/                  # Contratos TypeScript, IPC y formato
└── test/                 # Pruebas Vitest/Testing Library

src-tauri/
├── migrations/           # Esquema SQLite e índices/FTS5 versionados
├── src/db/               # Persistencia, consultas y reglas de organización
├── src/steam/            # OpenID, Web API, manifiestos y acciones permitidas
├── src/art_cache.rs      # Caché validada de imágenes oficiales
├── src/commands.rs       # Frontera IPC de Tauri
└── tests/                # Contratos de modelos, persistencia y escala
```

## Persistencia y seguridad

SQLite es la fuente de verdad. Su ruta efectiva aparece en
**Ajustes → Datos y copias → Ubicación de datos**. El backend activa claves foráneas, WAL,
durabilidad `FULL`, un timeout de cinco segundos y comprobaciones de integridad, historial
de migraciones y esquema al iniciar.

La Web API Key no se guarda en SQLite, `localStorage`, el frontend ni el backup: la gestiona
el almacén seguro nativo mediante el servicio `io.vindexa.desktop`. El inicio de la
aplicación no lee Keychain: `bootstrap` consulta un marcador no secreto en SQLite. Keychain
solo se abre al guardar, eliminar, comprobar voluntariamente o usar la clave para
sincronizar. La autenticación OpenID se abre en el navegador, usa un callback loopback
temporal y valida la afirmación con Steam.

Consulta [PRIVACY.md](./PRIVACY.md) antes de manejar una copia de seguridad: contiene
notas, checkpoints y demás datos personales en texto legible dentro de SQLite.

## Documentación

- [Arquitectura y flujos de datos](./ARCHITECTURE.md)
- [Manual de usuario](./USER_MANUAL.md)
- [Configurar Steam](./STEAM_SETUP.md)
- [Privacidad y almacenamiento](./PRIVACY.md)
- [Seguridad y modelo de amenazas](./SECURITY.md)
- [Esquema SQLite y migraciones](./DATABASE.md)
- [Compilar e instalar](./INSTALL.md)
- [Estrategia y comandos de pruebas](./TESTING.md)
- [Sistema de diseño](./DESIGN.md)
- [Guía de contribución](./CONTRIBUTING.md)
- [Decisiones arquitectónicas](./docs/adr/README.md)
- [Registro de cambios](./CHANGELOG.md)
- [Licencia MIT](./LICENSE)
- [Brief maestro](./VINDEXA_DESIGN_BRIEF.md)

## Límites verificados del alcance actual

- La sincronización remota necesita una Web API Key y una biblioteca visible para la clave;
  Steam puede devolver una colección vacía cuando la privacidad no permite consultar los
  juegos.
- El importador local conoce juegos instalados y metadatos de manifiesto, pero no puede
  deducir tiempos de juego ni perfil sin la Web API.
- Steam Family depende del grupo detectable y de la visibilidad de sus miembros. Catálogo
  visible no equivale a licencia; solo la evidencia local se incorpora como compartida.
- La desinstalación es una solicitud validada al cliente Steam; Vindexa no borra archivos ni
  confirma que Steam haya terminado.
- La ventana integrada de tienda solo navega por el host oficial, no es un navegador general
  ni un bloqueador publicitario completo. En macOS y Linux activa reglas nativas de contenido
  antes de navegar y falla cerrada si no puede instalarlas. La ruta Linux existe, pero su
  funcionamiento en Bazzite real todavía no está certificado.
- **Buscar actualizaciones** no descarga nada: falta endpoint de releases y clave pública.
- No hay telemetría, servidor Vindexa ni sincronización personal en la nube.
- Steam Deck permanece **Sin datos** mientras no exista una API pública documentada que
  Vindexa pueda consumir de forma fiable.
- El bundle debug se firma ad hoc; no existe firma Developer ID ni notarización para una
  distribución pública.
- Bazzite no puede considerarse verificado desde el entorno macOS de desarrollo.

Vindexa no está afiliada a Valve Corporation. Steam y sus marcas pertenecen a sus
respectivos titulares.
