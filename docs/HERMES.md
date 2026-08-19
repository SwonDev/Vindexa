# Conducir Vindexa hablando

Mandarle un mensaje a un agente —desde el ordenador o desde el móvil, escrito o
hablado— y que la biblioteca quede ordenada:

> «he estado dos horas con DragonSword Awakening y voy por el 40 % de la
> historia»

> «Hollow Knight ya me lo pasé, pero seguiré jugando: bájale la prioridad»

Ninguna de esas frases llega a Vindexa. Llega su traducción, que es JSON tipado:
`{"intent": "registrar_sesion", "game": {"name": "DragonSword Awakening"},
"minutes": 120, "progress": 40}`. Quién la traduce, con qué modelo y desde qué
aplicación de mensajería es asunto del agente; el contrato de Vindexa no cambia.

Esto **está construido y funcionando**. Lo que sigue describe cómo, por dónde
entra cada cosa y qué decisiones se tomaron y por qué.

## Las dos mitades

| | Agente de fuera | Agente de casa |
|---|---|---|
| Quién es | Hermes, Claude Code, cualquiera que hable MCP | El que trae Vindexa |
| Dónde se le habla | Donde ese agente ya viva: su ventana, Telegram, WhatsApp | En el pie de Vindexa |
| Qué modelo usa | El suyo | Uno que ya esté sirviendo en el bucle local |
| Cómo se conecta | Solo, al arrancar Vindexa | No hace falta conectarlo |
| Código | `src-tauri/src/mcp/`, `src-tauri/src/agent/{hosts,autolink}.rs` | `src-tauri/src/vindagent/` |

Las dos usan **el mismo puente** ([`AGENT_BRIDGE.md`](AGENT_BRIDGE.md)): las
mismas intenciones, los mismos ámbitos, el mismo límite de frecuencia, la misma
auditoría y el mismo deshacer. Ser el agente de casa no da ningún atajo.

## Cómo entra un agente de fuera

Vindexa habla [MCP](https://modelcontextprotocol.io). El agente arranca

```
/Applications/Vindexa.app/Contents/MacOS/vindexa mcp
```

y le habla por la entrada estándar. Diecinueve herramientas, una por intención,
más `deshacer`.

### Por qué no hay un puerto

Un socket en `127.0.0.1` está abierto para **cualquier** proceso local y para
cualquier página web capaz de hacerle una petición: sería una frontera de
confianza nueva y peor que la que hay. Aquí el transporte es la tubería que abre
quien lanza el proceso, y sólo quien lo lanza puede hablar con él. Encima hace
falta un testigo válido, que se emite en Ajustes y se puede revocar.

### El alta es automática

Al arrancar, `agent::autolink` mira qué agentes compatibles hay instalados,
emite un testigo, da de alta el servidor y comprueba que ha entrado. Si Vindexa
cambia de sitio o el testigo se revoca, lo rehace en el siguiente arranque. Se
puede apagar desde Ajustes → Agentes.

Dos cosas que costó ver y que ahora sujetan pruebas:

- **Salir con código cero no es haber entrado.** `hermes mcp add` pregunta si
  habilita las herramientas descubiertas; sin nadie al otro lado, la pregunta se
  cancela sola y el alta se pierde en silencio con estado de éxito. Se le
  contesta, y después se comprueba que el servidor aparezca en su lista.
- **Un testigo emitido y no entregado se revoca.** Si el alta falla, la
  credencial no puede quedarse viva.

## Cómo funciona el agente de casa

`vindagent::chat` busca un servidor de inferencia en el bucle local —llama.cpp,
LM Studio, Ollama—, le pasa las herramientas en el formato de OpenAI y ejecuta
lo que pida contra el puente, hasta ocho vueltas por turno. La respuesta vuelve
con **los pasos que dio**: qué llamó y con qué. Un agente que contesta «hecho»
te obliga a ir a comprobarlo.

Sólo habla con `127.0.0.1`. La comprobación analiza la URL en vez de mirar cómo
empieza, porque `http://127.0.0.1:8080@servidor.ajeno.tld/` empieza por
`http://127.0.0.1:` y viaja a otro sitio.

Si no hay ni motor ni modelo, Ajustes → Agentes enseña qué falta: la orden
exacta que instalaría llama.cpp con el gestor de paquetes del sistema —un botón,
no algo que ocurra solo— y qué modelos le caben a la máquina, preguntados a
Hugging Face en el momento.

## Lo que un agente no puede hacer

1. **Aprobar lo suyo.** Lo que exige confirmación humana espera dentro de
   Vindexa. Un agente no confirma sus propias acciones destructivas.
2. **Ampliar sus permisos.** Los ámbitos son los del testigo.
3. **Inventarse un juego.** Ante un nombre ambiguo devuelve las opciones y
   espera; ante un AppID que no es de ese juego, se niega diciendo de quién es.
4. **Borrar la biblioteca.** No hay ninguna intención que borre juegos.

## Lo que queda fuera, a propósito

- **Telegram, WhatsApp y la voz** son del agente, no de Vindexa. Hermes ya los
  tiene; añadir aquí una segunda implementación sería mantener dos.
- **Las tareas programadas** también: quien sabe de horarios es el agente, que
  ya trae su propio programador.
