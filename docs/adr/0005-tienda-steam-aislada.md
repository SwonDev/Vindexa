# ADR 0005: Tienda Steam en una ventana aislada

- Estado: Aceptada
- Fecha: 2026-08-14

## Contexto

El usuario quiere consultar la ficha oficial sin romper la continuidad visual. Cargar
contenido remoto dentro del WebView privilegiado o construir un navegador general sería un
riesgo desproporcionado.

## Decisión

**Tienda integrada** crea/reutiliza una `WebviewWindow` remota separada. La navegación superior
solo acepta HTTPS y el host exacto `store.steampowered.com`. La ventana usa modo privado,
deniega popups y descargas y desactiva autofill/DevTools. En macOS y Linux instala, antes de
navegar, una lista nativa acotada de reglas; el fallo o timeout cierra la ventana. macOS usa
`WKContentRuleList` y Linux la API nativa de WebKitGTK. En otras plataformas no hay filtro
adicional. La ventana no recibe capabilities ni IPC de Vindexa.

## Consecuencias

- La tienda se siente integrada sin mezclar contenido remoto con SQLite o Keychain.
- Navegaciones a comunidad, cuenta o soporte se bloquean; deben abrirse en el navegador
  oficial.
- El bloqueo de trackers es defensivo y acotado; no es un adblock completo. La ruta Linux
  todavía requiere validación runtime en una sesión gráfica Bazzite real.
- Steam controla HTML, recursos y disponibilidad; Vindexa no puede prometer compatibilidad
  permanente con cambios del sitio.
