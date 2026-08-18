---
version: alpha
name: Vindexa
description: Biblioteca de Steam densa, técnica y personal con identidad propia.
colors:
  primary: "#5CAAC1"
  primary-strong: "#0D6F9F"
  secondary: "#2E3848"
  tertiary: "#A4D007"
  neutral: "#ABB7B5"
  background: "#171D25"
  surface: "#22262D"
  surface-raised: "#2A3441"
  surface-selected: "#3D5473"
  on-surface: "#EEEEF0"
  on-surface-muted: "#ABB7B5"
  status-muted: "#82939E"
  border: "#3A4554"
  error: "#D85C5C"
  warning: "#D6A64B"
  success: "#7EA64B"
typography:
  headline-lg:
    fontFamily: Inter
    fontSize: 24px
    fontWeight: 600
    lineHeight: 1.2
    letterSpacing: -0.01em
  headline-md:
    fontFamily: Inter
    fontSize: 18px
    fontWeight: 600
    lineHeight: 1.25
    letterSpacing: -0.01em
  title-sm:
    fontFamily: Inter
    fontSize: 15px
    fontWeight: 600
    lineHeight: 1.3
  body-md:
    fontFamily: Inter
    fontSize: 14px
    fontWeight: 400
    lineHeight: 1.45
  body-sm:
    fontFamily: Inter
    fontSize: 13px
    fontWeight: 400
    lineHeight: 1.4
  label-md:
    fontFamily: Inter
    fontSize: 12px
    fontWeight: 600
    lineHeight: 1.2
    letterSpacing: 0.04em
  label-sm:
    fontFamily: Inter
    fontSize: 11px
    fontWeight: 500
    lineHeight: 1.2
    letterSpacing: 0.03em
  data-sm:
    fontFamily: Inter
    fontSize: 12px
    fontWeight: 500
    lineHeight: 1.2
    fontFeature: "tnum"
rounded:
  none: 0px
  xs: 2px
  sm: 3px
  md: 4px
  lg: 6px
  full: 9999px
spacing:
  base: 4px
  xxs: 2px
  xs: 4px
  sm: 8px
  md: 12px
  lg: 16px
  xl: 24px
  xxl: 32px
  sidebar: 272px
  topbar: 64px
  statusbar: 36px
components:
  button-primary:
    backgroundColor: "{colors.primary-strong}"
    textColor: "{colors.on-surface}"
    rounded: "{rounded.sm}"
    height: 34px
    padding: 12px
  button-primary-hover:
    backgroundColor: "{colors.primary}"
  button-secondary:
    backgroundColor: "{colors.secondary}"
    textColor: "{colors.on-surface}"
    rounded: "{rounded.sm}"
    height: 34px
    padding: 12px
  input:
    backgroundColor: "{colors.background}"
    textColor: "{colors.on-surface}"
    rounded: "{rounded.xs}"
    height: 36px
    padding: 10px
  library-row:
    backgroundColor: "{colors.surface}"
    textColor: "{colors.neutral}"
    rounded: "{rounded.none}"
    height: 34px
  library-row-selected:
    backgroundColor: "{colors.surface-selected}"
    textColor: "{colors.on-surface}"
    rounded: "{rounded.none}"
  panel:
    backgroundColor: "{colors.surface}"
    textColor: "{colors.on-surface}"
    rounded: "{rounded.xs}"
  toolbar:
    backgroundColor: "{colors.surface-raised}"
    textColor: "{colors.on-surface}"
    rounded: "{rounded.none}"
  divider:
    backgroundColor: "{colors.border}"
    height: 1px
  statusbar-text:
    textColor: "{colors.status-muted}"
  status-warning:
    backgroundColor: "{colors.warning}"
    textColor: "{colors.background}"
    rounded: "{rounded.xs}"
  status-success:
    backgroundColor: "{colors.success}"
    textColor: "{colors.background}"
    rounded: "{rounded.xs}"
  tooltip:
    backgroundColor: "{colors.background}"
    textColor: "{colors.on-surface-muted}"
    rounded: "{rounded.xs}"
---

# Sistema de diseño de Vindexa

## Overview

Vindexa es una herramienta de escritorio densa, inmediata y silenciosa para personas con bibliotecas de Steam grandes. Adopta la jerarquía compacta, los paneles tonales y la eficacia espacial de Steam, pero conserva identidad propia: un índice personal centrado en decidir, continuar y terminar juegos. Debe sentirse instalada, rápida y utilitaria; nunca como un dashboard SaaS, una landing page ni una maqueta de IA.

## Colors

La interfaz es oscura por defecto. Los cambios de profundidad se expresan con capas de carbón y pizarra; el cian se reserva para navegación, foco y acciones primarias. El verde lima únicamente señala progreso o una decisión positiva concreta.

- **Primary (#5CAAC1):** foco, enlaces y controles activos.
- **Primary Strong (#0D6F9F):** relleno accesible de la acción principal.
- **Background (#171D25):** cromado exterior y fondo raíz.
- **Surface (#22262D):** panel principal y listas.
- **Surface Raised (#2A3441):** barras, cabeceras y controles elevados.
- **Surface Selected (#3D5473):** selección inequívoca de filas o portadas.
- **Tertiary (#A4D007):** progreso y confirmación puntual; nunca decoración.
- **On Surface (#EEEEF0):** texto principal.
- **On Surface Muted (#ABB7B5):** metadatos y texto secundario.
- **Error (#D85C5C):** errores y acciones destructivas con icono y texto.

### Colores definidos por la persona usuaria

Los estados, las colecciones y las etiquetas llevan un color que elige quien usa la
aplicación. Ese color **no forma parte de la paleta del sistema**: identifica una entidad
concreta y puede ser cualquiera.

Reglas que sí se aplican a esos colores:

- se usan como **marca de identidad** —un cuadrado de 6 px, una barra de 3 px, el trazo de un
  icono—, nunca como fondo de un bloque de texto, para que jamás comprometan el contraste;
- el texto que los acompaña usa siempre los tokens del sistema (`--foreground`, `--v-muted`,
  `--v-subtle`), que tienen contraste garantizado;
- el color **no codifica significado**: el tipo de una colección (manual o inteligente) se
  distingue por la forma de su barra, no por su tono, porque el tono ya está ocupado por la
  identidad.

## Typography

Se usa **Inter Variable** incluida en el paquete para aproximar la compacidad humanista de una aplicación de biblioteca sin depender de la tipografía propietaria de Steam. Los títulos son contenidos; el cuerpo normal es de 13–14 px y los metadatos de 11–12 px. Las cifras usan `tnum` para evitar saltos durante sincronizaciones y recuentos.

## Layout

El marco de escritorio ocupa toda la ventana: barra superior fija, barra lateral persistente, lienzo de contenido y barra de estado inferior. La unidad base es 4 px. La barra lateral parte de 272 px y puede compactarse; el contenido se adapta desde 960 px hasta pantallas ultrapanorámicas. Las colecciones extensas siempre se virtualizan y cada vista conserva búsqueda, filtros y posición de desplazamiento.

La densidad predeterminada es compacta. Los modos cómodo y ultracompacto cambian exclusivamente tokens de altura, separación y tipografía, no la arquitectura de la pantalla.

## Elevation & Depth

La profundidad se obtiene mediante capas tonales, bordes de un píxel y sombras muy contenidas. Los paneles ordinarios no flotan. Solo menús, tooltips, diálogos y el panel contextual pueden usar sombra, y siempre sobre un scrim suficiente.

## Shapes

La forma es técnica y casi ortogonal. Filas y separadores no tienen radio; botones, campos y portadas usan 2–4 px. Los pills se reservan para estados y etiquetas que necesitan encerrar una palabra o un número.

## Components

- La navegación principal combina texto e icono, con selección por color, peso y barra de 3 px.
- La biblioteca lateral usa filas de 34 px, miniatura de 22 px y truncado con tooltip.
- Las portadas mantienen proporción 2:3, espacio reservado y carga progresiva; el estado aparece como banda o icono acompañado de texto accesible.
- El panel de juego entra desde la derecha y conserva el contexto de la biblioteca; el autoguardado muestra `Guardando`, `Guardado` o un error recuperable.
- El hero de la ficha ocupa una franja inmersiva con título y acciones fuera del flujo de texto; el parallax es sutil, ligado al scroll y desaparece por completo con reducción de movimiento.
- La ficha separa Plan personal, Información, Registro y Actividad. Descripción, pestañas y formularios conservan su propia altura: ningún texto puede crecer por debajo de otra sección o quedar cortado por una cabecera fija.
- Fechas, etiquetas y sesiones usan secciones densas con títulos, ayuda breve y feedback en vivo; edición y borrado mantienen acciones explícitas y foco visible.
- Los filtros complejos viven en popovers accesibles; los activos se reflejan en chips compactos eliminables.
- El drag and drop tiene destino visible, prohibido y activo, overlay opaco, contador de multiselección y alternativa completa mediante teclado o controles de lote. Deshacer aparece junto al feedback de la operación.
- La tienda oficial vive en una ventana nativa independiente. No imita controles de un navegador ni introduce un tercer lenguaje visual dentro del shell principal.
- Los estados de carga superiores a 300 ms usan skeletons de geometría estable; nunca sustituyen datos reales por contenido inventado.

## Do's and Don'ts

- Do mantener una densidad comparable a Steam y mostrar metadatos útiles cerca del juego.
- Do usar una sola acción primaria por contexto y feedback visible en menos de 100 ms.
- Do mantener WCAG 2.2 AA: 4.5:1 para texto normal, 3:1 para texto grande y foco siempre visible.
- Do ofrecer navegación por teclado y alternativa a cada gesto de arrastre.
- Do respetar `prefers-reduced-motion` y limitar las transiciones funcionales a 100–240 ms.
- Don't usar logotipos, tipografías propietarias, portadas falsas ni otros recursos protegidos de Steam.
- Don't usar gradientes morados, glassmorphism ornamental, tarjetas grandes ni espacios vacíos sin función.
- Don't introducir colores, radios, sombras o tiempos fuera de los tokens sin actualizar y volver a validar este documento.
- Don't usar animación en acciones frecuentes de teclado ni mover datos que el usuario está leyendo.
