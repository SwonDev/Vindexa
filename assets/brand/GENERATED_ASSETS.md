# Recursos generados de Vindexa

El icono oficial `vindexa-icon.png` fue elegido previamente por el usuario y no se ha modificado durante esta generación. Los recursos de esta carpeta son originales, no incorporan marcas de Steam ni portadas ficticias de juegos.

## Método

- Ruta: herramienta integrada `image_gen`.
- Modelo solicitado por el proyecto: `gpt-image-2`. La herramienta integrada no expone el identificador interno del modelo, por lo que no se afirma una verificación que la interfaz no permite realizar.
- Referencia visual: `vindexa-icon.png`, utilizada únicamente como guía de materiales, iluminación, paleta y nivel de acabado.
- Revisión: inspección visual individual, comprobación de dimensiones y canales mediante ImageMagick y optimización PNG sin pérdida.

## Fondo de bienvenida

- Archivo final: `vindexa-welcome-background.png`
- Dimensiones: 1672 × 941 px, RGB.
- SHA-256: `8f0e6ef9ff6da7a45993c0b82d06ae7766b8080f2731be02fa2ab8a98dfe1929`
- Uso recomendado: bienvenida o estado de cuenta no vinculada, con contenido textual en el 42 % izquierdo.
- Fuente generada: `exec-796d52d4-398c-43b5-9beb-038312930fa2.png`.

### Prompt final

```text
Use case: stylized-concept
Asset type: wide desktop welcome background for the Vindexa game-library application, 16:9 landscape
Input images: Image 1 is the authoritative Vindexa app-icon style and material reference; do not reproduce the icon or V mark literally
Primary request: create an atmospheric premium background suggesting a vast personal game library as layered collectible game cards receding into depth, with an abstract misty mountain-and-pine silhouette subtly embedded in surfaces
Scene/backdrop: deep navy-black gallery space with restrained volumetric haze and faint architectural layers
Style/medium: polished high-end 3D product visualization, tactile anodized dark metal, smoked glass, fine micro-texture, AAA desktop-software art direction
Composition/framing: wide 16:9; keep the entire left 42 percent calm and very dark for readable welcome copy and controls; place the layered-card focal cluster on the right half; safe margins; no important details at edges
Lighting/mood: elegant low-key lighting, thin cyan rim lights, one very restrained chartreuse accent near lower-right, cinematic but quiet
Color palette: #171D25, #22262D, #2E3848, #0D6F9F, #5CAAC1, tiny #A4D007 accent
Materials/textures: dark powder-coated metal, brushed graphite, smoked glass, fine-grain card stock
Constraints: background only; no words, letters, numbers, logos, icons, UI panels, people, weapons, game characters, game-box brands, watermarks; original IP; must remain subtle behind UI; avoid bright central glow; avoid busy wallpaper; avoid resemblance to Steam branding or logo
```
## Ilustración de biblioteca vacía

- Archivo final: `vindexa-empty-library.png`
- Dimensiones: 768 × 512 px, RGBA real.
- Cobertura alfa media: 0,364; alfa mínimo 0 y máximo 1.
- SHA-256: `2fce6ba1cdda5e367ceb84d410c3aafc7f2c403e1052d44b1088ba74666d09ae`
- Uso recomendado: vacío de biblioteca, colección o resultados; se mantiene legible entre 280 y 420 px CSS.
- Fuente generada final: `exec-b137e310-d691-434b-9893-48e765f57375.png`.
- Iteración descartada: la primera salida simuló la transparencia con un damero opaco. No se incorporó al proyecto. La segunda pasada eliminó únicamente el fondo y se verificó el canal alfa.

### Prompt de generación

```text
Use case: stylized-concept
Asset type: transparent-background empty-state illustration for the Vindexa desktop library, centered 4:3 composition
Input images: Image 1 is the authoritative material, lighting, palette, and quality reference; do not reproduce its V letter or outer app-icon tile
Primary request: an elegant small stack of three blank collectible game-library cards, slightly fanned, with one card gently lifted as if waiting to be filled; a subtle thin circular orbit line and three tiny square particles imply discovery and organization
Scene/backdrop: genuinely transparent background, clean isolated object with natural soft contact shadow fading to transparency
Style/medium: premium high-end 3D product illustration, AAA desktop software, restrained and legible at 280 to 420 CSS pixels
Composition/framing: centered, balanced silhouette, generous transparent padding, no cropping, minimal complexity
Lighting/mood: soft cool studio key light, fine cyan rim light, extremely small chartreuse accent on the lowest card edge
Color palette: graphite #22262D, steel #2E3848, cyan #5CAAC1, tiny lime #A4D007
Materials/textures: brushed dark metal edges, smoked glass faces, faint topographic/pine embossing without identifiable artwork
Constraints: truly transparent RGBA background; no words, letters, numbers, logos, V shapes, Steam symbols, game characters, brands, UI panels, watermarks; original IP; no checkerboard painted into image; crisp silhouette; quiet premium mood
```

### Prompt de corrección alfa

```text
Use case: background-extraction
Asset type: corrected transparent-background empty-state illustration for Vindexa
Input images: Image 1 is the edit target; preserve the three-card object, orbit line, particles, materials, colors, camera, lighting, and composition exactly
Primary request: remove only the white and light-gray checkerboard/background completely and replace it with genuine transparent alpha
Constraints: change only the background; preserve every foreground object and edge; output a true RGBA PNG with alpha; outside the object and its soft contact shadow alpha must be 0; preserve soft semitransparent shadow and antialiased edges; do not paint any checkerboard, white, gray, color, texture, floor, backdrop, vignette, or glow into transparent pixels; no text, logos, letters, symbols, or watermark
Avoid: simulated transparency, checkerboard pattern, opaque or white background, cut-out halos
```

## Fondo del instalador

- Archivo final: `vindexa-installer-background.png`
- Dimensiones: 660 × 400 px, RGB.
- SHA-256: `7e47999c12c06f742e967ad24a556322da0717a82c9ada0b75880d293105236e`
- Uso recomendado: fondo de DMG para macOS; las dos zonas circulares admiten los iconos de Vindexa y Aplicaciones sin competir con las etiquetas.
- Fuente generada: `exec-54bfdf57-79a3-4e89-9d7a-00577d42878a.png`.

### Prompt final

```text
Use case: product-mockup
Asset type: installer and disk-image background artwork for the Vindexa macOS desktop application, wide 5:3 landscape
Input images: Image 1 is the authoritative Vindexa app-icon style, material, palette, and quality reference; do not reproduce the icon tile or V letter literally
Primary request: a restrained premium product-stage background built from layered dark metallic library cards forming a shallow diagonal pathway from lower-left toward upper-right, suggesting moving a library into its new home
Scene/backdrop: dark navy graphite studio space with subtle depth, a faint mountain-and-pine relief in the far background, soft vignette
Style/medium: AAA 3D product visualization for a polished desktop installer, precise materials, modern and understated
Composition/framing: wide 5:3; reserve two uncluttered circular landing zones at approximately 28 percent and 72 percent of width for installer icons; keep the center path visible but do not draw arrows; safe margins on every edge; must still work when cropped to 660x400
Lighting/mood: low-key cool studio lighting, thin cyan line from left toward center and restrained chartreuse line from center toward right, no bloom
Color palette: #171D25, #22262D, #2E3848, #0D6F9F, #5CAAC1, tiny #A4D007 accent
Materials/textures: powder-coated metal, brushed graphite, smoked glass, finely embossed topographic patterns
Constraints: background artwork only; no text, words, letters, numbers, logos, icons, arrows, app symbols, folders, game brands, Steam imagery, characters, watermarks; original IP; avoid busy focal points at the two landing zones; avoid high contrast behind installer labels
```
