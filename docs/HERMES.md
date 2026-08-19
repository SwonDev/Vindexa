# Conectar Hermes con Vindexa

Plan de la integración. **Nada de esto está construido todavía**: es el diseño
acordado antes de escribir código, para que cuando se escriba no haya que
decidir nada a mitad de camino.

Lo que ya existe y en lo que se apoya todo esto está en
[`AGENT_BRIDGE.md`](AGENT_BRIDGE.md): un puente en Rust con dieciocho
intenciones tipadas, permisos por ámbito, confirmación humana de lo destructivo,
deshacer y registro de auditoría; sus once comandos ya expuestos; y la pantalla
de Ajustes → Agentes que emite los testigos.

## Qué se quiere poder hacer

Mandarle un mensaje a Hermes —desde el ordenador o desde el móvil, escrito o
hablado— y que Vindexa quede ordenada:

> «he estado dos horas con DragonSword Awakening y voy por el 40 % de la
> historia»

> «Hollow Knight ya me lo pasé, pero seguiré jugando: bájale la prioridad»

Ninguna de esas dos frases llega a Vindexa. Llega su traducción:
`{"intent": "registrar_sesion", "juego": {"nombre": "DragonSword Awakening"},
"minutos": 120, "progreso": 40}`. Quién la traduce, con qué modelo y desde qué
aplicación de mensajería es asunto del agente; el contrato de Vindexa es JSON
tipado y no cambia.

## La forma, y por qué es ésta

```
Telegram ──┐
           ├──► Hermes ──► proceso acompañante ──► comando Tauri ──► puente ──► SQLite
micrófono ─┘   (modelo)     (lo lanza Vindexa)
```

**Vindexa no habla con Telegram, ni con ningún servicio de mensajería, ni
invoca ningún modelo.** No es una limitación pendiente de resolver: es la
frontera que mantiene local a la aplicación. El puente hacia fuera es del
agente, que corre fuera.

**No hay puerto de escucha.** Ni siquiera en `127.0.0.1`: un puerto es una
puerta abierta a cualquier proceso local del usuario y a cualquier página web
capaz de llamar al bucle local. Un socket de dominio Unix con permisos `0600`
bajo el directorio de datos sería aceptable si algún día hiciera falta, pero la
ruta prevista no lo necesita.

## La pieza que falta: el proceso acompañante

Un ejecutable que Vindexa lanza como hijo y con el que habla por entrada y
salida estándar. Ni red, ni ficheros compartidos, ni puertos.

- **Vindexa lo lanza y lo supervisa.** Si muere, se reinicia con espera
  creciente; si no arranca, la pantalla de Agentes lo dice en vez de fingir que
  está conectado.
- **Habla líneas de JSON.** Una petición por línea, una respuesta por línea. El
  sobre es el mismo que ya define `AGENT_BRIDGE.md`, testigo incluido: el
  proceso acompañante no es de confianza por ser hijo.
- **Sin privilegios extra.** Pasa por el mismo puente, con el mismo testigo y
  los mismos ámbitos que cualquier agente. Que lo lance Vindexa no le da
  permisos: sólo le da un canal.
- **Su ruta se configura, no se adivina.** Ajustes → Agentes tendrá el campo, y
  mientras esté vacío no se lanza nada.

## Telegram, audio y modelos: del lado del agente

- **Telegram.** El bot es de Hermes. Quien controle ese bot puede pedir lo que
  el testigo permita, así que conviene que ese testigo lleve los ámbitos justos
  —registrar sesiones y cambiar estados basta para el caso de uso— y que lo
  destructivo siga esperando confirmación en Vindexa.
- **Audio.** La transcripción ocurre antes de llegar aquí. Vindexa no captura
  micrófono ni transcribe nada.
- **Modelos locales o de API.** Da igual cuál interprete la frase: el puente
  recibe JSON. Un modelo local es preferible por coherencia con el proyecto,
  pero no es una condición del contrato.

## Qué hay que construir, en orden

1. **Lanzador supervisado** del proceso acompañante, con su ruta configurable y
   su estado visible en Ajustes → Agentes.
2. **Protocolo de líneas JSON** sobre entrada y salida estándar, con tope de
   tamaño por línea y tiempo máximo por petición.
3. **Adaptador de Hermes**: la capa fina que traduce entre su convención de
   llamada a herramientas y el sobre de `AGENT_BRIDGE.md`. Vive en el lado del
   agente, no aquí.
4. **Confirmaciones a la vista.** Lo destructivo ya espera aprobación; falta
   que la interfaz avise de que hay algo esperando, en vez de tener que abrir
   Ajustes a mirar.

## Lo que no se va a hacer

- Abrir un puerto TCP, aunque sea local.
- Que Vindexa hable con Telegram, con un modelo o con cualquier servicio remoto
  que no sea la tienda de la que ya lee datos.
- Dar al proceso acompañante permisos que no tenga cualquier otro agente.
- Dejar que un agente apruebe sus propias acciones destructivas.
