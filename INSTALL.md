# Compilar e instalar Vindexa

Vindexa se compila con el toolchain nativo de cada plataforma. Un bundle macOS no certifica
Linux y no se debe presentar como compatible con Bazzite sin construirlo y ejecutarlo allí.

> [!IMPORTANT]
> La configuración de release no contiene identidad Developer ID, notarización,
> endpoint/clave pública de updater ni pipeline de publicación. El script debug aplica una
> firma ad hoc mediante `tauri.debug.conf.json`; produce artefactos locales verificables,
> no una release pública autenticada.

## Índice

- [Instalar desde una release](#instalar-desde-una-release)
- [Preparar el repositorio](#preparar-el-repositorio)
- [Ejecutar en desarrollo](#ejecutar-en-desarrollo)
- [Compilar para macOS](#compilar-para-macos)
- [Compilar y probar en Bazzite](#compilar-y-probar-en-bazzite)
- [Actualizar sin perder datos](#actualizar-sin-perder-datos)
- [Desinstalar](#desinstalar)
- [Diagnosticar una instalación](#diagnosticar-una-instalación)

## Instalar desde una release

Si sólo quieres usar Vindexa, no hace falta compilar nada: descarga el instalador de tu
sistema desde la [última versión](https://github.com/SwonDev/Vindexa/releases/latest).

| Sistema | Archivo | Qué hacer |
| --- | --- | --- |
| **macOS** | `Vindexa_x.y.z_universal.dmg` | Abre el DMG y arrastra Vindexa a Aplicaciones |
| **Windows** | `Vindexa_x.y.z_x64-setup.exe` | Ejecútalo. El `.msi` está para despliegues desatendidos |
| **Linux** | `Vindexa_x.y.z_amd64.AppImage` | `chmod +x` y ejecútalo, sin instalar nada |
| **Linux (Debian y derivadas)** | `Vindexa_x.y.z_amd64.deb` | `sudo apt install ./Vindexa_*.deb` |

El binario de macOS es universal: funciona en Apple Silicon y en Intel sin elegir versión.

### Los instaladores no están firmados

No hay certificado de Developer ID de Apple ni de Authenticode de Microsoft, así que el
sistema avisará la primera vez. El aviso es correcto: significa que el sistema no puede
verificar quién publicó el archivo, no que el archivo esté dañado.

**macOS.** Al abrirlo dirá que no se puede comprobar que esté libre de software malicioso.
Ve a **Ajustes del Sistema → Privacidad y seguridad**, busca el aviso sobre Vindexa y pulsa
**Abrir de todos modos**. Sólo hay que hacerlo una vez.

**Windows.** SmartScreen dirá que ha protegido tu PC. Pulsa **Más información** y luego
**Ejecutar de todas formas**.

**Linux.** No hay aviso: el AppImage sólo necesita permiso de ejecución.

Si prefieres no confiar en un binario sin firmar, compila desde el código fuente siguiendo el
resto de este documento. Es la única forma de verificar por ti mismo qué estás ejecutando.

### La primera vez que se abre

Vindexa crea su base de datos y **no muestra ningún juego**. No hay catálogo de demostración
ni carátulas falsas: un estado vacío significa que todavía no has importado nada. Desde
**Ajustes → Steam** puedes leer tu instalación local —sin cuenta ni clave— o vincular la
cuenta para traer tiempos de juego y logros. En [STEAM_SETUP.md](./STEAM_SETUP.md) están las
tres vías explicadas.

## Preparar el repositorio

Requisitos comunes:

- Node.js y Corepack;
- `pnpm` resuelto mediante Corepack;
- Rust estable con Cargo;
- Git;
- dependencias nativas de [Tauri 2](https://v2.tauri.app/start/prerequisites/).

Desde la raíz:

```bash
corepack enable
pnpm install --frozen-lockfile --ignore-scripts
pnpm check
pnpm test:rust
```

`pnpm-lock.yaml` y `Cargo.lock` fijan las versiones reproducibles. No mezcles npm o Yarn ni
regeneres los locks durante una instalación normal.

`--ignore-scripts` evita ejecutar scripts de instalación de paquetes. Si en el futuro una
dependencia necesita uno, revísalo explícitamente antes de quitar esta protección.

## Ejecutar en desarrollo

```bash
pnpm tauri dev
```

Este comando inicia Vite en `http://localhost:1420`, compila Rust y abre la ventana Tauri.
Es la única ruta de desarrollo que permite probar SQLite, Keychain, diálogos, callback
OpenID, manifiestos y URLs `steam://`.

`bootstrap` no abre Keychain. En desarrollo, solo una acción explícita que use la clave
puede mostrar el aviso de macOS; al recompilar, cambia la firma ad hoc del binario y Keychain puede
volver a solicitar autorización aunque se hubiera permitido una build anterior.

Para comprobar únicamente componentes web:

```bash
pnpm dev
```

El frontend aislado no dispone del bridge IPC. Un error de `invoke` en esa ruta no demuestra
un fallo del bundle de escritorio.

## Compilar para macOS

### Requisitos

- macOS 11 o posterior para ejecutar el bundle, según `tauri.conf.json`;
- Xcode Command Line Tools;
- Rust estable y el target de la arquitectura local;
- Node.js/Corepack/pnpm.

### Bundle local

Ejecuta primero todos los gates y después limita el bundle a los formatos macOS:

```bash
pnpm check
pnpm test:rust
pnpm tauri build --bundles app,dmg
```

Los resultados se generan bajo:

```text
src-tauri/target/release/bundle/macos/Vindexa.app
src-tauri/target/release/bundle/dmg/
```

El DMG usa `assets/brand/vindexa-installer-background.png`, el icono generado de Vindexa y
una ventana de 660 × 400 px. La aplicación utiliza el identificador
`io.vindexa.desktop` y el bundle declara categoría Entertainment.

### Verificación local antes de copiar a Aplicaciones

```bash
open src-tauri/target/release/bundle/macos/Vindexa.app
```

Comprueba los puntos de [TESTING.md](./TESTING.md), cierra la aplicación y solo entonces
arrastra el bundle a `/Applications` mediante Finder. Sustituir una versión existente es
una acción externa: conserva antes un backup desde **Ajustes → Datos y copias**.

### Firma y notarización

Una distribución pública fuera de la Mac App Store necesita firma Developer ID y
notarización. No están configuradas en el repositorio. La pseudoidentidad `-` del overlay
debug sella el bundle local, pero no autentica al desarrollador ni sustituye la notarización.
No publiques el DMG local como si estuviera notarizado y no añadas credenciales de firma a
archivos versionados.

## Compilar y probar en Bazzite

Bazzite es un sistema Linux inmutable basado en Fedora. La ruta de menor impacto para una
evaluación local es construir en un entorno Fedora compatible y ejecutar un AppImage; no
requiere instalar un RPM en el sistema base.

### Dependencias del builder

El entorno Linux de compilación debe incluir Node/Corepack, Rust y las dependencias Tauri
para Fedora/OSTree: WebKitGTK 4.1, OpenSSL, indicadores GTK, librsvg, libxdo y herramientas C.
Consulta siempre la sección Fedora/OSTree de los
[prerrequisitos oficiales](https://v2.tauri.app/start/prerequisites/) para los nombres de
paquete vigentes de la imagen Bazzite instalada.

Tauri recomienda construir Linux sobre la base más antigua que se vaya a soportar para no
elevar accidentalmente la versión mínima de glibc. La
[guía de AppImage](https://v2.tauri.app/distribute/appimage/) explica esta limitación.

### Build nativo

Dentro del builder Linux y desde el checkout:

```bash
corepack enable
pnpm install --frozen-lockfile --ignore-scripts
pnpm check
pnpm test:rust
pnpm tauri build --bundles appimage,rpm
```

En Linux, Tauri deja normalmente los artefactos bajo:

```text
src-tauri/target/release/bundle/appimage/
src-tauri/target/release/bundle/rpm/
```

El propio `pnpm tauri build --help` del host Linux es la fuente de verdad para los bundles
admitidos por esa instalación.

### Ejecutar el AppImage sin modificar el sistema base

Desde el directorio de salida:

```bash
chmod a+x ./*.AppImage
./*.AppImage
```

No instales el RPM con `rpm-ostree` como parte de una prueba automática: crea un nuevo
deployment del sistema y puede exigir reinicio. Si se decide validar ese formato, debe ser
una acción explícitamente autorizada, con rollback de Bazzite conocido.

### Gate Bazzite obligatorio

La certificación necesita evidencia en una sesión gráfica Bazzite real:

1. el AppImage inicia bajo Wayland y muestra el icono correcto;
2. se crea SQLite en el directorio de datos del usuario;
3. el almacén de secretos del escritorio acepta, recupera y elimina una clave de prueba;
4. Steam OpenID vuelve al listener loopback;
5. los manifiestos de la instalación Steam de Bazzite se detectan;
6. `steam://install`, `steam://run` y `steam://uninstall` llegan al cliente Steam sin borrado
   directo por Vindexa;
7. la ventana aislada de tienda abre el host oficial sin exponer IPC, instala el filtro
   nativo WebKitGTK antes de navegar y falla cerrada si no puede hacerlo;
8. una edición personal sigue presente después de cerrar y abrir;
9. exportar y restaurar una copia funciona;
10. no faltan WebKitGTK, GTK, SSL u otras bibliotecas dinámicas.

Este gate no puede ejecutarse desde el Mac de desarrollo. Hasta completarlo, el soporte de
Bazzite permanece **no verificado**.

La verificación del secreto debe distinguir dos casos: reiniciar/consultar `bootstrap` no
debe abrir el servicio de secretos; **Comprobar clave guardada** y una sincronización sí
deben acceder a él de forma explícita.

## Actualizar sin perder datos

El identificador `io.vindexa.desktop` debe mantenerse estable entre versiones. Tauri guarda
la base fuera del bundle, por lo que sustituir `Vindexa.app` o el AppImage no debe borrar
SQLite.

Antes de actualizar:

1. abre **Ajustes → Datos y copias**;
2. confirma que Integridad muestra `ok` y WAL está activo;
3. exporta un backup a una ruta distinta de la base activa;
4. cierra Vindexa;
5. sustituye solo el binario/bundle;
6. abre la nueva versión y comprueba versión de esquema, biblioteca y una nota conocida.

**Ajustes → Acerca de → Buscar actualizaciones** no sustituye este procedimiento. En el
build actual devuelve `notConfigured`: informa la versión instalada, pero no consulta un
servidor, descarga, verifica o ejecuta binarios. Un updater futuro necesitará HTTPS,
manifiesto firmado y clave pública configurados antes de habilitarse.

Las migraciones son acumulativas e idempotentes. No copies manualmente una base mientras la
aplicación está abierta; el archivo WAL puede contener cambios pendientes.

## Verificar artefactos

Calcula el checksum antes de mover o distribuir un bundle:

```bash
# macOS
shasum -a 256 src-tauri/target/release/bundle/dmg/*.dmg

# Linux
sha256sum src-tauri/target/release/bundle/appimage/*.AppImage
sha256sum src-tauri/target/release/bundle/rpm/*.rpm
```

Conserva el digest junto a la versión exacta `0.1.0` y no lo reutilices después de
recompilar.

## Desinstalar

### macOS

1. Cierra Vindexa.
2. Mueve `Vindexa.app` a la Papelera con Finder.
3. Decide por separado si quieres conservar la base para una reinstalación.

### Bazzite con AppImage

1. Cierra Vindexa.
2. Mueve el archivo AppImage a la Papelera.
3. Conserva o elimina por separado el directorio de datos mostrado en Diagnóstico.

Eliminar el bundle no borra automáticamente SQLite, backups, caché ni la entrada del
almacén seguro. Consulta [PRIVACY.md](./PRIVACY.md) para una eliminación completa y segura.

## Diagnosticar una instalación

### La app no abre en macOS

- Confirma que el bundle se generó en el host actual y que macOS cumple el mínimo 11.0.
- Una build local no notarizada puede requerir aprobación manual de Gatekeeper; no desactives
  globalmente sus protecciones.
- Ejecuta `pnpm tauri dev` desde el checkout para separar un fallo de código de uno de
  empaquetado.

### El AppImage falla en Bazzite

- Ejecuta el archivo desde terminal y conserva el mensaje de biblioteca ausente.
- Confirma que el artefacto se construyó para la arquitectura y glibc de destino.
- Comprueba WebKitGTK 4.1 y el servicio de secretos de la sesión gráfica.
- No asumas que un AppImage generado en otra distribución certifica Bazzite.

### La app abre sin datos anteriores

- Comprueba en Ajustes la ruta efectiva y el identificador del bundle.
- No restaures a ciegas: valida primero el backup desde la UI.
- Si cambió el usuario del sistema o el formato de empaquetado, el directorio Tauri puede ser
  distinto.
