-- Encargos que el agente repite solo.
--
-- Una frase y una cadencia: «cada domingo, sube a Backlog lo que lleve seis
-- meses sin tocar». Vindexa se la manda al agente cuando toca, con las mismas
-- herramientas y las mismas barreras que si se la dijera una persona.
--
-- Vive aquí y no en un programador de tareas genérico porque el encargo habla
-- de esta biblioteca: quien sabe de horarios no sabe qué es un estado ni qué
-- colecciones hay, y guardarlo fuera dejaría la frase en otro sitio, con otro
-- formato y sin el rastro de lo que hizo.
--
-- `last_result` guarda un resumen legible de la última pasada, también cuando
-- falla: un encargo que no funciona tiene que poder verse, no desaparecer.
CREATE TABLE agent_tasks (
    id TEXT PRIMARY KEY,
    instruction TEXT NOT NULL CHECK (length(trim(instruction)) BETWEEN 1 AND 500),
    cadence TEXT NOT NULL CHECK (cadence IN ('diaria', 'semanal', 'mensual')),
    enabled INTEGER NOT NULL DEFAULT 1 CHECK (enabled IN (0, 1)),
    last_run_at TEXT,
    last_result TEXT,
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);
