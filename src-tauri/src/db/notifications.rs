//! Avisos programados por la persona usuaria y eventos oficiales derivados de
//! señales que Vindexa ya tenía persistidas.
//!
//! La migración 023 aporta dos tablas y este módulo es su única puerta:
//!
//! - `notification_rules`: lo que la persona usuaria programa. Una regla dice
//!   *cuándo* avisar, no *qué ocurrió*.
//! - `notification_events`: la bandeja. Cada fila es un aviso ya materializado,
//!   con severidad, momento y una `dedupe_key` estable.
//!
//! ## Invariantes
//!
//! 1. **Nunca se inventa un evento.** Todo evento oficial nace de una fila que
//!    ya existía en `discovery_events`, `steam_news_items`, `game_reminders` o
//!    `game_dlc`. Si la señal oficial no existe, no hay evento: este módulo no
//!    consulta la red, no deduce fechas y no rellena huecos. Una tabla de
//!    origen vacía produce un informe con ceros, nunca un aviso plausible.
//! 2. **Derivar es idempotente.** La `dedupe_key` es determinista y el índice
//!    único parcial de la migración 023 la respalda: ejecutar la derivación dos
//!    veces sobre las mismas señales no crea una segunda fila.
//! 3. **Una regla no se dispara dos veces por la misma cita.** `last_fired_at`
//!    se compara siempre contra el instante de aviso vigente
//!    (`scheduled_for - lead_minutes`), no contra «ahora».
//! 4. **La recurrencia no desborda el calendario.** El paso mensual usa
//!    `chrono::Months`, que recorta al último día del mes destino: el 31 de
//!    enero pasa al 28 (o 29) de febrero y el 31 de diciembre al 31 de enero
//!    del año siguiente.
//!
//! ## `scheduled_for` es el ancla, no la próxima cita
//!
//! En una regla periódica `scheduled_for` guarda **la primera cita elegida por
//! la persona usuaria y no se reescribe nunca**. Las citas posteriores se
//! calculan siempre como `ancla + N periodos`, y el recorte de fin de mes de
//! `chrono::Months` se aplica desde el ancla en cada salto:
//!
//! ```text
//! ancla 31/01  ->  28/02  ->  31/03  ->  30/04  ->  31/05
//! ancla 30/01  ->  28/02  ->  30/03  ->  30/04
//! ```
//!
//! Guardar en su lugar la cita ya recortada arrastraría el día para siempre
//! (31 → 28 → 28 → 28). Como la migración 023 solo ofrece una columna de fecha,
//! la única forma de conservar el día que la persona eligió es que esa columna
//! sea el ancla. El progreso vive en `last_fired_at`, y la próxima cita viaja
//! calculada hacia la interfaz en `nextOccurrence`.

use crate::error::{AppError, AppResult};
use chrono::{DateTime, Datelike, Days, Months, SecondsFormat, TimeDelta, Utc};
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Tipos admitidos por el `CHECK` de `notification_rules.kind`.
pub const NOTIFICATION_KINDS: [&str; 6] = [
    "manual",
    "release_date",
    "early_access_exit",
    "official_news",
    "dlc_release",
    "reminder_digest",
];
/// Recurrencias admitidas por el `CHECK` de `notification_rules.repeat_rule`.
pub const REPEAT_RULES: [&str; 4] = ["none", "daily", "weekly", "monthly"];
/// Severidades admitidas por el `CHECK` de `notification_events.severity`.
pub const NOTIFICATION_SEVERITIES: [&str; 4] = ["info", "success", "warning", "critical"];

const MAX_TITLE_CHARS: usize = 120;
const MAX_BODY_CHARS: usize = 2_000;
const MAX_EVENT_TITLE_CHARS: usize = 200;
const MAX_EVENT_BODY_CHARS: usize = 600;
/// Treinta días. Un margen mayor que esto convierte el aviso en otra cita.
const MAX_LEAD_MINUTES: u32 = 30 * 24 * 60;
/// Techo de reglas que Vindexa conserva. Protege la bandeja de un bucle de
/// creación accidental desde la interfaz.
const MAX_RULES: i64 = 500;
/// Reglas que un solo barrido puede disparar.
const DUE_RULES_LIMIT: usize = 200;
/// Eventos que una sola derivación crea por cada fuente oficial. El filtro
/// `NOT EXISTS` garantiza que el siguiente barrido continúe donde acabó este.
const DERIVATION_BATCH: usize = 200;
/// Cien años de saltos mensuales. Un valor mayor solo puede venir de una fecha
/// corrupta, y el bucle debe terminar igualmente.
const MAX_RECURRENCE_STEPS: u32 = 1_200;
/// Filas que la bandeja devuelve como máximo en una página.
const MAX_INBOX_LIMIT: u32 = 200;
const DEFAULT_INBOX_LIMIT: u32 = 50;
/// Retención máxima admitida en la purga: diez años.
#[allow(dead_code, reason = "límite de prune_events, todavía sin llamador")]
const MAX_RETENTION_DAYS: u32 = 3_650;

// ---------------------------------------------------------------------------
// Modelos serializados hacia la interfaz
// ---------------------------------------------------------------------------

/// Una regla tal y como la lee la interfaz. `game_title` viene del `LEFT JOIN`
/// con `games`: es `None` cuando la regla no está atada a ningún juego.
///
/// `scheduled_for` es el **ancla** (la primera cita). `current_occurrence` y
/// `next_occurrence` no son columnas: se calculan a partir del ancla y del
/// instante consultado, de modo que la interfaz puede enseñar «la próxima vez»
/// sin que el backend pierda el día original.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NotificationRule {
    pub id: String,
    pub app_id: Option<u32>,
    pub game_title: Option<String>,
    pub kind: String,
    pub title: String,
    pub body: String,
    pub scheduled_for: Option<String>,
    pub repeat_rule: String,
    pub lead_minutes: u32,
    pub enabled: bool,
    pub last_fired_at: Option<String>,
    /// Última cita cuyo aviso ya venció en el instante consultado.
    pub current_occurrence: Option<String>,
    /// Primera cita cuyo aviso todavía no ha vencido. `None` cuando una regla
    /// puntual ya se consumió.
    pub next_occurrence: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

/// Entrada de creación o edición. `id` ausente crea; `id` presente exige que la
/// regla siga existiendo.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveNotificationRuleInput {
    pub id: Option<String>,
    pub app_id: Option<u32>,
    pub kind: String,
    pub title: String,
    #[serde(default)]
    pub body: String,
    pub scheduled_for: Option<String>,
    pub repeat_rule: String,
    #[serde(default)]
    pub lead_minutes: u32,
    pub enabled: bool,
}

/// Una regla que toca disparar: la cita vencida y el instante exacto en que su
/// aviso venció (`occurrence - lead_minutes`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DueRule {
    pub rule: NotificationRule,
    pub occurrence: String,
    pub due_at: String,
}

/// Un aviso ya materializado en la bandeja.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NotificationEvent {
    pub id: String,
    pub rule_id: Option<String>,
    pub app_id: Option<u32>,
    pub game_title: Option<String>,
    pub kind: String,
    pub severity: String,
    pub title: String,
    pub body: String,
    pub occurred_at: String,
    pub read_at: Option<String>,
    pub dismissed_at: Option<String>,
    pub dedupe_key: Option<String>,
}

/// Contadores de avisos sin leer y sin descartar. Son globales a propósito: la
/// interfaz los usa como distintivo y no debe cambiar al aplicar un filtro.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NotificationCounters {
    pub total: u32,
    pub info: u32,
    pub success: u32,
    pub warning: u32,
    pub critical: u32,
}

/// Filtro de la bandeja. Ningún campo se interpola en SQL: `scope` y `severity`
/// se resuelven contra una allowlist interna y `app_id` viaja como parámetro.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NotificationInboxFilter {
    pub scope: Option<String>,
    pub severity: Option<String>,
    pub app_id: Option<u32>,
}

/// Página de la bandeja más los contadores de no leídos.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NotificationInbox {
    pub items: Vec<NotificationEvent>,
    pub total: i64,
    pub limit: u32,
    pub offset: u32,
    pub unread: NotificationCounters,
}

/// Desglose de lo que produjo una derivación. `skipped_duplicates` cuenta las
/// señales que ya tenían un aviso y por tanto no crearon nada.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DerivedEventsReport {
    pub early_access_exits: u32,
    pub release_date_changes: u32,
    pub official_news: u32,
    pub due_reminders: u32,
    pub new_dlc: u32,
    pub created: u32,
    pub skipped_duplicates: u32,
}

/// Resultado de un barrido completo: reglas disparadas más señales derivadas.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NotificationRefreshReport {
    pub scheduled_events: u32,
    pub derived: DerivedEventsReport,
    pub unread: NotificationCounters,
}

// ---------------------------------------------------------------------------
// Allowlists internas
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum NotificationKind {
    Manual,
    ReleaseDate,
    EarlyAccessExit,
    OfficialNews,
    DlcRelease,
    ReminderDigest,
}

impl NotificationKind {
    fn parse(value: &str) -> AppResult<Self> {
        match value {
            "manual" => Ok(Self::Manual),
            "release_date" => Ok(Self::ReleaseDate),
            "early_access_exit" => Ok(Self::EarlyAccessExit),
            "official_news" => Ok(Self::OfficialNews),
            "dlc_release" => Ok(Self::DlcRelease),
            "reminder_digest" => Ok(Self::ReminderDigest),
            _ => Err(AppError::validation(format!(
                "El tipo de aviso no es válido. Usa uno de: {}.",
                NOTIFICATION_KINDS.join(", ")
            ))),
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::Manual => "manual",
            Self::ReleaseDate => "release_date",
            Self::EarlyAccessExit => "early_access_exit",
            Self::OfficialNews => "official_news",
            Self::DlcRelease => "dlc_release",
            Self::ReminderDigest => "reminder_digest",
        }
    }

    /// Un aviso sobre lanzamiento, salida de acceso anticipado, publicación
    /// oficial o DLC habla siempre de un juego concreto.
    fn requires_game(self) -> bool {
        matches!(
            self,
            Self::ReleaseDate | Self::EarlyAccessExit | Self::OfficialNews | Self::DlcRelease
        )
    }

    fn default_severity(self) -> &'static str {
        match self {
            Self::EarlyAccessExit => "success",
            Self::ReminderDigest => "warning",
            Self::Manual | Self::ReleaseDate | Self::OfficialNews | Self::DlcRelease => "info",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RepeatRule {
    None,
    Daily,
    Weekly,
    Monthly,
}

impl RepeatRule {
    fn parse(value: &str) -> AppResult<Self> {
        match value {
            "none" => Ok(Self::None),
            "daily" => Ok(Self::Daily),
            "weekly" => Ok(Self::Weekly),
            "monthly" => Ok(Self::Monthly),
            _ => Err(AppError::validation(format!(
                "La repetición del aviso no es válida. Usa una de: {}.",
                REPEAT_RULES.join(", ")
            ))),
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Daily => "daily",
            Self::Weekly => "weekly",
            Self::Monthly => "monthly",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum InboxScope {
    /// Todo lo que no se ha descartado. Es la vista por defecto.
    #[default]
    Pending,
    /// Pendiente y sin leer.
    Unread,
    /// Solo lo descartado.
    Dismissed,
    /// Todo, incluido lo descartado.
    All,
}

impl InboxScope {
    fn parse(value: Option<&str>) -> AppResult<Self> {
        match value.map(str::trim).filter(|value| !value.is_empty()) {
            None | Some("pending") => Ok(Self::Pending),
            Some("unread") => Ok(Self::Unread),
            Some("dismissed") => Ok(Self::Dismissed),
            Some("all") => Ok(Self::All),
            Some(_) => Err(AppError::validation(
                "El ámbito de la bandeja no es válido. Usa: pending, unread, dismissed o all.",
            )),
        }
    }

    fn clause(self) -> &'static str {
        match self {
            Self::Pending => "e.dismissed_at IS NULL",
            Self::Unread => "e.dismissed_at IS NULL AND e.read_at IS NULL",
            Self::Dismissed => "e.dismissed_at IS NOT NULL",
            Self::All => "1 = 1",
        }
    }
}

// ---------------------------------------------------------------------------
// Reglas programables
// ---------------------------------------------------------------------------

const RULE_COLUMNS: &str = "r.id, r.app_id, g.title, r.kind, r.title, r.body, r.scheduled_for,
     r.repeat_rule, r.lead_minutes, r.enabled, r.last_fired_at, r.created_at, r.updated_at";

/// Lista las reglas. `app_id = Some(_)` acota a un juego; `None` devuelve todas,
/// incluidas las globales. `now` solo sirve para calcular las citas derivadas.
pub fn list_rules(
    connection: &Connection,
    app_id: Option<u32>,
    now: DateTime<Utc>,
) -> AppResult<Vec<NotificationRule>> {
    if app_id == Some(0) {
        return Err(AppError::validation("El juego del aviso no es válido."));
    }
    let mut statement = connection.prepare(&format!(
        "SELECT {RULE_COLUMNS}
           FROM notification_rules r
           LEFT JOIN games g ON g.app_id = r.app_id
          WHERE (?1 = 0 OR r.app_id = ?1)
          ORDER BY r.enabled DESC,
                   r.scheduled_for IS NULL ASC,
                   datetime(r.scheduled_for) ASC,
                   r.title COLLATE NOCASE ASC,
                   r.id ASC"
    ))?;
    let mut rules = statement
        .query_map([app_id.unwrap_or(0)], map_rule)?
        .collect::<Result<Vec<_>, _>>()?;
    for rule in &mut rules {
        enrich_rule(rule, now)?;
    }
    Ok(rules)
}

/// Crea o actualiza una regla. La validación es estricta y devuelve siempre un
/// mensaje que dice qué corregir.
pub fn save_rule(
    connection: &mut Connection,
    input: &SaveNotificationRuleInput,
    now: DateTime<Utc>,
) -> AppResult<NotificationRule> {
    let validated = validate_rule_input(input)?;
    let transaction = connection.transaction()?;

    if let Some(app_id) = validated.app_id {
        let exists = transaction
            .query_row(
                "SELECT 1 FROM games WHERE app_id = ?1",
                [app_id],
                |_| Ok(()),
            )
            .optional()?
            .is_some();
        if !exists {
            return Err(AppError::not_found(
                "El juego al que apunta el aviso ya no está en la biblioteca.",
            ));
        }
    }

    let id = match input.id.as_deref().map(str::trim) {
        Some(raw) if !raw.is_empty() => {
            validate_id(raw, "El identificador del aviso no es válido.")?;
            // Cambiar el ancla o la recurrencia reinicia el progreso: la
            // persona usuaria acaba de reprogramar el aviso y espera que la
            // nueva agenda se cumpla. La `dedupe_key` sigue impidiendo que una
            // cita que ya generó aviso lo genere otra vez.
            let changed = transaction.execute(
                "UPDATE notification_rules
                    SET app_id = ?2,
                        kind = ?3,
                        title = ?4,
                        body = ?5,
                        last_fired_at = CASE
                            WHEN scheduled_for IS NOT ?6 OR repeat_rule <> ?7
                            THEN NULL ELSE last_fired_at
                        END,
                        scheduled_for = ?6,
                        repeat_rule = ?7,
                        lead_minutes = ?8,
                        enabled = ?9,
                        updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
                  WHERE id = ?1",
                params![
                    raw,
                    validated.app_id,
                    validated.kind.as_str(),
                    validated.title,
                    validated.body,
                    validated.scheduled_for,
                    validated.repeat.as_str(),
                    validated.lead_minutes,
                    validated.enabled,
                ],
            )?;
            if changed == 0 {
                return Err(AppError::not_found(
                    "El aviso que intentas editar ya no existe.",
                ));
            }
            raw.to_string()
        }
        _ => {
            let total: i64 =
                transaction.query_row("SELECT COUNT(*) FROM notification_rules", [], |row| {
                    row.get(0)
                })?;
            if total >= MAX_RULES {
                return Err(AppError::validation(format!(
                    "Ya hay {MAX_RULES} avisos programados. Elimina alguno antes de crear otro."
                )));
            }
            let id = Uuid::new_v4().to_string();
            transaction.execute(
                "INSERT INTO notification_rules(
                     id, app_id, kind, title, body, scheduled_for, repeat_rule,
                     lead_minutes, enabled
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                params![
                    id,
                    validated.app_id,
                    validated.kind.as_str(),
                    validated.title,
                    validated.body,
                    validated.scheduled_for,
                    validated.repeat.as_str(),
                    validated.lead_minutes,
                    validated.enabled,
                ],
            )?;
            id
        }
    };

    let rule = rule_by_id(&transaction, &id, now)?;
    transaction.commit()?;
    Ok(rule)
}

/// Elimina una regla. Los avisos que ya generó permanecen en la bandeja con
/// `rule_id = NULL` (`ON DELETE SET NULL` de la migración 023): borrar la regla
/// no reescribe la historia.
pub fn delete_rule(connection: &Connection, id: &str) -> AppResult<()> {
    validate_id(id, "El identificador del aviso no es válido.")?;
    let changed = connection.execute("DELETE FROM notification_rules WHERE id = ?1", [id])?;
    if changed == 0 {
        return Err(AppError::not_found(
            "El aviso que intentas eliminar ya no existe.",
        ));
    }
    Ok(())
}

/// Reglas que toca disparar en `now`.
///
/// Una regla vence cuando `scheduled_for - lead_minutes <= now`. No vuelve a
/// vencer mientras `last_fired_at` sea posterior o igual a ese mismo instante:
/// esa comparación —y no una contra «ahora»— es lo que impide el doble disparo.
pub fn due_rules(connection: &Connection, now: DateTime<Utc>) -> AppResult<Vec<DueRule>> {
    // SQLite descarta lo obvio; la aritmética de calendario la resuelve Rust,
    // porque el recorte de fin de mes desde el ancla no se puede expresar con
    // los modificadores de `datetime()`. El techo de MAX_RULES mantiene el
    // recorrido acotado.
    let mut statement = connection.prepare(&format!(
        "SELECT {RULE_COLUMNS}
           FROM notification_rules r
           LEFT JOIN games g ON g.app_id = r.app_id
          WHERE r.enabled = 1
            AND r.scheduled_for IS NOT NULL
            AND datetime(r.scheduled_for, '-' || r.lead_minutes || ' minutes')
                <= datetime(?1)
          ORDER BY datetime(r.scheduled_for) ASC, r.id ASC
          LIMIT ?2"
    ))?;
    let candidates = statement
        .query_map(params![iso(now), MAX_RULES], map_rule)?
        .collect::<Result<Vec<_>, _>>()?;

    let mut due = Vec::new();
    for mut rule in candidates {
        let anchor = parse_instant(
            rule.scheduled_for
                .as_deref()
                .ok_or_else(|| AppError::validation("El aviso programado perdió su fecha."))?,
        )?;
        let repeat = RepeatRule::parse(&rule.repeat_rule)?;
        let Some(occurrence) = current_occurrence(anchor, repeat, rule.lead_minutes, now)? else {
            continue;
        };
        let due_at = occurrence
            .checked_sub_signed(TimeDelta::minutes(i64::from(rule.lead_minutes)))
            .ok_or_else(calendar_overflow)?;
        // La comparación es contra el vencimiento de esta cita, no contra
        // «ahora»: eso es lo que impide que una regla se dispare dos veces.
        if let Some(last_fired_at) = rule.last_fired_at.as_deref()
            && parse_instant(last_fired_at)? >= due_at
        {
            continue;
        }
        enrich_rule(&mut rule, now)?;
        due.push(DueRule {
            rule,
            occurrence: iso(occurrence),
            due_at: iso(due_at),
        });
    }

    due.sort_by(|left, right| {
        left.due_at
            .cmp(&right.due_at)
            .then_with(|| left.rule.id.cmp(&right.rule.id))
    });
    due.truncate(DUE_RULES_LIMIT);
    Ok(due)
}

/// Registra que una regla acaba de dispararse.
///
/// `scheduled_for` **no se toca**: es el ancla y perder su día del mes haría
/// derivar la recurrencia. Lo que avanza es `last_fired_at`, y con él la cita
/// vigente: la regla devuelta ya trae `next_occurrence` recalculada desde el
/// ancla, con el recorte de fin de mes y el cambio de año aplicados.
pub fn mark_rule_fired(
    connection: &Connection,
    rule_id: &str,
    now: DateTime<Utc>,
) -> AppResult<NotificationRule> {
    validate_id(rule_id, "El identificador del aviso no es válido.")?;
    let changed = connection.execute(
        "UPDATE notification_rules
            SET last_fired_at = ?2,
                updated_at = ?2
          WHERE id = ?1",
        params![rule_id, iso(now)],
    )?;
    if changed == 0 {
        return Err(AppError::not_found("El aviso programado ya no existe."));
    }
    rule_by_id(connection, rule_id, now)
}

/// Dispara todas las reglas vencidas y crea un aviso por cada una, todo dentro
/// de una única transacción. Devuelve cuántos avisos se crearon.
pub fn run_due_rules(connection: &mut Connection, now: DateTime<Utc>) -> AppResult<u32> {
    let transaction = connection.transaction()?;
    let due = due_rules(&transaction, now)?;
    let mut created = 0u32;
    for entry in &due {
        let kind = NotificationKind::parse(&entry.rule.kind)?;
        let event = PendingEvent {
            rule_id: Some(entry.rule.id.clone()),
            app_id: entry.rule.app_id,
            kind: kind.as_str().to_string(),
            severity: kind.default_severity().to_string(),
            title: truncate_chars(&entry.rule.title, MAX_EVENT_TITLE_CHARS),
            body: truncate_chars(&entry.rule.body, MAX_EVENT_BODY_CHARS),
            occurred_at: entry.due_at.clone(),
            // La clave lleva la cita concreta, no el ancla: una regla semanal
            // genera un aviso por semana y ninguno repetido.
            dedupe_key: format!("rule:{}:{}", entry.rule.id, entry.occurrence),
        };
        if insert_event(&transaction, &event)? {
            created += 1;
        }
        mark_rule_fired(&transaction, &entry.rule.id, now)?;
    }
    transaction.commit()?;
    Ok(created)
}

// ---------------------------------------------------------------------------
// Derivación de eventos oficiales
// ---------------------------------------------------------------------------

/// Aviso a punto de insertarse. Solo existe dentro de una derivación.
struct PendingEvent {
    rule_id: Option<String>,
    app_id: Option<u32>,
    kind: String,
    severity: String,
    title: String,
    body: String,
    occurred_at: String,
    dedupe_key: String,
}

/// Convierte las señales oficiales ya persistidas en avisos de la bandeja.
///
/// **No inventa nada.** Cada rama lee una tabla que otra parte de Vindexa
/// rellenó a partir de una respuesta real de Steam o de una decisión explícita
/// de la persona usuaria. Si esas tablas están vacías, el informe sale a cero.
///
/// La deduplicación se apoya en el índice único parcial sobre `dedupe_key`: el
/// filtro `NOT EXISTS` evita releer lo ya derivado y el `INSERT OR IGNORE`
/// cierra la carrera si dos barridos coinciden.
pub fn derive_official_events(
    connection: &mut Connection,
    now: DateTime<Utc>,
) -> AppResult<DerivedEventsReport> {
    let transaction = connection.transaction()?;

    let early_access = derive_early_access_exits(&transaction)?;
    let release_dates = derive_release_date_changes(&transaction)?;
    let news = derive_official_news(&transaction)?;
    let reminders = derive_due_reminders(&transaction, now)?;
    let dlc = derive_new_dlc(&transaction)?;

    transaction.commit()?;

    let batches = [early_access, release_dates, news, reminders, dlc];
    Ok(DerivedEventsReport {
        early_access_exits: early_access.created,
        release_date_changes: release_dates.created,
        official_news: news.created,
        due_reminders: reminders.created,
        new_dlc: dlc.created,
        created: batches.iter().map(|batch| batch.created).sum(),
        skipped_duplicates: batches.iter().map(|batch| batch.skipped).sum(),
    })
}

/// Recuento de una sola fuente oficial dentro de una derivación.
#[derive(Debug, Clone, Copy, Default)]
struct DerivationBatch {
    created: u32,
    skipped: u32,
}

impl DerivationBatch {
    fn record(&mut self, inserted: bool) {
        if inserted {
            self.created += 1;
        } else {
            self.skipped += 1;
        }
    }
}

/// Salida de acceso anticipado.
///
/// `discovery_events` guarda el valor con la codificación que escribe
/// `discovery.rs` (`'released'` / `'early_access'`). Se aceptan además `'false'`
/// y `'0'` porque son codificaciones booleanas equivalentes de la misma señal;
/// ninguna otra cadena cuenta como salida de acceso anticipado.
fn derive_early_access_exits(connection: &Connection) -> AppResult<DerivationBatch> {
    let mut statement = connection.prepare(
        "SELECT d.app_id, g.title, d.observed_at
           FROM discovery_events d
           JOIN games g ON g.app_id = d.app_id
          WHERE d.kind = 'early_access_changed'
            AND lower(COALESCE(d.current_value, '')) IN ('released', 'false', '0')
            AND NOT EXISTS (
                SELECT 1 FROM notification_events e
                 WHERE e.dedupe_key =
                       'early_access_exit:' || d.app_id || ':' || d.observed_at
            )
          ORDER BY datetime(d.observed_at) DESC, d.id ASC
          LIMIT ?1",
    )?;
    let rows = statement
        .query_map([DERIVATION_BATCH as i64], |row| {
            Ok((
                row.get::<_, u32>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;

    let mut batch = DerivationBatch::default();
    for (app_id, title, observed_at) in rows {
        let event = PendingEvent {
            rule_id: None,
            app_id: Some(app_id),
            kind: "early_access_exit".to_string(),
            severity: "success".to_string(),
            title: truncate_chars(
                &format!("{title} ha salido de acceso anticipado"),
                MAX_EVENT_TITLE_CHARS,
            ),
            body: truncate_chars(
                &format!(
                    "Steam dejó de marcarlo como acceso anticipado. Señal observada el {}.",
                    human_date(&observed_at)
                ),
                MAX_EVENT_BODY_CHARS,
            ),
            occurred_at: observed_at.clone(),
            dedupe_key: format!("early_access_exit:{app_id}:{observed_at}"),
        };
        batch.record(insert_event(connection, &event)?);
    }
    Ok(batch)
}

/// Cambio de fecha de lanzamiento. El cuerpo cita la fecha anterior y la nueva
/// tal y como Steam las publicó; cuando falta alguna se dice, no se inventa.
fn derive_release_date_changes(connection: &Connection) -> AppResult<DerivationBatch> {
    let mut statement = connection.prepare(
        "SELECT d.app_id, g.title, d.observed_at, d.previous_value, d.current_value
           FROM discovery_events d
           JOIN games g ON g.app_id = d.app_id
          WHERE d.kind = 'release_date_changed'
            AND NOT EXISTS (
                SELECT 1 FROM notification_events e
                 WHERE e.dedupe_key =
                       'release_date_changed:' || d.app_id || ':' || d.observed_at
            )
          ORDER BY datetime(d.observed_at) DESC, d.id ASC
          LIMIT ?1",
    )?;
    let rows = statement
        .query_map([DERIVATION_BATCH as i64], |row| {
            Ok((
                row.get::<_, u32>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, Option<String>>(3)?,
                row.get::<_, Option<String>>(4)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;

    let mut batch = DerivationBatch::default();
    for (app_id, title, observed_at, previous, current) in rows {
        let previous = describe_release_date(previous.as_deref());
        let current = describe_release_date(current.as_deref());
        let event = PendingEvent {
            rule_id: None,
            app_id: Some(app_id),
            kind: "release_date_changed".to_string(),
            severity: "info".to_string(),
            title: truncate_chars(
                &format!("{title} cambió su fecha de lanzamiento"),
                MAX_EVENT_TITLE_CHARS,
            ),
            body: truncate_chars(
                &format!("Steam pasó de {previous} a {current}."),
                MAX_EVENT_BODY_CHARS,
            ),
            occurred_at: observed_at.clone(),
            dedupe_key: format!("release_date_changed:{app_id}:{observed_at}"),
        };
        batch.record(insert_event(connection, &event)?);
    }
    Ok(batch)
}

/// Publicación oficial nueva de un juego en seguimiento. La caché de
/// `steam_news_items` la mantiene `discovery.rs`; aquí solo se lee.
fn derive_official_news(connection: &Connection) -> AppResult<DerivationBatch> {
    let mut statement = connection.prepare(
        "SELECT n.app_id, g.title, n.gid, n.title, n.content_preview, n.published_at
           FROM steam_news_items n
           JOIN games g ON g.app_id = n.app_id
           JOIN game_personal p ON p.app_id = n.app_id
          WHERE p.tracking = 1
            AND NOT EXISTS (
                SELECT 1 FROM notification_events e
                 WHERE e.dedupe_key = 'official_news:' || n.app_id || ':' || n.gid
            )
          ORDER BY datetime(n.published_at) DESC, n.gid ASC
          LIMIT ?1",
    )?;
    let rows = statement
        .query_map([DERIVATION_BATCH as i64], |row| {
            Ok((
                row.get::<_, u32>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, String>(5)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;

    let mut batch = DerivationBatch::default();
    for (app_id, game_title, gid, news_title, preview, published_at) in rows {
        let event = PendingEvent {
            rule_id: None,
            app_id: Some(app_id),
            kind: "official_news".to_string(),
            severity: "info".to_string(),
            title: truncate_chars(
                &format!("{game_title}: {news_title}"),
                MAX_EVENT_TITLE_CHARS,
            ),
            body: truncate_chars(&preview, MAX_EVENT_BODY_CHARS),
            occurred_at: published_at,
            dedupe_key: format!("official_news:{app_id}:{gid}"),
        };
        batch.record(insert_event(connection, &event)?);
    }
    Ok(batch)
}

/// Recordatorio vencido. Aplazar un recordatorio cambia su `due_at` y por tanto
/// su `dedupe_key`: el aviso vuelve a aparecer cuando vuelva a vencer, sin
/// duplicar el anterior.
fn derive_due_reminders(connection: &Connection, now: DateTime<Utc>) -> AppResult<DerivationBatch> {
    let mut statement = connection.prepare(
        "SELECT r.id, r.app_id, g.title, r.due_at, r.note
           FROM game_reminders r
           JOIN games g ON g.app_id = r.app_id
          WHERE r.completed_at IS NULL
            AND datetime(r.due_at) <= datetime(?1)
            AND NOT EXISTS (
                SELECT 1 FROM notification_events e
                 WHERE e.dedupe_key = 'reminder:' || r.id || ':' || r.due_at
            )
          ORDER BY datetime(r.due_at) ASC, r.id ASC
          LIMIT ?2",
    )?;
    let rows = statement
        .query_map(params![iso(now), DERIVATION_BATCH as i64], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, u32>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;

    let mut batch = DerivationBatch::default();
    for (reminder_id, app_id, title, due_at, note) in rows {
        let note = note.trim();
        let body = if note.is_empty() {
            format!("El recordatorio vencía el {}.", human_date(&due_at))
        } else {
            format!("Vencía el {}. {note}", human_date(&due_at))
        };
        let event = PendingEvent {
            rule_id: None,
            app_id: Some(app_id),
            kind: "reminder_due".to_string(),
            severity: "warning".to_string(),
            title: truncate_chars(
                &format!("Recordatorio vencido: {title}"),
                MAX_EVENT_TITLE_CHARS,
            ),
            body: truncate_chars(&body, MAX_EVENT_BODY_CHARS),
            occurred_at: due_at.clone(),
            dedupe_key: format!("reminder:{reminder_id}:{due_at}"),
        };
        batch.record(insert_event(connection, &event)?);
    }
    Ok(batch)
}

/// DLC nuevo detectado en `game_dlc` (migración 020). Solo se avisa de un DLC
/// cuya ficha Steam sí publicó (`metadata_status = 'success'` y con título): un
/// AppID suelto sin nombre no es una noticia. La `dedupe_key` no lleva fecha, de
/// modo que cada DLC genera como mucho un aviso en toda la vida de la base.
///
/// La tabla puede estar vacía —la puebla otro módulo— y ese caso devuelve cero
/// sin error.
fn derive_new_dlc(connection: &Connection) -> AppResult<DerivationBatch> {
    let mut statement = connection.prepare(
        "SELECT d.app_id, g.title, d.dlc_app_id, d.title, d.release_date, d.updated_at
           FROM game_dlc d
           JOIN games g ON g.app_id = d.app_id
          WHERE d.hidden = 0
            AND d.metadata_status = 'success'
            AND trim(d.title) <> ''
            AND NOT EXISTS (
                SELECT 1 FROM notification_events e
                 WHERE e.dedupe_key = 'dlc_release:' || d.app_id || ':' || d.dlc_app_id
            )
          ORDER BY datetime(d.updated_at) DESC, d.dlc_app_id ASC
          LIMIT ?1",
    )?;
    let rows = statement
        .query_map([DERIVATION_BATCH as i64], |row| {
            Ok((
                row.get::<_, u32>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, u32>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, Option<String>>(4)?,
                row.get::<_, String>(5)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;

    let mut batch = DerivationBatch::default();
    for (app_id, game_title, dlc_app_id, dlc_title, release_date, updated_at) in rows {
        let body = match release_date
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            Some(date) => format!("Fecha de lanzamiento publicada por Steam: {date}."),
            None => "Steam todavía no publica su fecha de lanzamiento.".to_string(),
        };
        let event = PendingEvent {
            rule_id: None,
            app_id: Some(app_id),
            kind: "dlc_release".to_string(),
            severity: "info".to_string(),
            title: truncate_chars(
                &format!("Nuevo DLC de {game_title}: {dlc_title}"),
                MAX_EVENT_TITLE_CHARS,
            ),
            body: truncate_chars(&body, MAX_EVENT_BODY_CHARS),
            occurred_at: updated_at,
            dedupe_key: format!("dlc_release:{app_id}:{dlc_app_id}"),
        };
        batch.record(insert_event(connection, &event)?);
    }
    Ok(batch)
}

/// Barrido completo: dispara las reglas vencidas y deriva las señales oficiales.
pub fn refresh(
    connection: &mut Connection,
    now: DateTime<Utc>,
) -> AppResult<NotificationRefreshReport> {
    let scheduled_events = run_due_rules(connection, now)?;
    let derived = derive_official_events(connection, now)?;
    let unread = unread_counters(connection)?;
    Ok(NotificationRefreshReport {
        scheduled_events,
        derived,
        unread,
    })
}

// ---------------------------------------------------------------------------
// Bandeja
// ---------------------------------------------------------------------------

const EVENT_COLUMNS: &str = "e.id, e.rule_id, e.app_id, g.title, e.kind, e.severity, e.title,
     e.body, e.occurred_at, e.read_at, e.dismissed_at, e.dedupe_key";

/// Página de la bandeja, ordenada por `occurred_at` descendente.
pub fn inbox(
    connection: &Connection,
    filter: &NotificationInboxFilter,
    limit: u32,
    offset: u32,
) -> AppResult<NotificationInbox> {
    let scope = InboxScope::parse(filter.scope.as_deref())?;
    let severity = normalized_severity(filter.severity.as_deref())?.unwrap_or("");
    let app_id = match filter.app_id {
        Some(0) => return Err(AppError::validation("El juego del filtro no es válido.")),
        other => other.unwrap_or(0),
    };
    // `0` significa «usa la página estándar»; el resto se acota al techo.
    let limit = if limit == 0 {
        DEFAULT_INBOX_LIMIT
    } else {
        limit.min(MAX_INBOX_LIMIT)
    };
    let clause = scope.clause();

    let total: i64 = connection.query_row(
        &format!(
            "SELECT COUNT(*)
               FROM notification_events e
              WHERE {clause}
                AND (?1 = '' OR e.severity = ?1)
                AND (?2 = 0 OR e.app_id = ?2)"
        ),
        params![severity, app_id],
        |row| row.get(0),
    )?;

    let mut statement = connection.prepare(&format!(
        "SELECT {EVENT_COLUMNS}
           FROM notification_events e
           LEFT JOIN games g ON g.app_id = e.app_id
          WHERE {clause}
            AND (?1 = '' OR e.severity = ?1)
            AND (?2 = 0 OR e.app_id = ?2)
          ORDER BY datetime(e.occurred_at) DESC, e.occurred_at DESC, e.id ASC
          LIMIT ?3 OFFSET ?4"
    ))?;
    let items = statement
        .query_map(params![severity, app_id, limit, offset], map_event)?
        .collect::<Result<Vec<_>, _>>()?;

    Ok(NotificationInbox {
        items,
        total,
        limit,
        offset,
        unread: unread_counters(connection)?,
    })
}

/// Contadores de avisos sin leer y sin descartar, desglosados por severidad.
pub fn unread_counters(connection: &Connection) -> AppResult<NotificationCounters> {
    connection
        .query_row(
            "SELECT COUNT(*),
                    COALESCE(SUM(CASE WHEN severity = 'info' THEN 1 ELSE 0 END), 0),
                    COALESCE(SUM(CASE WHEN severity = 'success' THEN 1 ELSE 0 END), 0),
                    COALESCE(SUM(CASE WHEN severity = 'warning' THEN 1 ELSE 0 END), 0),
                    COALESCE(SUM(CASE WHEN severity = 'critical' THEN 1 ELSE 0 END), 0)
               FROM notification_events
              WHERE dismissed_at IS NULL AND read_at IS NULL",
            [],
            |row| {
                Ok(NotificationCounters {
                    total: row.get(0)?,
                    info: row.get(1)?,
                    success: row.get(2)?,
                    warning: row.get(3)?,
                    critical: row.get(4)?,
                })
            },
        )
        .map_err(Into::into)
}

/// Marca un aviso como leído. Idempotente: `COALESCE` conserva la marca
/// original, de modo que repetir la llamada no reescribe el instante.
pub fn mark_read(connection: &Connection, event_id: &str, now: DateTime<Utc>) -> AppResult<()> {
    validate_id(event_id, "El identificador del aviso no es válido.")?;
    let changed = connection.execute(
        "UPDATE notification_events
            SET read_at = COALESCE(read_at, ?2)
          WHERE id = ?1",
        params![event_id, iso(now)],
    )?;
    if changed == 0 {
        return Err(AppError::not_found("El aviso ya no está en la bandeja."));
    }
    Ok(())
}

/// Marca como leídos todos los avisos pendientes. Devuelve cuántos cambiaron;
/// repetir la llamada devuelve `0` sin tocar nada.
pub fn mark_all_read(connection: &mut Connection, now: DateTime<Utc>) -> AppResult<u32> {
    let transaction = connection.transaction()?;
    let changed = transaction.execute(
        "UPDATE notification_events
            SET read_at = ?1
          WHERE read_at IS NULL AND dismissed_at IS NULL",
        [iso(now)],
    )?;
    transaction.commit()?;
    Ok(changed as u32)
}

/// Descarta un aviso. Descartar implica haberlo visto, así que también fija
/// `read_at` si aún estaba sin leer. Idempotente por la misma razón que
/// [`mark_read`].
pub fn dismiss_event(connection: &Connection, event_id: &str, now: DateTime<Utc>) -> AppResult<()> {
    validate_id(event_id, "El identificador del aviso no es válido.")?;
    let moment = iso(now);
    let changed = connection.execute(
        "UPDATE notification_events
            SET dismissed_at = COALESCE(dismissed_at, ?2),
                read_at = COALESCE(read_at, ?2)
          WHERE id = ?1",
        params![event_id, moment],
    )?;
    if changed == 0 {
        return Err(AppError::not_found("El aviso ya no está en la bandeja."));
    }
    Ok(())
}

/// Descarta todos los avisos pendientes. Devuelve cuántos cambiaron.
// La bandeja no ofrece todavía «descartar todos», así que nada en producción
// llama aquí. Está probada y lista para cuando ese botón exista.
#[allow(dead_code, reason = "falta la acción «descartar todos» en la bandeja")]
pub fn dismiss_all(connection: &mut Connection, now: DateTime<Utc>) -> AppResult<u32> {
    let moment = iso(now);
    let transaction = connection.transaction()?;
    let changed = transaction.execute(
        "UPDATE notification_events
            SET dismissed_at = ?1,
                read_at = COALESCE(read_at, ?1)
          WHERE dismissed_at IS NULL",
        [moment],
    )?;
    transaction.commit()?;
    Ok(changed as u32)
}

/// Borra los avisos descartados hace más de `retention_days` días.
///
/// Nunca toca un aviso pendiente: la condición exige `dismissed_at IS NOT NULL`.
/// Devuelve cuántas filas se eliminaron.
// Ninguna tarea de mantenimiento la invoca aún, así que `notification_events`
// crece sin tope. Es una deuda conocida, no una función abandonada.
#[allow(dead_code, reason = "falta engancharla al mantenimiento periódico")]
pub fn prune_events(
    connection: &Connection,
    now: DateTime<Utc>,
    retention_days: u32,
) -> AppResult<u32> {
    if retention_days == 0 || retention_days > MAX_RETENTION_DAYS {
        return Err(AppError::validation(format!(
            "La retención debe estar entre 1 y {MAX_RETENTION_DAYS} días."
        )));
    }
    let deleted = connection.execute(
        "DELETE FROM notification_events
          WHERE dismissed_at IS NOT NULL
            AND datetime(dismissed_at) < datetime(?1, '-' || ?2 || ' days')",
        params![iso(now), retention_days],
    )?;
    Ok(deleted as u32)
}

// ---------------------------------------------------------------------------
// Validación y utilidades
// ---------------------------------------------------------------------------

struct ValidatedRule {
    app_id: Option<u32>,
    kind: NotificationKind,
    title: String,
    body: String,
    scheduled_for: Option<String>,
    repeat: RepeatRule,
    lead_minutes: u32,
    enabled: bool,
}

fn validate_rule_input(input: &SaveNotificationRuleInput) -> AppResult<ValidatedRule> {
    let kind = NotificationKind::parse(input.kind.trim())?;
    let repeat = RepeatRule::parse(input.repeat_rule.trim())?;

    let title = input.title.trim();
    if title.is_empty() {
        return Err(AppError::validation(
            "El aviso necesita un título: escribe qué quieres recordar.",
        ));
    }
    if title.chars().count() > MAX_TITLE_CHARS {
        return Err(AppError::validation(format!(
            "El título del aviso no puede superar {MAX_TITLE_CHARS} caracteres."
        )));
    }
    let body = input.body.trim();
    if body.chars().count() > MAX_BODY_CHARS {
        return Err(AppError::validation(format!(
            "La descripción del aviso no puede superar {MAX_BODY_CHARS} caracteres."
        )));
    }

    if input.lead_minutes > MAX_LEAD_MINUTES {
        return Err(AppError::validation(format!(
            "El margen de aviso no puede superar {MAX_LEAD_MINUTES} minutos (30 días)."
        )));
    }

    let app_id = match input.app_id {
        Some(0) => {
            return Err(AppError::validation(
                "El juego del aviso no es válido: usa un AppID mayor que cero o deja el aviso sin juego.",
            ));
        }
        other => other,
    };
    if kind.requires_game() && app_id.is_none() {
        return Err(AppError::validation(
            "Este tipo de aviso habla de un juego concreto: elige el juego antes de guardarlo.",
        ));
    }

    let scheduled_for = match input
        .scheduled_for
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        Some(raw) => Some(iso(parse_instant(raw)?)),
        None => None,
    };

    if scheduled_for.is_none() {
        if kind == NotificationKind::Manual {
            return Err(AppError::validation(
                "Un aviso manual sin fecha no puede dispararse: indica cuándo quieres recibirlo.",
            ));
        }
        if repeat != RepeatRule::None {
            return Err(AppError::validation(
                "Un aviso que se repite necesita una primera fecha: indícala o cambia la repetición a «none».",
            ));
        }
    }

    Ok(ValidatedRule {
        app_id,
        kind,
        title: title.to_string(),
        body: body.to_string(),
        scheduled_for,
        repeat,
        lead_minutes: input.lead_minutes,
        enabled: input.enabled,
    })
}

/// Horizonte de vencimiento: una cita está vencida cuando
/// `cita - margen <= ahora`, es decir cuando `cita <= ahora + margen`.
fn due_horizon(now: DateTime<Utc>, lead_minutes: u32) -> AppResult<DateTime<Utc>> {
    now.checked_add_signed(TimeDelta::minutes(i64::from(lead_minutes)))
        .ok_or_else(calendar_overflow)
}

/// Índice de la última cita vencida en `horizon`, contando el ancla como cita
/// número cero. `None` cuando ni siquiera la primera cita ha vencido.
///
/// El índice se estima de un salto y se corrige como mucho un periodo, así que
/// una regla diaria olvidada durante años se resuelve sin recorrer el calendario.
fn elapsed_periods(
    anchor: DateTime<Utc>,
    repeat: RepeatRule,
    horizon: DateTime<Utc>,
) -> AppResult<Option<u32>> {
    if anchor > horizon {
        return Ok(None);
    }
    if repeat == RepeatRule::None {
        return Ok(Some(0));
    }

    let mut periods: u32 = match repeat {
        RepeatRule::Daily | RepeatRule::Weekly => {
            let period_days = if repeat == RepeatRule::Daily { 1 } else { 7 };
            u32::try_from((horizon - anchor).num_days() / period_days)
                .map_err(|_| calendar_overflow())?
        }
        // El hueco en meses naturales es exacto salvo por el día del mes, que
        // corrige el bucle de abajo.
        RepeatRule::Monthly => {
            let gap = (i64::from(horizon.year()) - i64::from(anchor.year())) * 12
                + i64::from(horizon.month())
                - i64::from(anchor.month());
            u32::try_from(gap.max(0)).map_err(|_| calendar_overflow())?
        }
        RepeatRule::None => unreachable!("la recurrencia nula ya se resolvió arriba"),
    };

    let mut corrections = 0u32;
    // La estimación puede pasarse: retrocede hasta la última cita ya vencida.
    while periods > 0 && occurrence(anchor, repeat, periods)? > horizon {
        periods -= 1;
        corrections += 1;
        if corrections > MAX_RECURRENCE_STEPS {
            return Err(stalled_recurrence_error());
        }
    }
    // O quedarse corta: avanza mientras la siguiente cita también esté vencida.
    while occurrence(anchor, repeat, periods + 1)? <= horizon {
        periods += 1;
        corrections += 1;
        if corrections > MAX_RECURRENCE_STEPS {
            return Err(stalled_recurrence_error());
        }
    }
    Ok(Some(periods))
}

/// Última cita cuyo aviso ya venció en `now`, o `None` si aún no llegó ninguna.
fn current_occurrence(
    anchor: DateTime<Utc>,
    repeat: RepeatRule,
    lead_minutes: u32,
    now: DateTime<Utc>,
) -> AppResult<Option<DateTime<Utc>>> {
    let horizon = due_horizon(now, lead_minutes)?;
    match elapsed_periods(anchor, repeat, horizon)? {
        Some(periods) => Ok(Some(occurrence(anchor, repeat, periods)?)),
        None => Ok(None),
    }
}

/// Primera cita cuyo aviso todavía no ha vencido en `now`. `None` cuando una
/// regla puntual ya quedó consumida.
fn next_occurrence(
    anchor: DateTime<Utc>,
    repeat: RepeatRule,
    lead_minutes: u32,
    now: DateTime<Utc>,
) -> AppResult<Option<DateTime<Utc>>> {
    let horizon = due_horizon(now, lead_minutes)?;
    match elapsed_periods(anchor, repeat, horizon)? {
        // Ni la primera cita ha vencido: la próxima es el propio ancla.
        None => Ok(Some(anchor)),
        Some(_) if repeat == RepeatRule::None => Ok(None),
        Some(periods) => Ok(Some(occurrence(anchor, repeat, periods + 1)?)),
    }
}

fn stalled_recurrence_error() -> AppError {
    AppError::validation(
        "El aviso periódico lleva demasiado tiempo sin dispararse: edita su fecha para reactivarlo.",
    )
}

/// La cita número `periods` contada desde `anchor`, que es siempre la cita
/// cero. El salto mensual delega en `chrono::Months`, que recorta al último día
/// válido del mes destino en lugar de desbordar: el 31 de enero cae en el 28 (o
/// 29) de febrero, y como el ancla nunca se reescribe, el mes siguiente vuelve
/// al 31.
fn occurrence(anchor: DateTime<Utc>, repeat: RepeatRule, periods: u32) -> AppResult<DateTime<Utc>> {
    let advanced = match repeat {
        RepeatRule::None if periods == 0 => Some(anchor),
        RepeatRule::None => {
            return Err(AppError::validation(
                "Un aviso sin repetición solo tiene una cita.",
            ));
        }
        RepeatRule::Daily => anchor.checked_add_days(Days::new(u64::from(periods))),
        RepeatRule::Weekly => anchor.checked_add_days(Days::new(u64::from(periods) * 7)),
        RepeatRule::Monthly => anchor.checked_add_months(Months::new(periods)),
    };
    advanced.ok_or_else(calendar_overflow)
}

fn calendar_overflow() -> AppError {
    AppError::validation("La fecha del aviso se sale del calendario admitido.")
}

fn insert_event(connection: &Connection, event: &PendingEvent) -> AppResult<bool> {
    let inserted = connection.execute(
        "INSERT OR IGNORE INTO notification_events(
             id, rule_id, app_id, kind, severity, title, body, occurred_at, dedupe_key
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        params![
            Uuid::new_v4().to_string(),
            event.rule_id,
            event.app_id,
            event.kind,
            event.severity,
            event.title,
            event.body,
            event.occurred_at,
            event.dedupe_key,
        ],
    )?;
    Ok(inserted == 1)
}

fn rule_by_id(
    connection: &Connection,
    id: &str,
    now: DateTime<Utc>,
) -> AppResult<NotificationRule> {
    let mut rule = connection
        .query_row(
            &format!(
                "SELECT {RULE_COLUMNS}
                   FROM notification_rules r
                   LEFT JOIN games g ON g.app_id = r.app_id
                  WHERE r.id = ?1"
            ),
            [id],
            map_rule,
        )
        .optional()?
        .ok_or_else(|| AppError::not_found("El aviso programado ya no existe."))?;
    enrich_rule(&mut rule, now)?;
    Ok(rule)
}

/// Calcula la cita vigente y la siguiente a partir del ancla persistida. Son
/// campos derivados: no existen como columnas y por eso dependen del instante
/// que se consulte.
fn enrich_rule(rule: &mut NotificationRule, now: DateTime<Utc>) -> AppResult<()> {
    let Some(raw) = rule.scheduled_for.as_deref() else {
        rule.current_occurrence = None;
        rule.next_occurrence = None;
        return Ok(());
    };
    let anchor = parse_instant(raw)?;
    let repeat = RepeatRule::parse(&rule.repeat_rule)?;
    rule.current_occurrence = current_occurrence(anchor, repeat, rule.lead_minutes, now)?.map(iso);
    rule.next_occurrence = next_occurrence(anchor, repeat, rule.lead_minutes, now)?.map(iso);
    Ok(())
}

fn map_rule(row: &rusqlite::Row<'_>) -> rusqlite::Result<NotificationRule> {
    Ok(NotificationRule {
        id: row.get(0)?,
        app_id: row.get(1)?,
        game_title: row.get(2)?,
        kind: row.get(3)?,
        title: row.get(4)?,
        body: row.get(5)?,
        scheduled_for: row.get(6)?,
        repeat_rule: row.get(7)?,
        lead_minutes: row.get(8)?,
        enabled: row.get::<_, i64>(9)? != 0,
        last_fired_at: row.get(10)?,
        // Campos derivados: los rellena `enrich_rule` con el instante que pida
        // quien consulta, porque no existen como columnas.
        current_occurrence: None,
        next_occurrence: None,
        created_at: row.get(11)?,
        updated_at: row.get(12)?,
    })
}

fn map_event(row: &rusqlite::Row<'_>) -> rusqlite::Result<NotificationEvent> {
    Ok(NotificationEvent {
        id: row.get(0)?,
        rule_id: row.get(1)?,
        app_id: row.get(2)?,
        game_title: row.get(3)?,
        kind: row.get(4)?,
        severity: row.get(5)?,
        title: row.get(6)?,
        body: row.get(7)?,
        occurred_at: row.get(8)?,
        read_at: row.get(9)?,
        dismissed_at: row.get(10)?,
        dedupe_key: row.get(11)?,
    })
}

fn normalized_severity(value: Option<&str>) -> AppResult<Option<&str>> {
    let Some(value) = value.map(str::trim).filter(|value| !value.is_empty()) else {
        return Ok(None);
    };
    if NOTIFICATION_SEVERITIES.contains(&value) {
        Ok(Some(value))
    } else {
        Err(AppError::validation(format!(
            "La severidad del filtro no es válida. Usa una de: {}.",
            NOTIFICATION_SEVERITIES.join(", ")
        )))
    }
}

fn validate_id(value: &str, message: &str) -> AppResult<()> {
    Uuid::parse_str(value)
        .map(|_| ())
        .map_err(|_| AppError::validation(message))
}

/// Instante ISO-8601 en UTC con milisegundos, idéntico al que produce
/// `strftime('%Y-%m-%dT%H:%M:%fZ', 'now')` en el resto del esquema.
fn iso(moment: DateTime<Utc>) -> String {
    moment.to_rfc3339_opts(SecondsFormat::Millis, true)
}

fn parse_instant(value: &str) -> AppResult<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(value.trim())
        .map(|moment| moment.with_timezone(&Utc))
        .map_err(|_| {
            AppError::validation(
                "La fecha del aviso debe ser una marca ISO-8601 con zona horaria, por ejemplo 2026-09-01T18:30:00Z.",
            )
        })
}

/// Fecha legible `dd/mm/aaaa`. Si la marca no es analizable se devuelve tal cual:
/// preferimos mostrar el dato original antes que fabricar uno.
fn human_date(value: &str) -> String {
    match DateTime::parse_from_rfc3339(value.trim()) {
        Ok(moment) => moment.with_timezone(&Utc).format("%d/%m/%Y").to_string(),
        Err(_) => value.trim().to_string(),
    }
}

fn describe_release_date(value: Option<&str>) -> String {
    match value.map(str::trim).filter(|value| !value.is_empty()) {
        Some(date) => format!("«{date}»"),
        None => "«sin fecha publicada»".to_string(),
    }
}

fn truncate_chars(value: &str, max: usize) -> String {
    let trimmed = value.trim();
    if trimmed.chars().count() <= max {
        return trimmed.to_string();
    }
    let mut result: String = trimmed.chars().take(max.saturating_sub(1)).collect();
    result.push('…');
    result
}

// ---------------------------------------------------------------------------
// Pruebas
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::migrations;

    fn instant(value: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(value)
            .expect("marca de prueba válida")
            .with_timezone(&Utc)
    }

    /// Instante de referencia de las pruebas que no dependen del reloj.
    fn ahora_base() -> DateTime<Utc> {
        instant("2026-01-01T00:00:00Z")
    }

    /// `save_rule` con el instante de referencia: las pruebas de validación no
    /// dependen de cuándo se ejecutan.
    fn guardar_regla(
        connection: &mut Connection,
        input: &SaveNotificationRuleInput,
    ) -> AppResult<NotificationRule> {
        save_rule(connection, input, ahora_base())
    }

    fn database() -> Connection {
        let mut connection = Connection::open_in_memory().expect("abrir SQLite");
        migrations::migrate(&mut connection).expect("aplicar migraciones");
        connection
            .execute_batch(
                "INSERT INTO statuses(id, name, color, position, built_in)
                 VALUES ('unclassified', 'Sin clasificar', '#71838E', 0, 1);
                 INSERT INTO games(app_id, title) VALUES
                   (10, 'Viajero'),
                   (20, 'Estratega');
                 INSERT INTO game_personal(app_id, status_id, tracking) VALUES
                   (10, 'unclassified', 1),
                   (20, 'unclassified', 0);",
            )
            .expect("sembrar biblioteca de prueba");
        connection
    }

    fn manual_rule(title: &str, scheduled_for: &str, repeat: &str) -> SaveNotificationRuleInput {
        SaveNotificationRuleInput {
            id: None,
            app_id: None,
            kind: "manual".to_string(),
            title: title.to_string(),
            body: "Cuerpo del aviso".to_string(),
            scheduled_for: Some(scheduled_for.to_string()),
            repeat_rule: repeat.to_string(),
            lead_minutes: 0,
            enabled: true,
        }
    }

    // ---------------------------------------------------------------- validación

    #[test]
    fn rechaza_una_regla_manual_sin_fecha_con_un_mensaje_accionable() {
        let mut connection = database();
        let input = SaveNotificationRuleInput {
            scheduled_for: None,
            ..manual_rule("Revisar la cola", "2026-09-01T10:00:00Z", "none")
        };

        let error = guardar_regla(&mut connection, &input).expect_err("rechazar aviso sin fecha");

        assert_eq!(error.code, "validation");
        assert!(
            error.message.contains("indica cuándo quieres recibirlo"),
            "el mensaje debe decir qué corregir: {}",
            error.message
        );
    }

    #[test]
    fn rechaza_titulos_vacios_desmesurados_y_valores_fuera_de_allowlist() {
        let mut connection = database();

        let vacio = guardar_regla(
            &mut connection,
            &manual_rule("   ", "2026-09-01T10:00:00Z", "none"),
        )
        .expect_err("rechazar título vacío");
        assert_eq!(vacio.code, "validation");
        assert!(vacio.message.contains("título"));

        let largo = guardar_regla(
            &mut connection,
            &manual_rule(&"á".repeat(121), "2026-09-01T10:00:00Z", "none"),
        )
        .expect_err("rechazar título largo");
        assert!(largo.message.contains("120 caracteres"));

        let tipo = guardar_regla(
            &mut connection,
            &SaveNotificationRuleInput {
                kind: "inventado".to_string(),
                ..manual_rule("Aviso", "2026-09-01T10:00:00Z", "none")
            },
        )
        .expect_err("rechazar tipo desconocido");
        assert!(tipo.message.contains("El tipo de aviso no es válido"));

        let repeticion = guardar_regla(
            &mut connection,
            &manual_rule("Aviso", "2026-09-01T10:00:00Z", "quincenal"),
        )
        .expect_err("rechazar repetición desconocida");
        assert!(
            repeticion
                .message
                .contains("La repetición del aviso no es válida")
        );

        let margen = guardar_regla(
            &mut connection,
            &SaveNotificationRuleInput {
                lead_minutes: MAX_LEAD_MINUTES + 1,
                ..manual_rule("Aviso", "2026-09-01T10:00:00Z", "none")
            },
        )
        .expect_err("rechazar margen excesivo");
        assert!(margen.message.contains("margen de aviso"));

        let fecha = guardar_regla(
            &mut connection,
            &manual_rule("Aviso", "1 de septiembre", "none"),
        )
        .expect_err("rechazar fecha no ISO");
        assert!(fecha.message.contains("ISO-8601"));
    }

    #[test]
    fn rechaza_una_repeticion_sin_primera_fecha_y_un_tipo_de_juego_sin_juego() {
        let mut connection = database();

        let sin_fecha = guardar_regla(
            &mut connection,
            &SaveNotificationRuleInput {
                kind: "reminder_digest".to_string(),
                scheduled_for: None,
                repeat_rule: "weekly".to_string(),
                ..manual_rule("Resumen", "2026-09-01T10:00:00Z", "weekly")
            },
        )
        .expect_err("rechazar repetición sin fecha");
        assert!(sin_fecha.message.contains("primera fecha"));

        let sin_juego = guardar_regla(
            &mut connection,
            &SaveNotificationRuleInput {
                kind: "early_access_exit".to_string(),
                app_id: None,
                ..manual_rule("Salida de EA", "2026-09-01T10:00:00Z", "none")
            },
        )
        .expect_err("rechazar tipo de juego sin juego");
        assert!(sin_juego.message.contains("elige el juego"));

        let juego_ausente = guardar_regla(
            &mut connection,
            &SaveNotificationRuleInput {
                kind: "release_date".to_string(),
                app_id: Some(999),
                ..manual_rule("Lanzamiento", "2026-09-01T10:00:00Z", "none")
            },
        )
        .expect_err("rechazar juego inexistente");
        assert_eq!(juego_ausente.code, "not_found");
    }

    #[test]
    fn crea_edita_lista_y_elimina_una_regla() {
        let mut connection = database();
        let creada = guardar_regla(
            &mut connection,
            &SaveNotificationRuleInput {
                app_id: Some(10),
                ..manual_rule("Terminar el capítulo 3", "2026-09-01T18:30:00Z", "none")
            },
        )
        .expect("crear aviso");
        assert_eq!(
            creada.scheduled_for.as_deref(),
            Some("2026-09-01T18:30:00.000Z")
        );
        assert_eq!(creada.game_title.as_deref(), Some("Viajero"));
        assert!(creada.enabled);

        let editada = guardar_regla(
            &mut connection,
            &SaveNotificationRuleInput {
                id: Some(creada.id.clone()),
                app_id: Some(10),
                enabled: false,
                ..manual_rule("Terminar el capítulo 4", "2026-09-02T18:30:00Z", "weekly")
            },
        )
        .expect("editar aviso");
        assert_eq!(editada.id, creada.id);
        assert_eq!(editada.title, "Terminar el capítulo 4");
        assert_eq!(editada.repeat_rule, "weekly");
        assert!(!editada.enabled);

        assert_eq!(
            list_rules(&connection, Some(10), ahora_base())
                .expect("listar")
                .len(),
            1
        );
        assert_eq!(
            list_rules(&connection, Some(20), ahora_base())
                .expect("listar")
                .len(),
            0
        );
        assert_eq!(
            list_rules(&connection, None, ahora_base())
                .expect("listar")
                .len(),
            1
        );

        delete_rule(&connection, &creada.id).expect("eliminar aviso");
        assert!(
            list_rules(&connection, None, ahora_base())
                .expect("listar")
                .is_empty()
        );
        assert_eq!(
            delete_rule(&connection, &creada.id)
                .expect_err("segundo borrado")
                .code,
            "not_found"
        );
    }

    // ------------------------------------------------------------ disparo único

    #[test]
    fn una_regla_puntual_no_se_dispara_dos_veces() {
        let mut connection = database();
        guardar_regla(
            &mut connection,
            &manual_rule("Aviso puntual", "2026-09-01T10:00:00Z", "none"),
        )
        .expect("crear aviso");

        let ahora = instant("2026-09-01T10:05:00Z");
        assert_eq!(due_rules(&connection, ahora).expect("vencidas").len(), 1);
        assert_eq!(run_due_rules(&mut connection, ahora).expect("disparar"), 1);

        assert!(
            due_rules(&connection, instant("2026-09-02T10:00:00Z"))
                .expect("vencidas tras disparar")
                .is_empty()
        );
        assert_eq!(
            run_due_rules(&mut connection, instant("2026-09-02T10:00:00Z")).expect("repetir"),
            0
        );

        let bandeja = inbox(
            &connection,
            &NotificationInboxFilter::default(),
            DEFAULT_INBOX_LIMIT,
            0,
        )
        .expect("bandeja");
        assert_eq!(bandeja.total, 1);
        assert_eq!(bandeja.items[0].title, "Aviso puntual");
        assert_eq!(bandeja.items[0].severity, "info");
    }

    #[test]
    fn el_margen_de_aviso_adelanta_el_vencimiento_sin_duplicarlo() {
        let mut connection = database();
        guardar_regla(
            &mut connection,
            &SaveNotificationRuleInput {
                lead_minutes: 60,
                ..manual_rule("Con margen", "2026-09-01T12:00:00Z", "none")
            },
        )
        .expect("crear aviso con margen");

        assert!(
            due_rules(&connection, instant("2026-09-01T10:59:00Z"))
                .expect("antes del margen")
                .is_empty()
        );
        let vencidas =
            due_rules(&connection, instant("2026-09-01T11:00:00Z")).expect("en el margen");
        assert_eq!(vencidas.len(), 1);
        assert_eq!(vencidas[0].due_at, "2026-09-01T11:00:00.000Z");

        run_due_rules(&mut connection, instant("2026-09-01T11:00:00Z")).expect("disparar");
        assert!(
            due_rules(&connection, instant("2026-09-01T12:30:00Z"))
                .expect("tras la cita")
                .is_empty()
        );
    }

    #[test]
    fn una_regla_deshabilitada_nunca_vence() {
        let mut connection = database();
        guardar_regla(
            &mut connection,
            &SaveNotificationRuleInput {
                enabled: false,
                ..manual_rule("Silenciado", "2026-09-01T10:00:00Z", "none")
            },
        )
        .expect("crear aviso deshabilitado");

        assert!(
            due_rules(&connection, instant("2026-12-01T10:00:00Z"))
                .expect("vencidas")
                .is_empty()
        );
    }

    // ------------------------------------------------------------- recurrencia

    /// Atajo de lectura: la cita `periods` contada desde el ancla.
    fn cita(ancla: &str, repeat: RepeatRule, periods: u32) -> DateTime<Utc> {
        occurrence(instant(ancla), repeat, periods).expect("cita dentro del calendario")
    }

    #[test]
    fn la_recurrencia_mensual_recorta_el_fin_de_mes_y_recupera_el_dia_original() {
        // Ancla el 31: febrero recorta a 28, pero marzo vuelve al 31 porque
        // cada cita se calcula desde el ancla, no desde la cita recortada.
        let ancla = "2027-01-31T09:00:00Z";
        assert_eq!(
            cita(ancla, RepeatRule::Monthly, 1),
            instant("2027-02-28T09:00:00Z")
        );
        assert_eq!(
            cita(ancla, RepeatRule::Monthly, 2),
            instant("2027-03-31T09:00:00Z")
        );
        assert_eq!(
            cita(ancla, RepeatRule::Monthly, 3),
            instant("2027-04-30T09:00:00Z")
        );
        assert_eq!(
            cita(ancla, RepeatRule::Monthly, 4),
            instant("2027-05-31T09:00:00Z")
        );

        // Ancla el 30: febrero recorta a 28 y marzo vuelve al 30, no al 31.
        let ancla = "2027-01-30T09:00:00Z";
        assert_eq!(
            cita(ancla, RepeatRule::Monthly, 1),
            instant("2027-02-28T09:00:00Z")
        );
        assert_eq!(
            cita(ancla, RepeatRule::Monthly, 2),
            instant("2027-03-30T09:00:00Z")
        );
        assert_eq!(
            cita(ancla, RepeatRule::Monthly, 3),
            instant("2027-04-30T09:00:00Z")
        );

        // Año bisiesto: el mismo ancla del 31 llega al 29 de febrero.
        let ancla = "2028-01-31T09:00:00Z";
        assert_eq!(
            cita(ancla, RepeatRule::Monthly, 1),
            instant("2028-02-29T09:00:00Z")
        );
        assert_eq!(
            cita(ancla, RepeatRule::Monthly, 2),
            instant("2028-03-31T09:00:00Z")
        );

        // Doce saltos consecutivos desde el 31 de enero: ningún febrero
        // arrastra el día del resto del año.
        let ancla = "2027-01-31T09:00:00Z";
        let esperado = [
            "2027-02-28T09:00:00Z",
            "2027-03-31T09:00:00Z",
            "2027-04-30T09:00:00Z",
            "2027-05-31T09:00:00Z",
            "2027-06-30T09:00:00Z",
            "2027-07-31T09:00:00Z",
            "2027-08-31T09:00:00Z",
            "2027-09-30T09:00:00Z",
            "2027-10-31T09:00:00Z",
            "2027-11-30T09:00:00Z",
            "2027-12-31T09:00:00Z",
            "2028-01-31T09:00:00Z",
        ];
        for (indice, fecha) in esperado.iter().enumerate() {
            assert_eq!(
                cita(ancla, RepeatRule::Monthly, indice as u32 + 1),
                instant(fecha),
                "salto {} desde el ancla",
                indice + 1
            );
        }
    }

    #[test]
    fn la_recurrencia_mensual_cruza_el_cambio_de_ano() {
        assert_eq!(
            next_occurrence(
                instant("2027-12-31T09:00:00Z"),
                RepeatRule::Monthly,
                0,
                instant("2027-12-31T12:00:00Z"),
            )
            .expect("salto de año"),
            Some(instant("2028-01-31T09:00:00Z"))
        );

        // Una regla mensual olvidada durante catorce meses coloca una única
        // cita futura, no una por mes perdido, y conserva el día del ancla
        // pese a los febreros intermedios.
        assert_eq!(
            next_occurrence(
                instant("2026-11-30T09:00:00Z"),
                RepeatRule::Monthly,
                0,
                instant("2028-01-05T00:00:00Z"),
            )
            .expect("puesta al día mensual"),
            Some(instant("2028-01-30T09:00:00Z"))
        );
    }

    #[test]
    fn disparos_sucesivos_no_pierden_el_dia_elegido() {
        // El caso que motivó el diseño: el ancla vive en `scheduled_for` y no
        // se reescribe, así que marzo recupera el 31 tras el febrero de 28.
        let mut connection = database();
        let regla = guardar_regla(
            &mut connection,
            &manual_rule("Mensual", "2027-01-31T09:00:00Z", "monthly"),
        )
        .expect("crear aviso mensual");

        let esperado = [
            ("2027-01-31T09:30:00Z", "2027-02-28T09:00:00.000Z"),
            ("2027-02-28T09:30:00Z", "2027-03-31T09:00:00.000Z"),
            ("2027-03-31T09:30:00Z", "2027-04-30T09:00:00.000Z"),
            ("2027-04-30T09:30:00Z", "2027-05-31T09:00:00.000Z"),
        ];
        for (disparo, siguiente) in esperado {
            let actualizada = mark_rule_fired(&connection, &regla.id, instant(disparo))
                .expect("marcar disparada");
            // El ancla nunca se mueve.
            assert_eq!(
                actualizada.scheduled_for.as_deref(),
                Some("2027-01-31T09:00:00.000Z")
            );
            assert_eq!(actualizada.next_occurrence.as_deref(), Some(siguiente));
            assert!(
                due_rules(&connection, instant(disparo))
                    .expect("vencidas tras disparar")
                    .is_empty()
            );
        }
    }

    #[test]
    fn la_recurrencia_diaria_y_semanal_se_pone_al_dia_de_una_sola_vez() {
        assert_eq!(
            next_occurrence(
                instant("2026-01-01T09:00:00Z"),
                RepeatRule::Daily,
                0,
                instant("2026-03-01T10:00:00Z"),
            )
            .expect("puesta al día diaria"),
            Some(instant("2026-03-02T09:00:00Z"))
        );

        assert_eq!(
            next_occurrence(
                instant("2026-01-01T09:00:00Z"),
                RepeatRule::Weekly,
                0,
                instant("2026-01-15T08:00:00Z"),
            )
            .expect("puesta al día semanal"),
            Some(instant("2026-01-15T09:00:00Z"))
        );

        // El margen desplaza el horizonte: con 120 minutos de antelación la
        // cita del 15 a las 09:00 ya está vencida el 15 a las 08:00.
        assert_eq!(
            next_occurrence(
                instant("2026-01-01T09:00:00Z"),
                RepeatRule::Weekly,
                120,
                instant("2026-01-15T08:00:00Z"),
            )
            .expect("puesta al día semanal con margen"),
            Some(instant("2026-01-22T09:00:00Z"))
        );
    }

    #[test]
    fn una_regla_puntual_expone_su_unica_cita_y_luego_ninguna() {
        let ancla = instant("2026-09-01T10:00:00Z");
        assert_eq!(
            next_occurrence(ancla, RepeatRule::None, 0, instant("2026-08-01T00:00:00Z"))
                .expect("antes de la cita"),
            Some(ancla)
        );
        assert_eq!(
            current_occurrence(ancla, RepeatRule::None, 0, instant("2026-08-01T00:00:00Z"))
                .expect("antes de la cita"),
            None
        );
        assert_eq!(
            current_occurrence(ancla, RepeatRule::None, 0, instant("2026-09-01T10:00:00Z"))
                .expect("en la cita"),
            Some(ancla)
        );
        assert_eq!(
            next_occurrence(ancla, RepeatRule::None, 0, instant("2026-09-02T00:00:00Z"))
                .expect("tras la cita"),
            None
        );
    }

    #[test]
    fn editar_el_ancla_reinicia_el_progreso_de_la_regla() {
        let mut connection = database();
        let regla = guardar_regla(
            &mut connection,
            &manual_rule("Reprogramable", "2026-09-01T10:00:00Z", "none"),
        )
        .expect("crear aviso");
        run_due_rules(&mut connection, instant("2026-09-01T10:05:00Z")).expect("disparar");
        assert!(
            due_rules(&connection, instant("2026-09-01T11:00:00Z"))
                .expect("consumida")
                .is_empty()
        );

        let reprogramada = guardar_regla(
            &mut connection,
            &SaveNotificationRuleInput {
                id: Some(regla.id.clone()),
                ..manual_rule("Reprogramable", "2026-09-03T10:00:00Z", "none")
            },
        )
        .expect("reprogramar");
        assert!(reprogramada.last_fired_at.is_none());
        assert_eq!(
            due_rules(&connection, instant("2026-09-03T10:05:00Z"))
                .expect("vencidas tras reprogramar")
                .len(),
            1
        );
    }

    #[test]
    fn una_regla_periodica_solo_genera_un_aviso_por_barrido() {
        let mut connection = database();
        guardar_regla(
            &mut connection,
            &manual_rule("Diario", "2026-09-01T09:00:00Z", "daily"),
        )
        .expect("crear aviso diario");

        assert_eq!(
            run_due_rules(&mut connection, instant("2026-09-05T09:30:00Z")).expect("disparar"),
            1
        );
        assert_eq!(
            run_due_rules(&mut connection, instant("2026-09-05T09:31:00Z")).expect("repetir"),
            0
        );
        assert_eq!(
            run_due_rules(&mut connection, instant("2026-09-06T09:30:00Z")).expect("día siguiente"),
            1
        );

        let bandeja = inbox(
            &connection,
            &NotificationInboxFilter::default(),
            DEFAULT_INBOX_LIMIT,
            0,
        )
        .expect("bandeja");
        assert_eq!(bandeja.total, 2);
    }

    // -------------------------------------------------------------- derivación

    fn sembrar_senales(connection: &Connection) {
        connection
            .execute_batch(
                "INSERT INTO discovery_events(id, app_id, kind, previous_value, current_value, observed_at)
                 VALUES
                   ('11111111-1111-4111-8111-111111111111', 10, 'early_access_changed',
                    'early_access', 'released', '2026-08-01T10:00:00.000Z'),
                   ('22222222-2222-4222-8222-222222222222', 20, 'release_date_changed',
                    '2026-10-01', '2026-12-01', '2026-08-02T10:00:00.000Z'),
                   ('33333333-3333-4333-8333-333333333333', 10, 'early_access_changed',
                    'released', 'early_access', '2026-08-03T10:00:00.000Z');
                 INSERT INTO steam_news_items(
                     app_id, gid, title, content_preview, published_at, feed_label, feed_name
                 ) VALUES
                   (10, '900001', 'Parche 1.4', 'Correcciones y equilibrio.',
                    '2026-08-04T10:00:00.000Z', 'Anuncios', 'steam_community_announcements'),
                   (20, '900002', 'Retraso', 'Aplazamos el lanzamiento.',
                    '2026-08-05T10:00:00.000Z', 'Anuncios', 'steam_community_announcements');
                 INSERT INTO game_reminders(id, app_id, due_at, note)
                 VALUES ('44444444-4444-4444-8444-444444444444', 10,
                         '2026-08-06T10:00:00.000Z', 'Retomar la partida');
                 INSERT INTO game_dlc(
                     app_id, dlc_app_id, title, release_date, metadata_status, updated_at
                 ) VALUES
                   (10, 111, 'Expansión del norte', '12 Mar, 2027', 'success',
                    '2026-08-07T10:00:00.000Z'),
                   (10, 112, '', NULL, 'pending', '2026-08-07T10:00:00.000Z');",
            )
            .expect("sembrar señales oficiales");
    }

    #[test]
    fn deriva_cada_senal_oficial_una_sola_vez() {
        let mut connection = database();
        sembrar_senales(&connection);
        let ahora = instant("2026-08-10T00:00:00Z");

        let primero = derive_official_events(&mut connection, ahora).expect("derivar");
        assert_eq!(primero.early_access_exits, 1);
        assert_eq!(primero.release_date_changes, 1);
        // El juego 20 no está en seguimiento: su publicación no se deriva.
        assert_eq!(primero.official_news, 1);
        assert_eq!(primero.due_reminders, 1);
        // El DLC sin ficha publicada no genera aviso.
        assert_eq!(primero.new_dlc, 1);
        assert_eq!(primero.created, 5);
        assert_eq!(primero.skipped_duplicates, 0);

        let segundo = derive_official_events(&mut connection, ahora).expect("derivar de nuevo");
        assert_eq!(segundo, DerivedEventsReport::default());

        let total: i64 = connection
            .query_row("SELECT COUNT(*) FROM notification_events", [], |row| {
                row.get(0)
            })
            .expect("contar avisos");
        assert_eq!(total, 5);
    }

    #[test]
    fn las_claves_de_deduplicacion_son_estables_y_descriptivas() {
        let mut connection = database();
        sembrar_senales(&connection);
        derive_official_events(&mut connection, instant("2026-08-10T00:00:00Z")).expect("derivar");

        let mut statement = connection
            .prepare("SELECT dedupe_key FROM notification_events ORDER BY dedupe_key ASC")
            .expect("preparar");
        let claves = statement
            .query_map([], |row| row.get::<_, String>(0))
            .expect("consultar")
            .collect::<Result<Vec<_>, _>>()
            .expect("recoger");

        assert_eq!(
            claves,
            vec![
                "dlc_release:10:111".to_string(),
                "early_access_exit:10:2026-08-01T10:00:00.000Z".to_string(),
                "official_news:10:900001".to_string(),
                "release_date_changed:20:2026-08-02T10:00:00.000Z".to_string(),
                "reminder:44444444-4444-4444-8444-444444444444:2026-08-06T10:00:00.000Z"
                    .to_string(),
            ]
        );
    }

    #[test]
    fn el_texto_derivado_cita_la_senal_oficial_sin_inventarla() {
        let mut connection = database();
        sembrar_senales(&connection);
        derive_official_events(&mut connection, instant("2026-08-10T00:00:00Z")).expect("derivar");

        let bandeja = inbox(
            &connection,
            &NotificationInboxFilter::default(),
            DEFAULT_INBOX_LIMIT,
            0,
        )
        .expect("bandeja");

        let salida = bandeja
            .items
            .iter()
            .find(|item| item.kind == "early_access_exit")
            .expect("evento de acceso anticipado");
        assert_eq!(salida.title, "Viajero ha salido de acceso anticipado");
        assert_eq!(salida.severity, "success");
        assert!(salida.body.contains("01/08/2026"));

        let fecha = bandeja
            .items
            .iter()
            .find(|item| item.kind == "release_date_changed")
            .expect("evento de fecha");
        assert_eq!(fecha.severity, "info");
        assert!(fecha.body.contains("«2026-10-01»"));
        assert!(fecha.body.contains("«2026-12-01»"));

        let recordatorio = bandeja
            .items
            .iter()
            .find(|item| item.kind == "reminder_due")
            .expect("evento de recordatorio");
        assert_eq!(recordatorio.severity, "warning");
        assert!(recordatorio.body.contains("Retomar la partida"));

        let dlc = bandeja
            .items
            .iter()
            .find(|item| item.kind == "dlc_release")
            .expect("evento de DLC");
        assert_eq!(dlc.title, "Nuevo DLC de Viajero: Expansión del norte");
        assert!(dlc.body.contains("12 Mar, 2027"));
    }

    #[test]
    fn sin_senales_oficiales_no_se_inventa_ningun_evento() {
        let mut connection = database();

        let informe = derive_official_events(&mut connection, instant("2026-08-10T00:00:00Z"))
            .expect("derivar sobre tablas vacías");

        assert_eq!(informe, DerivedEventsReport::default());
        let bandeja = inbox(
            &connection,
            &NotificationInboxFilter::default(),
            DEFAULT_INBOX_LIMIT,
            0,
        )
        .expect("bandeja vacía");
        assert_eq!(bandeja.total, 0);
        assert_eq!(bandeja.unread, NotificationCounters::default());
    }

    #[test]
    fn un_recordatorio_futuro_no_genera_aviso_hasta_vencer() {
        let mut connection = database();
        connection
            .execute(
                "INSERT INTO game_reminders(id, app_id, due_at, note)
                 VALUES ('55555555-5555-4555-8555-555555555555', 10,
                         '2026-12-01T10:00:00.000Z', '')",
                [],
            )
            .expect("crear recordatorio futuro");

        let antes = derive_official_events(&mut connection, instant("2026-08-10T00:00:00Z"))
            .expect("derivar antes");
        assert_eq!(antes.due_reminders, 0);

        let despues = derive_official_events(&mut connection, instant("2026-12-02T00:00:00Z"))
            .expect("derivar después");
        assert_eq!(despues.due_reminders, 1);
    }

    #[test]
    fn el_barrido_completo_combina_reglas_y_senales() {
        let mut connection = database();
        sembrar_senales(&connection);
        guardar_regla(
            &mut connection,
            &manual_rule("Aviso propio", "2026-08-09T10:00:00Z", "none"),
        )
        .expect("crear aviso");

        let informe = refresh(&mut connection, instant("2026-08-10T00:00:00Z")).expect("barrido");
        assert_eq!(informe.scheduled_events, 1);
        assert_eq!(informe.derived.created, 5);
        assert_eq!(informe.unread.total, 6);
        assert_eq!(informe.unread.success, 1);
        assert_eq!(informe.unread.warning, 1);
        assert_eq!(informe.unread.info, 4);

        let repetido = refresh(&mut connection, instant("2026-08-10T00:10:00Z")).expect("repetir");
        assert_eq!(repetido.scheduled_events, 0);
        assert_eq!(repetido.derived.created, 0);
    }

    // ------------------------------------------------------------------ bandeja

    fn sembrar_bandeja(connection: &Connection) {
        connection
            .execute_batch(
                "INSERT INTO notification_events(
                     id, app_id, kind, severity, title, body, occurred_at, dedupe_key
                 ) VALUES
                   ('aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaa1', 10, 'manual', 'info',
                    'Uno', '', '2026-08-01T10:00:00.000Z', 'prueba:1'),
                   ('aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaa2', 10, 'manual', 'success',
                    'Dos', '', '2026-08-02T10:00:00.000Z', 'prueba:2'),
                   ('aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaa3', 20, 'manual', 'warning',
                    'Tres', '', '2026-08-03T10:00:00.000Z', 'prueba:3'),
                   ('aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaa4', NULL, 'manual', 'critical',
                    'Cuatro', '', '2026-08-04T10:00:00.000Z', 'prueba:4');",
            )
            .expect("sembrar bandeja");
    }

    #[test]
    fn la_bandeja_ordena_pagina_filtra_y_cuenta_por_severidad() {
        let connection = database();
        sembrar_bandeja(&connection);

        let primera =
            inbox(&connection, &NotificationInboxFilter::default(), 2, 0).expect("primera página");
        assert_eq!(primera.total, 4);
        assert_eq!(primera.limit, 2);
        assert_eq!(
            primera
                .items
                .iter()
                .map(|item| item.title.as_str())
                .collect::<Vec<_>>(),
            vec!["Cuatro", "Tres"]
        );
        assert_eq!(primera.unread.total, 4);
        assert_eq!(primera.unread.info, 1);
        assert_eq!(primera.unread.success, 1);
        assert_eq!(primera.unread.warning, 1);
        assert_eq!(primera.unread.critical, 1);

        let segunda =
            inbox(&connection, &NotificationInboxFilter::default(), 2, 2).expect("segunda página");
        assert_eq!(
            segunda
                .items
                .iter()
                .map(|item| item.title.as_str())
                .collect::<Vec<_>>(),
            vec!["Dos", "Uno"]
        );

        let por_juego = inbox(
            &connection,
            &NotificationInboxFilter {
                app_id: Some(10),
                ..NotificationInboxFilter::default()
            },
            DEFAULT_INBOX_LIMIT,
            0,
        )
        .expect("filtro por juego");
        assert_eq!(por_juego.total, 2);
        assert!(por_juego.items.iter().all(|item| item.app_id == Some(10)));
        assert_eq!(por_juego.items[0].game_title.as_deref(), Some("Viajero"));

        let por_severidad = inbox(
            &connection,
            &NotificationInboxFilter {
                severity: Some("warning".to_string()),
                ..NotificationInboxFilter::default()
            },
            DEFAULT_INBOX_LIMIT,
            0,
        )
        .expect("filtro por severidad");
        assert_eq!(por_severidad.total, 1);
        // Los contadores del distintivo son globales y no siguen al filtro.
        assert_eq!(por_severidad.unread.total, 4);

        let ambito = inbox(
            &connection,
            &NotificationInboxFilter {
                scope: Some("inventado".to_string()),
                ..NotificationInboxFilter::default()
            },
            DEFAULT_INBOX_LIMIT,
            0,
        )
        .expect_err("rechazar ámbito desconocido");
        assert_eq!(ambito.code, "validation");

        let severidad = inbox(
            &connection,
            &NotificationInboxFilter {
                severity: Some("urgentísimo".to_string()),
                ..NotificationInboxFilter::default()
            },
            DEFAULT_INBOX_LIMIT,
            0,
        )
        .expect_err("rechazar severidad desconocida");
        assert_eq!(severidad.code, "validation");
    }

    #[test]
    fn marcar_leido_y_descartar_son_idempotentes() {
        let mut connection = database();
        sembrar_bandeja(&connection);
        let id = "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaa1";

        mark_read(&connection, id, instant("2026-08-10T10:00:00Z")).expect("marcar leído");
        mark_read(&connection, id, instant("2026-08-11T10:00:00Z")).expect("repetir marcado");
        let read_at: String = connection
            .query_row(
                "SELECT read_at FROM notification_events WHERE id = ?1",
                [id],
                |row| row.get(0),
            )
            .expect("leer marca");
        assert_eq!(read_at, "2026-08-10T10:00:00.000Z");
        assert_eq!(unread_counters(&connection).expect("contadores").total, 3);

        dismiss_event(&connection, id, instant("2026-08-12T10:00:00Z")).expect("descartar");
        dismiss_event(&connection, id, instant("2026-08-13T10:00:00Z")).expect("repetir descarte");
        let dismissed_at: String = connection
            .query_row(
                "SELECT dismissed_at FROM notification_events WHERE id = ?1",
                [id],
                |row| row.get(0),
            )
            .expect("leer descarte");
        assert_eq!(dismissed_at, "2026-08-12T10:00:00.000Z");

        assert_eq!(
            mark_all_read(&mut connection, instant("2026-08-14T10:00:00Z")).expect("marcar todo"),
            3
        );
        assert_eq!(
            mark_all_read(&mut connection, instant("2026-08-15T10:00:00Z")).expect("repetir todo"),
            0
        );
        assert_eq!(
            unread_counters(&connection).expect("contadores"),
            NotificationCounters::default()
        );

        assert_eq!(
            dismiss_all(&mut connection, instant("2026-08-16T10:00:00Z")).expect("descartar todo"),
            3
        );
        assert_eq!(
            dismiss_all(&mut connection, instant("2026-08-17T10:00:00Z")).expect("repetir"),
            0
        );
        assert_eq!(
            inbox(
                &connection,
                &NotificationInboxFilter::default(),
                DEFAULT_INBOX_LIMIT,
                0
            )
            .expect("bandeja pendiente")
            .total,
            0
        );
        assert_eq!(
            inbox(
                &connection,
                &NotificationInboxFilter {
                    scope: Some("dismissed".to_string()),
                    ..NotificationInboxFilter::default()
                },
                DEFAULT_INBOX_LIMIT,
                0
            )
            .expect("bandeja descartada")
            .total,
            4
        );

        let ausente = mark_read(
            &connection,
            "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbb1",
            instant("2026-08-18T10:00:00Z"),
        )
        .expect_err("aviso inexistente");
        assert_eq!(ausente.code, "not_found");

        let identificador = mark_read(&connection, "no-es-uuid", instant("2026-08-18T10:00:00Z"))
            .expect_err("identificador inválido");
        assert_eq!(identificador.code, "validation");
    }

    #[test]
    fn la_purga_borra_lo_descartado_antiguo_y_respeta_lo_pendiente() {
        let connection = database();
        sembrar_bandeja(&connection);
        connection
            .execute_batch(
                "UPDATE notification_events
                    SET dismissed_at = '2026-06-01T10:00:00.000Z',
                        read_at = '2026-06-01T10:00:00.000Z'
                  WHERE id = 'aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaa1';
                 UPDATE notification_events
                    SET dismissed_at = '2026-08-09T10:00:00.000Z',
                        read_at = '2026-08-09T10:00:00.000Z'
                  WHERE id = 'aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaa2';",
            )
            .expect("preparar descartes");

        let borrados =
            prune_events(&connection, instant("2026-08-10T10:00:00Z"), 30).expect("purgar");
        assert_eq!(borrados, 1);

        let restantes: i64 = connection
            .query_row("SELECT COUNT(*) FROM notification_events", [], |row| {
                row.get(0)
            })
            .expect("contar");
        assert_eq!(restantes, 3);
        let pendientes = inbox(
            &connection,
            &NotificationInboxFilter::default(),
            DEFAULT_INBOX_LIMIT,
            0,
        )
        .expect("bandeja")
        .total;
        assert_eq!(pendientes, 2);

        assert_eq!(
            prune_events(&connection, instant("2026-08-10T10:00:00Z"), 0)
                .expect_err("retención cero")
                .code,
            "validation"
        );
        assert_eq!(
            prune_events(
                &connection,
                instant("2026-08-10T10:00:00Z"),
                MAX_RETENTION_DAYS + 1
            )
            .expect_err("retención desmesurada")
            .code,
            "validation"
        );
    }

    #[test]
    fn eliminar_una_regla_conserva_los_avisos_que_ya_genero() {
        let mut connection = database();
        let regla = guardar_regla(
            &mut connection,
            &manual_rule("Aviso con historia", "2026-09-01T10:00:00Z", "none"),
        )
        .expect("crear aviso");
        run_due_rules(&mut connection, instant("2026-09-01T10:05:00Z")).expect("disparar");

        delete_rule(&connection, &regla.id).expect("eliminar regla");

        let bandeja = inbox(
            &connection,
            &NotificationInboxFilter::default(),
            DEFAULT_INBOX_LIMIT,
            0,
        )
        .expect("bandeja");
        assert_eq!(bandeja.total, 1);
        assert!(bandeja.items[0].rule_id.is_none());
    }
}
