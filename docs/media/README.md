# Material visual del repositorio

Capturas y vídeos de la aplicación real. Todo sale del escenario `showcase` de
las pruebas de extremo a extremo: es Vindexa de verdad, con el arte oficial de
las tiendas, sobre un catálogo de muestra de 48 juegos.

El catálogo es de muestra a propósito. Una biblioteca real contiene la cuenta y
los juegos de quien la tiene, y este repositorio es público. Además así el
material es reproducible: cualquiera puede regenerarlo y obtener lo mismo.

Se regenera con:

```bash
scripts/vitrina.sh            # capturas y vídeos
scripts/vitrina.sh imagenes   # sólo las capturas
scripts/vitrina.sh videos     # sólo los vídeos
```

Necesita `magick` (ImageMagick), `cwebp` y `ffmpeg`. Las capturas sin enmarcar
quedan en `artifacts/showcase/`, que no se publica.

## Capturas

| Archivo | Qué muestra | Tamaño |
| --- | --- | --- |
| `biblioteca-rejilla.webp` | La biblioteca en cuadrícula, a 1920×1080 | 192 KB |
| `biblioteca-lista.webp` | La misma biblioteca en lista, con progreso y última sesión | 60 KB |
| `biblioteca-ultracompacta.webp` | Densidad ultracompacta, para bibliotecas de miles de juegos | 72 KB |
| `biblioteca-agrupada.webp` | Agrupación por estado, con encabezado y recuento por grupo | 60 KB |
| `ficha-juego.webp` | Ficha de un juego con su arte, metadatos y acciones | 76 KB |
| `ficha-prioridad.webp` | La explicación de la prioridad: qué señales la mueven y cuánto | 56 KB |
| `ficha-contenido-adicional.webp` | Contenido adicional con su evidencia de propiedad | 64 KB |
| `deseados.webp` | Lista de deseados por intención, con precios y descuentos | 52 KB |
| `planificador.webp` | Planificador con cola, semana y límites de trabajo en curso | 56 KB |
| `colecciones.webp` | Colecciones manuales e inteligentes | 132 KB |
| `seguimiento.webp` | Seguimiento: olvidados, casi terminados y próximos | 92 KB |
| `avisos.webp` | Bandeja de avisos y reglas programables | 108 KB |
| `paleta-comandos.webp` | Paleta de comandos buscando en toda la aplicación | 96 KB |
| `ajustes-familia.webp` | Ajustes de Steam, con el vínculo de sesión para el catálogo de Family | 84 KB |

Todas a 1440×936, WebP con calidad 88. Por debajo de esa calidad se ven
artefactos en el texto de 11 px de los metadatos, que es justo lo que hay que
poder leer en una captura.

## Vídeos

| Archivo | Qué muestra | Duración | Tamaño |
| --- | --- | --- | --- |
| `recorrido-biblioteca.mp4` | Desplazamiento por la biblioteca, con el fundido de los bordes | 9 s | 2,0 MB |
| `densidades.mp4` | Cambio entre cuadrícula, lista y ultracompacta | 9 s | 732 KB |
| `ficha-juego.mp4` | Apertura y recorrido de la ficha de un juego | 9 s | 912 KB |
| `paleta-comandos.mp4` | La paleta de comandos filtrando mientras se escribe | 9 s | 656 KB |

Todos a 1440×936, H.264 con el índice al principio para que se puedan
reproducir sin descargarlos enteros. Sin audio: no hay nada que oír.
