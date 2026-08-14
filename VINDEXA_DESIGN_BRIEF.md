# Vindexa — Documento maestro de diseño y producto

> Especificación funcional, visual y técnica de alto nivel. Este documento no contiene implementación ni código.

## 1. Visión del producto

Vindexa será una aplicación de escritorio destinada a importar, visualizar, clasificar, planificar y organizar una biblioteca personal de Steam. Su propósito es convertir una colección extensa de juegos en un sistema visual de decisión y seguimiento.

La aplicación debe ayudar a responder de manera inmediata:

- ¿Qué juego ahora?
- ¿Qué jugaré después?
- ¿Dónde dejé cada juego?
- ¿Qué tengo instalado?
- ¿Qué debería instalar o desinstalar?
- ¿Qué títulos están pendientes, pausados, abandonados o terminados?
- ¿Qué juegos quiero seguir de cerca?
- ¿Cuáles siguen en Early Access?
- ¿Qué juegos han recibido actualizaciones importantes?
- ¿Qué puedo jugar según el tiempo que tengo disponible?
- ¿Qué prioridad tiene cada juego?
- ¿Qué quiero terminar esta semana, este mes o este año?

Vindexa no debe ser una demostración superficial. El producto final debe utilizar datos reales, tener persistencia fiable, funcionar con bibliotecas grandes y estar preparado para mantenimiento y evolución a largo plazo.

## 2. Principios de producto

1. **Biblioteca primero:** toda función debe facilitar encontrar, priorizar, continuar o clasificar un juego.
2. **Control personal:** Steam aporta los datos, pero el usuario decide estados, orden, etiquetas, colecciones y planificación.
3. **Información densa y clara:** mostrar muchos juegos y metadatos sin convertir la interfaz en un dashboard genérico.
4. **Persistencia fiable:** ninguna decisión personal debe perderse durante una sincronización.
5. **Acciones reales:** no habrá botones decorativos ni datos falsos en el flujo final.
6. **Privacidad y seguridad:** nunca se solicitará ni almacenará la contraseña de Steam.
7. **Rendimiento de escritorio:** la experiencia debe seguir siendo rápida con miles de juegos.
8. **Diseño verificable:** las decisiones visuales se contrastarán con la referencia y con capturas en varios tamaños.

## 3. Referencia visual obligatoria

La referencia principal es la captura de Steam situada en:

`/home/adrian/.codex/attachments/8e49e65b-d811-4808-937d-f4f19e3baf1a/image-1.png`

La interfaz debe reproducir fielmente:

- La densidad visual de Steam.
- La navegación superior oscura y compacta.
- La biblioteca lateral persistente.
- La cuadrícula de portadas.
- Los paneles y divisores discretos.
- La tipografía compacta.
- El uso contenido del azul como acento.
- Los estados de selección, foco y puntero.
- La sensación de aplicación nativa de escritorio.
- La cantidad de información visible sin resultar caótica.

La referencia debe reinterpretarse con una identidad propia de Vindexa, sin copiar logotipos ni elementos protegidos de Steam.

### 3.1. Estéticas que deben evitarse

- Dashboard SaaS genérico.
- Tarjetas desproporcionadamente grandes.
- Gradientes morados o estética de IA genérica.
- Glassmorphism excesivo.
- Bordes excesivamente redondeados.
- Grandes superficies vacías sin función.
- Tipografía sobredimensionada.
- Decoración que reduzca la densidad útil.
- Animaciones innecesarias o lentas.

## 4. Proceso de diseño y desarrollo

El producto se abordará en fases y no se implementará completamente antes de validar su dirección:

1. Auditar el entorno y el repositorio.
2. Comprobar las versiones estables actuales de las tecnologías.
3. Revisar vulnerabilidades, avisos de seguridad y compatibilidad.
4. Analizar detalladamente la referencia visual.
5. Elaborar la especificación funcional.
6. Definir la arquitectura y el modelo de datos.
7. Crear el sistema de diseño.
8. Elaborar wireframes y mockups de alta fidelidad.
9. Presentar las pantallas principales para aprobación.
10. Implementar el producto por fases después de la aprobación.
11. Verificar cada fase con pruebas y capturas.
12. Compilar, empaquetar e instalar solo la versión final validada.

Las decisiones importantes que afecten al alcance, seguridad, datos o experiencia deberán exponerse antes de aplicarse.

## 5. Plataforma y tecnología prevista

La solución se construirá con versiones estables, actuales y se deberá de verificar que todo sea lo más actual y compatibles de:

- Vite 8 o superior.
- React y TypeScript estricto.
- Rust estable.
- Tauri 2.
- SQLite.
- Una capa de acceso a datos Rust con migraciones versionadas.
- shadcn/ui y primitivas accesibles de Radix.
- Tailwind CSS.
- Motion para animaciones.
- Tabler Icons.
- dnd-kit para interacciones de arrastrar y soltar.
- Zod y react-hook-form para validación y formularios.
- TanStack Query para estado asíncrono.
- Zustand únicamente si existe una necesidad real de estado global.
- Vitest, React Testing Library y Playwright.
- Biome para formato y análisis estático.
- pnpm como gestor de paquetes.

Las dependencias deberán estar justificadas y fijadas en versiones reproducibles. La versión final no utilizará rangos indiscriminados equivalentes a “latest”.

## 6. Arquitectura conceptual

La solución separará claramente:

- Dominio.
- Casos de uso.
- Infraestructura.
- Persistencia.
- Integración con Steam.
- Estado de interfaz.
- Componentes de presentación.
- Sistema de diseño.

La interfaz no conocerá SQL ni detalles internos de red. La comunicación con Rust utilizará operaciones pequeñas, tipadas y validadas. Se evitarán archivos monolíticos y componentes que concentren responsabilidades no relacionadas.

### 6.1. Áreas funcionales del frontend

- Aplicación y navegación.
- Biblioteca.
- Planificador.
- Colecciones.
- Integración con Steam.
- Seguimiento.
- Ajustes.
- Componentes visuales reutilizables.
- Disposición y navegación global.
- Estado y utilidades compartidas.

### 6.2. Áreas funcionales del backend

- Comandos de escritorio.
- Dominio.
- Repositorios.
- Servicios.
- Integración con Steam.
- Base de datos.
- Migraciones.
- Gestión de errores.

## 7. Integración con Steam

La vinculación debe ser real, oficial y segura.

### 7.1. Requisitos

- Utilizar el flujo oficial aplicable de Steam OpenID para identificar la cuenta.
- Abrir la autenticación oficial en el navegador externo.
- Validar correctamente la respuesta recibida.
- Obtener y guardar el SteamID64.
- No pedir ni almacenar la contraseña de Steam.
- No mostrar una página de autenticación falsa o imitada.
- Explicar claramente si Steam exige una Web API Key para determinadas funciones.
- Guardar secretos mediante el almacén seguro del sistema cuando sea imprescindible.
- Permitir desvincular la cuenta.
- Ofrecer sincronización manual y periódica.
- Mostrar la fecha y el resultado de la última sincronización.
- Manejar bibliotecas privadas, errores de API y límites de peticiones.
- Nunca eliminar metadatos personales al resincronizar.

### 7.2. Datos que se importarán cuando Steam los exponga

- App ID.
- Nombre.
- Tiempo total jugado.
- Tiempo jugado recientemente.
- Iconos, cabeceras y portadas.
- Logros desbloqueados y totales.
- Porcentaje de logros.
- Última fecha de juego.
- Juegos gratuitos.
- Información de préstamo o uso compartido cuando pueda identificarse.

Las imágenes procederán de fuentes oficiales de Steam y dispondrán de caché local.

## 8. Modelo de información

### 8.1. Cuenta de Steam

- SteamID64.
- Nombre visible.
- Avatar.
- URL del perfil.
- Visibilidad.
- Última sincronización.

### 8.2. Juego

- App ID.
- Título.
- Portada, cabecera e icono.
- Tiempo total y reciente.
- Última vez jugado.
- Fecha de lanzamiento.
- Desarrollador y editor.
- Géneros y categorías de Steam.
- Estado de Early Access.
- Compatibilidad con Steam Deck.
- Logros desbloqueados y totales.
- Fechas de importación y actualización.

### 8.3. Organización personal del juego

- Estado personal.
- Progreso.
- Prioridad.
- Instalado.
- Fijado.
- En seguimiento.
- Valoración personal.
- Duración estimada.
- Fecha objetivo.
- Próxima acción.
- Último punto o checkpoint.
- Notas privadas.
- Fecha de inicio, finalización o abandono.

### 8.4. Colección

- Identificador.
- Nombre.
- Descripción.
- Color.
- Icono.
- Tipo manual o inteligente.
- Posición.

### 8.5. Relación entre colecciones y juegos

- Colección.
- Juego.
- Posición manual.

### 8.6. Etiquetas

- Nombre.
- Color.
- Asociación con uno o varios juegos.

### 8.7. Reglas inteligentes

- Colección relacionada.
- Campo evaluado.
- Operador.
- Valor.
- Grupo lógico.
- Orden de evaluación.

### 8.8. Sesión de juego

- Juego.
- Inicio y final.
- Progreso antes y después.
- Nota de sesión.

### 8.9. Actividad

- Tipo.
- Juego relacionado.
- Mensaje.
- Fecha.

## 9. Persistencia

SQLite será la fuente de verdad en escritorio. El almacenamiento del navegador no se utilizará como persistencia principal.

La base de datos debe incluir:

- WAL.
- Foreign keys.
- Tiempo de espera ante bloqueos.
- Migraciones versionadas.
- Índices adecuados.
- Transacciones.
- Copias de seguridad.
- Exportación e importación.
- Recuperación ante corrupción.
- Pruebas de migraciones.

## 10. Estados de juego

Estados iniciales:

- Sin clasificar.
- Jugando ahora.
- Jugar después.
- Backlog.
- Pausado.
- Completado.
- Abandonado.
- Infinito o recurrente.
- Solo multijugador.
- Esperando actualización.
- Esperando salir de Early Access.

El usuario podrá crear, renombrar, recolorear y reordenar estados.

## 11. Biblioteca

La vista principal incluirá:

- Barra lateral compacta.
- Buscador instantáneo.
- Filtros combinables.
- Ordenación.
- Vista de cuadrícula.
- Vista de lista.
- Vista compacta.
- Portadas con carga progresiva.
- Virtualización para miles de juegos.
- Menú contextual.
- Selección múltiple.
- Acciones masivas.
- Navegación completa por teclado.
- Atajos configurables.

### 11.1. Filtros

- Estado.
- Instalación.
- Early Access.
- Sin jugar.
- Horas jugadas.
- Progreso.
- Valoración.
- Género.
- Etiqueta.
- Colección.
- Fecha de lanzamiento.
- Última vez jugado.
- Logros.
- Compatibilidad con Steam Deck.
- Seguimiento.
- Fecha objetivo.
- Tamaño de sesión.

### 11.2. Ordenaciones

- Prioridad manual.
- Orden alfabético.
- Horas jugadas.
- Última vez jugado.
- Fecha de lanzamiento.
- Progreso.
- Valoración.
- Fecha objetivo.
- Añadido recientemente.
- Orden aleatorio.

## 12. Ficha del juego

La ficha será un panel lateral o modal amplio y contextual, no un formulario genérico.

Debe incluir:

- Hero y portada.
- Horas jugadas.
- Logros.
- Última sesión.
- Estado.
- Progreso.
- Prioridad.
- Valoración personal.
- Géneros.
- Etiquetas.
- Colecciones.
- Estado de instalación.
- “Por dónde lo dejé”.
- “Qué tengo que hacer después”.
- Fecha objetivo.
- Duración restante estimada.
- Historial de sesiones.
- Notas.
- Enlace a Steam.
- Acción para ejecutar el juego.
- Acción para solicitar su instalación.
- Seguimiento de actualizaciones.
- Línea temporal personal.

El guardado automático o el estado de cambios pendientes debe ser inequívoco.

## 13. Arrastrar y soltar

El drag and drop será una función central y deberá permitir:

- Reordenar manualmente la biblioteca.
- Reordenar juegos dentro de una colección.
- Mover juegos entre estados.
- Arrastrar varios juegos simultáneamente.
- Mover juegos entre columnas del planificador.
- Añadir juegos a colecciones soltándolos en la barra lateral.
- Utilizar alternativas accesibles mediante teclado.
- Mostrar indicadores claros de destino.
- Deshacer la última operación.

Todas las posiciones deben persistirse en SQLite.

## 14. Planificador

Vista Kanban con columnas configurables:

- Jugando ahora.
- A continuación.
- Este mes.
- Más adelante.
- Pausados.
- Terminados.

Funciones:

- Crear y reordenar columnas.
- Arrastrar juegos entre columnas.
- Definir un límite de juegos activos.
- Añadir fechas objetivo.
- Mostrar horas estimadas.
- Detectar sobrecarga del plan.
- Crear una cola lineal.
- Elaborar planes semanales y mensuales.
- Marcar objetivos.
- Mostrar progreso agregado.
y otras que se te ocurran

## 15. Colecciones inteligentes

Debe existir un constructor visual de reglas con condiciones AND y OR, además de una vista previa antes de guardar.

Colecciones iniciales sugeridas:

- Early Access.
- Instalados.
- Nunca jugados.
- Menos de dos horas.
- Sesiones cortas.
- Más de cincuenta horas.
- Sin jugar durante un año.
- Progreso superior al 75 %.
- En seguimiento.
- Próximos a su fecha objetivo.
- Compatibles con Steam Deck.
- Actualizados recientemente.
y otras que se te ocurran

Las colecciones manuales e inteligentes se distinguirán claramente.

## 16. Seguimiento y descubrimiento

- Lista de seguimiento.
- Cambios de Early Access.
- Actualizaciones importantes.
- Lanzamientos futuros relacionados.
- Recordatorios.
- Juegos olvidados.
- Juegos casi terminados.
- Recomendaciones basadas en datos propios.
- Selector de tiempo disponible: 30 minutos, una hora o dos horas.
- Selector de ánimo o tipo de experiencia.
- Acción “Elige por mí”.
- Historial de recomendaciones descartadas.

Las recomendaciones no presentarán información inventada como si fuera real.

## 17. Gestión de instalaciones

Cuando el entorno lo permita:

- Detectar bibliotecas locales de Steam.
- Detectar juegos instalados.
- Leer manifiestos oficiales de Steam.
- Calcular espacio ocupado.
- Identificar unidad y biblioteca.
- Abrir la carpeta correspondiente.
- Solicitar a Steam la instalación de un juego.
- Solicitar a Steam la desinstalación de un juego.
- Ejecutar un juego a través de Steam.
- Pedir confirmación antes de desinstalar.
- No modificar directamente archivos internos de Steam.

## 18. Configuración

- Cuenta de Steam.
- Sincronización.
- Apariencia.
- Densidad.
- Comportamiento.
- Atajos.
- Privacidad.
- Copias de seguridad.
- Importación y exportación.
- Ubicación de los datos.
- Diagnóstico.
- Acerca de.
- Búsqueda de actualizaciones.

## 19. Sistema de diseño

El sistema visual deberá documentar:

- Paleta.
- Tokens semánticos.
- Tipografía.
- Escala de espaciado.
- Radios.
- Bordes.
- Sombras.
- Niveles de densidad.
- Estados interactivos.
- Animaciones.
- Iconografía.
- Reglas de accesibilidad.

### 19.1. Accesibilidad

- Cumplimiento de WCAG 2.2 AA.
- Contraste mínimo de 4.5:1.
- Foco siempre visible.
- Navegación completa por teclado.
- Respeto a la reducción de movimiento.
- La información no dependerá únicamente del color.
- Tooltips en acciones sin texto.
- Áreas interactivas adecuadas.
- Textos en español de España.
- Adaptación a ventanas pequeñas y grandes.

## 20. Animaciones y microinteracciones

Motion.dev se utilizará con moderación para:

- Entrada escalonada inicial.
- Transiciones entre vistas.
- Animación de reordenación.
- Overlay durante el arrastre.
- Apertura y cierre de paneles.
- Confirmación de guardado.
- Feedback de botones y filtros.
- Skeletons durante la sincronización.

No se añadirán animaciones puramente decorativas que ralenticen la biblioteca.

## 21. Recursos gráficos

Los recursos originales necesarios se crearán con GPT Imagegen 2:

- Icono de Vindexa.
- Fondo sutil de bienvenida.
- Ilustración para una biblioteca todavía no vinculada.
- Arte del instalador.
- Variantes en los tamaños requeridos.

No se generarán portadas falsas para juegos reales. Todos los recursos finales se guardarán dentro del proyecto.

## 22. Rendimiento

La aplicación debe manejar al menos 5.000 juegos mediante:

- Virtualización.
- Consultas paginadas.
- Índices SQLite.
- Caché de imágenes.
- Búsqueda con debounce.
- Carga diferida.
- Reducción de renderizados innecesarios.
- Operaciones Rust asíncronas.
- Ausencia de bloqueos en el hilo principal.
- Medición explícita del rendimiento.

## 23. Seguridad

Antes de incorporar dependencias se revisarán versiones, vulnerabilidades y avisos de seguridad. Además:

- CSP restrictiva.
- Capacidades Tauri mínimas.
- Validación de todas las operaciones nativas.
- Ningún secreto en logs.
- Ningún secreto en almacenamiento web.
- Validación de URLs externas.
- Consultas SQL parametrizadas.
- Ausencia de interpolación SQL insegura.
- Dependencias fijadas.
- Actualizaciones firmadas si se añade actualización automática.
- Registro de riesgos aceptados.

## 24. Estrategia de pruebas

### 24.1. Rust

- Dominio.
- Repositorios.
- Migraciones.
- Reglas inteligentes.
- Importación de Steam mediante respuestas simuladas.

### 24.2. Frontend

- Componentes.
- Filtros.
- Formularios.
- Estado.
- Drag and drop.
- Carga, vacío y error.

### 24.3. Pruebas de extremo a extremo

- Primera ejecución.
- Vinculación de cuenta.
- Importación de biblioteca.
- Creación de colección.
- Edición de progreso.
- Guardado de checkpoint.
- Movimiento de un juego.
- Reinicio y comprobación de persistencia.
- Desvinculación.
- Exportación de copia de seguridad.

### 24.4. Pruebas visuales

- Comparaciones visuales.
- Detección de desbordamientos.
- Tema oscuro.
- Reducción de movimiento.

No se afirmará que una función está terminada sin evidencia directa.


### 25.2. Instalador personalizado

- Identidad visual propia.
- Comprobación de requisitos.
- Confirmación antes de sustituir otra versión.
- Conservación de datos personales.
- Verificación del checksum.
- Resultado claramente comunicado.
- Desinstalación limpia.
- Ninguna modificación del sistema base inmutable.

### 25.3. Verificación posterior a la instalación

- Aparición en el menú de aplicaciones.
- Inicio correcto.
- Creación de la base de datos.
- Persistencia después de reiniciar.
- Apertura de URLs de Steam.
- Icono correcto.
- Ausencia de bibliotecas dinámicas faltantes.

## 26. Documentación que deberá acompañar al producto

- README.
- Sistema de diseño.
- Arquitectura.
- Seguridad.
- Guía de contribución.
- Manual de usuario.
- Guía de compilación.
- Guía de instalación.
- Esquema de base de datos.
- Registro de decisiones arquitectónicas.
- Changelog.
- Licencia.

## 27. Criterios de finalización

La aplicación no se considerará terminada únicamente porque compile. Debe demostrarse que:

- Todos los requisitos explícitos están implementados.
- No quedan botones decorativos sin comportamiento.
- No existen datos simulados en el flujo final.
- Steam se vincula de manera real.
- SQLite es la fuente de verdad.
- La organización sobrevive al reinicio.
- El drag and drop persiste correctamente.
- Las colecciones inteligentes funcionan.
- Las pruebas pasan.
- La interfaz ha sido comparada visualmente con la referencia.
- Los instaladores han sido generados.
- La versión final se ha instalado y probado en Bazzite.
- Existe evidencia verificable para cada requisito.

Las limitaciones reales de Steam, Tauri o del sistema deberán documentarse junto con la alternativa técnicamente correcta.

## 28. Primera fase autorizada

La primera fase queda limitada a:

jamás usar datos mock ni demo bajo ningún concepto.

No se iniciará la implementación hasta obtener aprobación expresa de esta fase.
