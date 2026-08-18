# Contribuir a Vindexa

Vindexa acepta cambios que preserven su contrato local-first, la organización personal y la
densidad de escritorio descrita en [DESIGN.md](./DESIGN.md). Antes de escribir código, busca
si la función ya existe y extiéndela en vez de crear una implementación paralela.

## Índice

- [Preparar el entorno](#preparar-el-entorno)
- [Fuentes de verdad](#fuentes-de-verdad)
- [Flujo de trabajo](#flujo-de-trabajo)
- [Arquitectura y estilo](#arquitectura-y-estilo)
- [Migraciones SQLite](#migraciones-sqlite)
- [Steam, privacidad y seguridad](#steam-privacidad-y-seguridad)
- [Sistema de diseño y accesibilidad](#sistema-de-diseño-y-accesibilidad)
- [Pruebas obligatorias](#pruebas-obligatorias)
- [Documentación y commits](#documentación-y-commits)
- [Publicar una versión](#publicar-una-versión)
- [Lista de revisión](#lista-de-revisión)

## Preparar el entorno

Requisitos: Node.js con Corepack, `pnpm`, Rust estable y dependencias nativas de Tauri 2.

```bash
corepack enable
pnpm install --frozen-lockfile --ignore-scripts
pnpm check
pnpm test:rust
```

`pnpm-lock.yaml` y `Cargo.lock` pertenecen al repositorio. No mezcles npm/Yarn ni actualices
dependencias como efecto secundario de otra tarea.

## Fuentes de verdad

Lee antes de modificar:

1. [VINDEXA_DESIGN_BRIEF.md](./VINDEXA_DESIGN_BRIEF.md): intención y alcance de producto.
2. [DESIGN.md](./DESIGN.md): tokens y reglas visuales; prevalece sobre valores improvisados.
3. [ARCHITECTURE.md](./ARCHITECTURE.md): fronteras y flujos ejecutables.
4. [DATABASE.md](./DATABASE.md): invariantes y migraciones.
5. El código y sus tests: prevalecen sobre documentación obsoleta respecto al comportamiento
   que existe hoy.

Si el brief pide una capacidad que todavía no existe, documenta la diferencia; no presentes
una intención como implementación.

## Flujo de trabajo

1. Reproduce el problema o define una costura pública observable.
2. Busca implementaciones y tests existentes con `rg`.
3. Añade un test que falle por la razón correcta.
4. Implementa el cambio mínimo completo.
5. Refactoriza sin romper el gate.
6. Ejecuta pruebas proporcionales y, antes de entregar, el conjunto completo.
7. Para UI, ejecuta la app Tauri y revisa píxeles reales en 960 × 680 y 1440 × 900 como
   mínimo; no actualices una referencia visual sin revisión independiente.
8. Actualiza documentación y changelog cuando cambie un contrato visible, esquema, riesgo o
   limitación.

No entregues `TODO`, `FIXME`, botones decorativos, juegos demo o estados que finjan datos de
Steam.

## Arquitectura y estilo

### TypeScript y React

- Mantén TypeScript estricto y los payloads IPC en `camelCase`.
- TanStack Query gestiona datos nativos y su invalidación; evita una segunda fuente global.
- La UI no importa SQL, Keychain, `reqwest` ni acceso de archivos.
- Usa las primitivas shadcn/Radix existentes; no dupliques un botón, diálogo o selector.
- Conserva estados de carga, vacío, error y feedback accesible.
- Virtualiza colecciones grandes y evita cargar todos los juegos para filtrar en React.

### Rust

- Los comandos Tauri deserializan, validan y delegan; la lógica pertenece al módulo de
  dominio/persistencia correspondiente.
- Usa parámetros SQL. Cualquier fragmento dinámico debe proceder de una allowlist interna.
- Envía SQLite o filesystem bloqueante a la costura prevista y respeta el lock de
  mantenimiento.
- Devuelve `AppError` con código y mensaje seguro; no incluyas secretos, SQL interno o URLs
  con credenciales.
- No abras una URL o ruta recibida directamente desde React.

## Migraciones SQLite

1. Nunca edites una migración aplicada.
2. Reserva el siguiente número y crea un archivo aditivo en `src-tauri/migrations/`.
3. Regístralo en `src-tauri/src/db/migrations.rs`.
4. Añade pruebas de base nueva y actualización desde la versión anterior.
5. Demuestra idempotencia del inicializador, integridad, foreign keys e invariantes.
6. Si el cambio corrige datos heredados, define una cohorte conservadora y una ruta de
   promoción posterior; no reclasifiques datos ambiguos como confirmados.
7. Actualiza [DATABASE.md](./DATABASE.md) y [CHANGELOG.md](./CHANGELOG.md).

## Steam, privacidad y seguridad

- No pidas, imprimas o guardes contraseñas, Steam Guard o Web API Keys.
- No añadas fallback a texto plano si falla `keyring`.
- Mantén OpenID en el navegador oficial y valida la afirmación en Rust.
- Diferencia dato ausente de cero y fuente pública no documentada de una API contractual.
- Los miembros de Steam Family son datos transitorios; no persistas identidad de terceros.
- No debilites CSP/capabilities para facilitar una integración. Una ventana remota no debe
  compartir IPC con la principal.
- Cualquier acción destructiva necesita confirmación y feedback honesto sobre lo que Steam,
  no Vindexa, terminó realmente.

Lee [SECURITY.md](./SECURITY.md) y [PRIVACY.md](./PRIVACY.md) antes de tocar estas áreas.

## Sistema de diseño y accesibilidad

- Usa exclusivamente tokens de [DESIGN.md](./DESIGN.md). Si falta uno, actualiza el contrato
  y vuelve a ejecutar su lint.
- Mantén la densidad comparable a Steam sin copiar marca o activos protegidos.
- Valida foco, contraste WCAG 2.2 AA, nombres accesibles y teclado completo.
- Todo drag debe tener alternativa sin puntero y anuncio comprensible.
- Respeta `prefers-reduced-motion`; parallax y transformaciones no esenciales deben
  desactivarse.
- Comprueba textos largos, Unicode y ventana mínima sin solapamientos ni cortes tonales.

Validación del contrato de diseño:

```bash
pnpm dlx @google/design.md lint DESIGN.md
```

## Pruebas obligatorias

Durante el ciclo usa el test específico. Antes de entregar:

```bash
pnpm check
pnpm test:rust
cargo fmt --manifest-path src-tauri/Cargo.toml --check
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets --all-features -- -D warnings
```

Una función nativa necesita además `pnpm tauri dev` o el bundle correspondiente. Para una
release sigue la matriz completa de [TESTING.md](./TESTING.md); una build macOS no demuestra
Bazzite.

## Documentación y commits

- Markdown en UTF-8, español con tildes y enlaces relativos para archivos del repositorio.
- Explica el comportamiento, el motivo y los límites; evita narrar líneas de código.
- No añadas URLs de release, soporte o seguridad que no hayan sido verificadas.
- Mantén los cambios enfocados y no reviertas trabajo ajeno no relacionado.
- No incluyas bases, backups, Keychain, rutas personales, capturas privadas ni artefactos de
  compilación.

## Publicar una versión

El criterio de versionado del proyecto avanza el tercer número hasta `0.1.10` y a partir de
ahí pasa a `0.2.0`: `0.1.0` → `0.1.1` → … → `0.1.9` → `0.1.10` → `0.2.0`.

La versión vive por triplicado —`package.json`, `src-tauri/Cargo.toml` y
`src-tauri/tauri.conf.json`— porque cada herramienta lee la suya. Nunca se editan a mano:

```bash
scripts/version.sh siguiente     # o scripts/version.sh 0.1.4 para fijarla
```

Después:

1. Pasa el contenido de `[Sin publicar]` del `CHANGELOG.md` a la nueva versión, con su fecha.
2. `git commit -am "chore: versión x.y.z"`
3. `git tag vx.y.z && git push origin main --follow-tags`

El flujo de publicación comprueba que la etiqueta coincide con la versión declarada antes de
compilar nada, construye los instaladores de macOS, Windows y Linux, y saca la release del
borrador sólo cuando las tres han subido lo suyo.

### Firma de código

Las releases salen **sin firmar**: no hay certificados. El flujo ya lee los secretos que harían
falta, así que en cuanto existan no hay que tocar nada:

| Secreto | Para qué |
| --- | --- |
| `APPLE_CERTIFICATE` · `APPLE_CERTIFICATE_PASSWORD` · `APPLE_SIGNING_IDENTITY` | Firmar el `.app` y el DMG con un Developer ID |
| `APPLE_ID` · `APPLE_PASSWORD` · `APPLE_TEAM_ID` | Notarizar el DMG |

No hay secreto de actualizaciones automáticas porque **Buscar actualizaciones** todavía no
descarga nada: falta el punto de publicación y la clave pública de firma.

### Material visual

Las capturas y los vídeos del README se regeneran con `scripts/vitrina.sh`, que necesita
`magick`, `cwebp` y `ffmpeg`. Salen del escenario `showcase` de las pruebas de extremo a
extremo: la aplicación real sobre un catálogo de muestra, nunca una biblioteca personal.

## Lista de revisión

- [ ] La función no duplica una existente.
- [ ] Hay prueba roja/verde del contrato público.
- [ ] La organización personal sobrevive a importación, sincronización y reinicio.
- [ ] No se introducen datos mock en runtime.
- [ ] Los límites de Steam o plataforma se muestran con honestidad.
- [ ] UI, teclado, reducción de movimiento y ventana mínima se verificaron si aplica.
- [ ] Migraciones y documentación se actualizaron si aplica.
- [ ] `pnpm check`, Rust, fmt y Clippy pasan.
