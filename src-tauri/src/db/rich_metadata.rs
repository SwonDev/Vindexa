//! Persistencia de los metadatos enriquecidos de ficha (migraciones 018 y 019).
//!
//! Los tipos del contrato con la interfaz viven en `crate::models`, que se
//! compila aislado en las pruebas de contrato y por eso no puede depender de
//! ningún módulo de base de datos. Aquí quedan su saneado, su validación y su
//! persistencia, y se reexportan para que la ruta habitual siga funcionando. La
//! dependencia siempre apunta de `steam` hacia `db`, nunca al revés.
//!
//! ## Descripciones sin HTML
//!
//! La tienda de Steam devuelve HTML crudo en `detailed_description` y
//! `about_the_game`. Vindexa **no** guarda ese HTML: `steam::store_api` lo
//! convierte a un [`StructuredDescription`] de bloques con texto plano
//! (encabezado, párrafo, lista) y aquí se persiste serializado como JSON. Así la
//! interfaz puede maquetar la ficha sin usar nunca `dangerouslySetInnerHTML`:
//! ningún atributo, script, iframe ni URL `javascript:` sobrevive al parseo.
//!
//! ## Semántica de escritura
//!
//! Todos los campos son opcionales y significan «la respuesta oficial traía este
//! dato». Una respuesta parcial nunca borra un dato ya verificado: la escritura
//! usa `COALESCE`, de modo que aplicar dos veces la misma actualización deja
//! exactamente el mismo estado (idempotencia) y un campo ausente conserva el
//! valor anterior. Los medios sí se reemplazan como conjunto, pero solo cuando
//! la respuesta declara explícitamente su lista (`Some`).

// El cableado a `db::mod` y `commands` llega en el mismo commit de integración
// descrito en el informe de esta tarea; hasta entonces el análisis de código
// muerto no ve a los consumidores de esta API. **Retira este `allow` al añadir
// los `pub use` de `db/mod.rs` y los comandos correspondientes.**
#![allow(dead_code)]

use crate::error::{AppError, AppResult};
pub use crate::models::{
    DescriptionBlock, DrmAssessment, DrmEvidence, DrmState, DrmStateCounts, GameMediaItem,
    GameMediaKind, LogoPosition, RichGameMetadata, StructuredDescription,
};
use rusqlite::{Connection, OptionalExtension, Transaction, params, params_from_iter};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

/// Límite de bloques que aceptamos por descripción estructurada.
pub const MAX_DESCRIPTION_BLOCKS: usize = 120;
/// Límite de elementos de una lista dentro de una descripción.
pub const MAX_LIST_ITEMS: usize = 40;
/// Límite de caracteres de cualquier bloque individual.
pub const MAX_BLOCK_CHARS: usize = 2_000;
/// Límite de caracteres sumados de una descripción estructurada.
pub const MAX_STRUCTURED_CHARS: usize = 20_000;
/// Límite de capturas y vídeos que conservamos por juego.
pub const MAX_MEDIA_ITEMS: usize = 48;
/// Límite de caracteres de un aviso legal o de DRM.
pub const MAX_NOTICE_CHARS: usize = 2_000;
/// Límite de caracteres de cualquier URL persistida.
pub const MAX_URL_CHARS: usize = 2_048;
/// Límite de caracteres del listado de idiomas soportados.
pub const MAX_LANGUAGES_CHARS: usize = 1_000;
/// Límite de evidencias que acompañan a una clasificación de DRM.
pub const MAX_DRM_EVIDENCE: usize = 8;

// ---------------------------------------------------------------------------
// Descripción estructurada
// ---------------------------------------------------------------------------

impl StructuredDescription {
    pub fn is_empty(&self) -> bool {
        self.blocks.is_empty()
    }

    /// Comprueba los límites antes de persistir. Evita que una respuesta
    /// anómala de la tienda haga crecer la base de datos sin control.
    pub fn validate(&self) -> AppResult<()> {
        if self.blocks.len() > MAX_DESCRIPTION_BLOCKS {
            return Err(AppError::validation(
                "La descripción de la tienda supera el número de bloques admitido.",
            ));
        }
        let mut total = 0usize;
        for block in &self.blocks {
            match block {
                DescriptionBlock::Heading { level, text } => {
                    if !(1..=6).contains(level) {
                        return Err(AppError::validation(
                            "La descripción de la tienda declara un encabezado fuera de rango.",
                        ));
                    }
                    total = total.saturating_add(check_block_chars(text)?);
                }
                DescriptionBlock::Paragraph { text } => {
                    total = total.saturating_add(check_block_chars(text)?);
                }
                DescriptionBlock::List { items, .. } => {
                    if items.len() > MAX_LIST_ITEMS {
                        return Err(AppError::validation(
                            "La descripción de la tienda supera el número de elementos de lista admitido.",
                        ));
                    }
                    for item in items {
                        total = total.saturating_add(check_block_chars(item)?);
                    }
                }
            }
        }
        if total > MAX_STRUCTURED_CHARS {
            return Err(AppError::validation(
                "La descripción de la tienda supera el tamaño máximo admitido.",
            ));
        }
        Ok(())
    }
}

fn check_block_chars(text: &str) -> AppResult<usize> {
    let length = text.chars().count();
    if length > MAX_BLOCK_CHARS {
        return Err(AppError::validation(
            "Un bloque de la descripción de la tienda supera el tamaño máximo admitido.",
        ));
    }
    Ok(length)
}

// ---------------------------------------------------------------------------
// DRM
// ---------------------------------------------------------------------------

impl DrmState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Unknown => "unknown",
            Self::DrmFree => "drm_free",
            Self::ThirdPartyDrm => "third_party_drm",
            Self::SteamDrm => "steam_drm",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "unknown" => Some(Self::Unknown),
            "drm_free" => Some(Self::DrmFree),
            "third_party_drm" => Some(Self::ThirdPartyDrm),
            "steam_drm" => Some(Self::SteamDrm),
            _ => None,
        }
    }
}

impl DrmEvidence {
    pub fn new(source: impl Into<String>, matched: impl Into<String>) -> Self {
        Self {
            source: source.into(),
            matched: matched.into(),
        }
    }
}

// ---------------------------------------------------------------------------
// Medios
// ---------------------------------------------------------------------------

impl GameMediaKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Screenshot => "screenshot",
            Self::Movie => "movie",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "screenshot" => Some(Self::Screenshot),
            "movie" => Some(Self::Movie),
            _ => None,
        }
    }
}

// ---------------------------------------------------------------------------
// Posición del logotipo
// ---------------------------------------------------------------------------

const PINNED_POSITIONS: [&str; 4] = [
    "BottomLeft",
    "BottomCenter",
    "CenterCenter",
    "UpperCenter",
];

impl LogoPosition {
    pub fn validate(&self) -> AppResult<()> {
        if !PINNED_POSITIONS.contains(&self.pinned_position.as_str()) {
            return Err(AppError::validation(
                "La posición del logotipo no es una de las publicadas por Steam.",
            ));
        }
        let in_range = |value: f64| value.is_finite() && (0.0..=100.0).contains(&value);
        if !in_range(self.width_pct) || !in_range(self.height_pct) {
            return Err(AppError::validation(
                "El tamaño del logotipo está fuera del rango admitido.",
            ));
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Escritura
// ---------------------------------------------------------------------------

/// Actualización enriquecida. Cada `None` significa «la respuesta oficial no
/// traía este dato»: se conserva lo ya guardado.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RichMetadataUpdate {
    pub detailed_description: Option<StructuredDescription>,
    pub about_the_game: Option<StructuredDescription>,
    pub supported_languages: Option<String>,
    pub website_url: Option<String>,
    pub metacritic_score: Option<u8>,
    pub metacritic_url: Option<String>,
    pub required_age: Option<u32>,
    pub controller_support: Option<String>,
    pub background_url: Option<String>,
    pub library_hero_url: Option<String>,
    pub library_logo_url: Option<String>,
    pub logo_position: Option<LogoPosition>,
    /// Aviso de DRM literal publicado por la tienda.
    pub drm_notice: Option<String>,
    /// Clasificación de DRM. `None` deja intacto lo ya clasificado.
    pub drm: Option<DrmAssessment>,
    /// Conjunto completo de medios. `None` no toca nada; `Some(vec![])` declara
    /// explícitamente que el juego no publica medios y vacía la tabla.
    pub media: Option<Vec<GameMediaItem>>,
}

impl RichMetadataUpdate {
    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }

    fn validate(&self) -> AppResult<()> {
        if let Some(description) = &self.detailed_description {
            description.validate()?;
        }
        if let Some(description) = &self.about_the_game {
            description.validate()?;
        }
        check_optional_text(
            self.supported_languages.as_deref(),
            MAX_LANGUAGES_CHARS,
            "El listado de idiomas de la tienda supera el tamaño admitido.",
        )?;
        check_optional_text(
            self.drm_notice.as_deref(),
            MAX_NOTICE_CHARS,
            "El aviso de DRM de la tienda supera el tamaño admitido.",
        )?;
        check_optional_text(
            self.controller_support.as_deref(),
            64,
            "El soporte de mando declarado por la tienda no es válido.",
        )?;
        if let Some(support) = self.controller_support.as_deref()
            && !matches!(support, "full" | "partial")
        {
            return Err(AppError::validation(
                "El soporte de mando declarado por la tienda no es válido.",
            ));
        }
        for url in [
            self.website_url.as_deref(),
            self.metacritic_url.as_deref(),
            self.background_url.as_deref(),
            self.library_hero_url.as_deref(),
            self.library_logo_url.as_deref(),
        ] {
            check_optional_url(url)?;
        }
        if let Some(score) = self.metacritic_score
            && score > 100
        {
            return Err(AppError::validation(
                "La puntuación de Metacritic recibida está fuera de rango.",
            ));
        }
        if let Some(age) = self.required_age
            && age > 100
        {
            return Err(AppError::validation(
                "La edad recomendada recibida está fuera de rango.",
            ));
        }
        if let Some(position) = &self.logo_position {
            position.validate()?;
        }
        if let Some(drm) = &self.drm
            && drm.evidence.len() > MAX_DRM_EVIDENCE
        {
            return Err(AppError::validation(
                "La clasificación de DRM aporta más evidencias de las admitidas.",
            ));
        }
        if let Some(media) = &self.media {
            validate_media(media)?;
        }
        Ok(())
    }
}

fn check_optional_text(value: Option<&str>, max: usize, message: &str) -> AppResult<()> {
    if let Some(value) = value
        && value.chars().count() > max
    {
        return Err(AppError::validation(message));
    }
    Ok(())
}

fn check_optional_url(value: Option<&str>) -> AppResult<()> {
    let Some(value) = value else {
        return Ok(());
    };
    if value.trim().is_empty() || value.chars().count() > MAX_URL_CHARS {
        return Err(AppError::validation(
            "Una URL recibida de la tienda no es válida.",
        ));
    }
    if !value.starts_with("https://") {
        return Err(AppError::validation(
            "Vindexa solo conserva URLs de la tienda servidas por HTTPS.",
        ));
    }
    Ok(())
}

fn validate_media(media: &[GameMediaItem]) -> AppResult<()> {
    if media.len() > MAX_MEDIA_ITEMS {
        return Err(AppError::validation(
            "La tienda devolvió más medios de los que Vindexa conserva.",
        ));
    }
    let mut seen = HashSet::new();
    for item in media {
        let id = item.media_id.trim();
        if id.is_empty() || id.chars().count() > 128 {
            return Err(AppError::validation(
                "Un medio de la tienda llegó sin identificador utilizable.",
            ));
        }
        if !seen.insert(id.to_owned()) {
            return Err(AppError::validation(
                "La tienda devolvió dos medios con el mismo identificador.",
            ));
        }
        for url in [
            item.thumbnail_url.as_deref(),
            item.full_url.as_deref(),
            item.alt_url.as_deref(),
        ] {
            check_optional_url(url)?;
        }
        if item.thumbnail_url.is_none() && item.full_url.is_none() && item.alt_url.is_none() {
            return Err(AppError::validation(
                "Un medio de la tienda llegó sin ninguna URL oficial utilizable.",
            ));
        }
    }
    Ok(())
}

/// Guarda los metadatos enriquecidos de un juego en una sola transacción.
pub fn save(
    connection: &mut Connection,
    app_id: u32,
    update: &RichMetadataUpdate,
) -> AppResult<()> {
    let transaction = connection.transaction()?;
    save_in_transaction(&transaction, app_id, update)?;
    transaction.commit()?;
    Ok(())
}

/// Variante para componer con otras escrituras dentro de una transacción ya
/// abierta (enriquecimiento en lote, sincronización…).
pub fn save_in_transaction(
    transaction: &Transaction<'_>,
    app_id: u32,
    update: &RichMetadataUpdate,
) -> AppResult<()> {
    if app_id == 0 {
        return Err(AppError::validation("El AppID de Steam no es válido."));
    }
    update.validate()?;

    let detailed_description = encode_description(update.detailed_description.as_ref())?;
    let about_the_game = encode_description(update.about_the_game.as_ref())?;
    let logo_position = match &update.logo_position {
        Some(position) => Some(encode_json(position, "No se pudo preparar la posición del logotipo recibida de Steam.")?),
        None => None,
    };
    let (drm_state, drm_evidence) = match &update.drm {
        Some(assessment) => (
            Some(assessment.state.as_str()),
            Some(encode_json(
                &assessment.evidence,
                "No se pudieron preparar las evidencias de DRM recibidas de Steam.",
            )?),
        ),
        None => (None, None),
    };

    let changed = transaction.execute(
        "UPDATE games SET
            detailed_description = COALESCE(?2, detailed_description),
            about_the_game = COALESCE(?3, about_the_game),
            supported_languages = COALESCE(?4, supported_languages),
            website_url = COALESCE(?5, website_url),
            metacritic_score = COALESCE(?6, metacritic_score),
            metacritic_url = COALESCE(?7, metacritic_url),
            required_age = COALESCE(?8, required_age),
            controller_support = COALESCE(?9, controller_support),
            background_url = COALESCE(?10, background_url),
            library_hero_url = COALESCE(?11, library_hero_url),
            library_logo_url = COALESCE(?12, library_logo_url),
            logo_position_json = COALESCE(?13, logo_position_json),
            drm_notice = COALESCE(?14, drm_notice),
            drm_state = COALESCE(?15, drm_state),
            drm_evidence_json = COALESCE(?16, drm_evidence_json),
            drm_checked_at = CASE
                WHEN ?15 IS NULL THEN drm_checked_at
                ELSE strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
            END,
            updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
         WHERE app_id = ?1",
        params![
            app_id,
            detailed_description,
            about_the_game,
            update.supported_languages,
            update.website_url,
            update.metacritic_score,
            update.metacritic_url,
            update.required_age,
            update.controller_support,
            update.background_url,
            update.library_hero_url,
            update.library_logo_url,
            logo_position,
            update.drm_notice,
            drm_state,
            drm_evidence,
        ],
    )?;
    if changed != 1 {
        return Err(AppError::not_found("El juego ya no está en la biblioteca."));
    }

    if let Some(media) = &update.media {
        replace_media_in_transaction(transaction, app_id, media)?;
    }
    Ok(())
}

/// Reemplaza el conjunto de medios de un juego: inserta o actualiza los que
/// llegan (conservando su `position`) y borra los que ya no vienen.
pub fn replace_media(
    connection: &mut Connection,
    app_id: u32,
    media: &[GameMediaItem],
) -> AppResult<()> {
    let transaction = connection.transaction()?;
    replace_media_in_transaction(&transaction, app_id, media)?;
    transaction.commit()?;
    Ok(())
}

pub fn replace_media_in_transaction(
    transaction: &Transaction<'_>,
    app_id: u32,
    media: &[GameMediaItem],
) -> AppResult<()> {
    if app_id == 0 {
        return Err(AppError::validation("El AppID de Steam no es válido."));
    }
    validate_media(media)?;
    {
        let mut upsert = transaction.prepare_cached(
            "INSERT INTO game_media(
                app_id, media_id, kind, thumbnail_url, full_url, alt_url, position
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
             ON CONFLICT(app_id, media_id) DO UPDATE SET
                kind = excluded.kind,
                thumbnail_url = excluded.thumbnail_url,
                full_url = excluded.full_url,
                alt_url = excluded.alt_url,
                position = excluded.position,
                updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')",
        )?;
        for item in media {
            upsert.execute(params![
                app_id,
                item.media_id.trim(),
                item.kind.as_str(),
                item.thumbnail_url,
                item.full_url,
                item.alt_url,
                item.position,
            ])?;
        }
    }
    if media.is_empty() {
        transaction.execute("DELETE FROM game_media WHERE app_id = ?1", params![app_id])?;
        return Ok(());
    }
    let placeholders = std::iter::repeat_n("?", media.len())
        .collect::<Vec<_>>()
        .join(", ");
    let mut arguments: Vec<Box<dyn rusqlite::ToSql>> = Vec::with_capacity(media.len() + 1);
    arguments.push(Box::new(app_id));
    for item in media {
        arguments.push(Box::new(item.media_id.trim().to_owned()));
    }
    transaction.execute(
        &format!("DELETE FROM game_media WHERE app_id = ?1 AND media_id NOT IN ({placeholders})"),
        params_from_iter(arguments.iter().map(|value| value.as_ref())),
    )?;
    Ok(())
}

fn encode_description(value: Option<&StructuredDescription>) -> AppResult<Option<String>> {
    match value {
        Some(description) => Ok(Some(encode_json(
            description,
            "No se pudo preparar la descripción recibida de Steam.",
        )?)),
        None => Ok(None),
    }
}

fn encode_json<T: Serialize>(value: &T, message: &str) -> AppResult<String> {
    serde_json::to_string(value).map_err(|_| AppError::new("rich_metadata_encode", message))
}

// ---------------------------------------------------------------------------
// Lectura
// ---------------------------------------------------------------------------

/// Lee los metadatos enriquecidos de un juego. Un JSON corrupto en una columna
/// degrada solo ese campo: la ficha sigue abriéndose con el resto de datos
/// verificados en lugar de fallar entera.
pub fn get(connection: &Connection, app_id: u32) -> AppResult<RichGameMetadata> {
    let mut metadata = connection
        .query_row(
            "SELECT detailed_description, about_the_game, supported_languages, website_url,
                    metacritic_score, metacritic_url, required_age, controller_support,
                    background_url, library_hero_url, library_logo_url, logo_position_json,
                    drm_notice, drm_state, drm_evidence_json, drm_checked_at
               FROM games WHERE app_id = ?1",
            params![app_id],
            |row| {
                let state = row
                    .get::<_, String>(13)
                    .map(|value| DrmState::parse(&value).unwrap_or_default())?;
                let evidence = row
                    .get::<_, String>(14)
                    .map(|value| serde_json::from_str(&value).unwrap_or_default())?;
                Ok(RichGameMetadata {
                    app_id,
                    detailed_description: decode_description(row.get(0)?),
                    about_the_game: decode_description(row.get(1)?),
                    supported_languages: row.get(2)?,
                    website_url: row.get(3)?,
                    metacritic_score: row.get(4)?,
                    metacritic_url: row.get(5)?,
                    required_age: row.get(6)?,
                    controller_support: row.get(7)?,
                    background_url: row.get(8)?,
                    library_hero_url: row.get(9)?,
                    library_logo_url: row.get(10)?,
                    logo_position: row
                        .get::<_, Option<String>>(11)?
                        .and_then(|value| serde_json::from_str(&value).ok()),
                    drm_notice: row.get(12)?,
                    drm: DrmAssessment { state, evidence },
                    drm_checked_at: row.get(15)?,
                    screenshots: Vec::new(),
                    movies: Vec::new(),
                })
            },
        )
        .optional()?
        .ok_or_else(|| AppError::not_found("El juego ya no está en la biblioteca."))?;

    let mut statement = connection.prepare(
        "SELECT media_id, kind, thumbnail_url, full_url, alt_url, position
           FROM game_media
          WHERE app_id = ?1
          ORDER BY kind ASC, position ASC, media_id ASC",
    )?;
    let rows = statement
        .query_map(params![app_id], |row| {
            let kind = row.get::<_, String>(1)?;
            Ok((
                kind,
                GameMediaItem {
                    media_id: row.get(0)?,
                    // Sustituido justo debajo por el tipo real de la fila.
                    kind: GameMediaKind::Screenshot,
                    thumbnail_url: row.get(2)?,
                    full_url: row.get(3)?,
                    alt_url: row.get(4)?,
                    position: row.get(5)?,
                },
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    for (kind, mut item) in rows {
        match GameMediaKind::parse(&kind) {
            Some(GameMediaKind::Screenshot) => {
                item.kind = GameMediaKind::Screenshot;
                metadata.screenshots.push(item);
            }
            Some(GameMediaKind::Movie) => {
                item.kind = GameMediaKind::Movie;
                metadata.movies.push(item);
            }
            // El `CHECK` de la migración 019 impide otros valores; si aparece
            // uno, se ignora en vez de inventar una categoría.
            None => {}
        }
    }
    Ok(metadata)
}

/// Recuento por estado de DRM para los filtros de biblioteca.
pub fn drm_state_counts(connection: &Connection) -> AppResult<DrmStateCounts> {
    let mut statement =
        connection.prepare("SELECT drm_state, COUNT(*) FROM games GROUP BY drm_state")?;
    let mut counts = DrmStateCounts::default();
    let rows = statement
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    for (state, total) in rows {
        match DrmState::parse(&state) {
            Some(DrmState::Unknown) => counts.unknown = total,
            Some(DrmState::DrmFree) => counts.drm_free = total,
            Some(DrmState::ThirdPartyDrm) => counts.third_party_drm = total,
            Some(DrmState::SteamDrm) => counts.steam_drm = total,
            None => {}
        }
    }
    Ok(counts)
}

fn decode_description(value: Option<String>) -> Option<StructuredDescription> {
    value
        .as_deref()
        .and_then(|value| serde_json::from_str::<StructuredDescription>(value).ok())
        .filter(|description| !description.is_empty())
}

#[cfg(test)]
mod tests {
    use super::{
        DescriptionBlock, DrmAssessment, DrmEvidence, DrmState, GameMediaItem, GameMediaKind,
        LogoPosition, RichMetadataUpdate, StructuredDescription, drm_state_counts, get,
        replace_media, save,
    };
    use crate::db::migrations;
    use rusqlite::Connection;

    fn database(app_ids: &[u32]) -> Connection {
        let mut connection = Connection::open_in_memory().expect("abrir SQLite");
        migrations::migrate(&mut connection).expect("migrar");
        for app_id in app_ids {
            connection
                .execute(
                    "INSERT INTO games(app_id, title) VALUES (?1, ?2)",
                    rusqlite::params![app_id, format!("Juego {app_id}")],
                )
                .expect("crear juego");
        }
        connection
    }

    fn description() -> StructuredDescription {
        StructuredDescription {
            blocks: vec![
                DescriptionBlock::Heading {
                    level: 2,
                    text: "Sobre el juego".into(),
                },
                DescriptionBlock::Paragraph {
                    text: "Un párrafo con acentuación: cañón, ártico.".into(),
                },
                DescriptionBlock::List {
                    ordered: false,
                    items: vec!["Cooperativo".into(), "Un jugador".into()],
                },
            ],
        }
    }

    fn screenshot(id: u32, position: u32) -> GameMediaItem {
        GameMediaItem {
            media_id: format!("screenshot:{id}"),
            kind: GameMediaKind::Screenshot,
            thumbnail_url: Some(format!(
                "https://shared.steamstatic.com/store_item_assets/steam/apps/620/ss_{id}.600x338.jpg"
            )),
            full_url: Some(format!(
                "https://shared.steamstatic.com/store_item_assets/steam/apps/620/ss_{id}.1920x1080.jpg"
            )),
            alt_url: None,
            position,
        }
    }

    #[test]
    fn persists_and_reads_back_the_full_rich_card_without_html() {
        let mut connection = database(&[620]);
        let update = RichMetadataUpdate {
            detailed_description: Some(description()),
            about_the_game: Some(description()),
            supported_languages: Some("Español, Inglés".into()),
            website_url: Some("https://www.thinkwithportals.com/".into()),
            metacritic_score: Some(95),
            metacritic_url: Some("https://www.metacritic.com/game/pc/portal-2".into()),
            required_age: Some(0),
            controller_support: Some("full".into()),
            background_url: Some(
                "https://store.akamai.steamstatic.com/images/storepagebackground/app/620?t=1".into(),
            ),
            library_hero_url: Some(
                "https://shared.steamstatic.com/store_item_assets/steam/apps/620/library_hero.jpg"
                    .into(),
            ),
            library_logo_url: Some(
                "https://shared.steamstatic.com/store_item_assets/steam/apps/620/logo.png".into(),
            ),
            logo_position: Some(LogoPosition {
                pinned_position: "BottomLeft".into(),
                width_pct: 43.0,
                height_pct: 26.0,
            }),
            drm_notice: None,
            drm: Some(DrmAssessment {
                state: DrmState::DrmFree,
                evidence: vec![DrmEvidence::new(
                    "storeAppdetails",
                    "sin drm_notice ni ext_user_account_notice",
                )],
            }),
            media: Some(vec![screenshot(1, 0), screenshot(2, 1)]),
        };
        save(&mut connection, 620, &update).expect("guardar metadatos ricos");

        let stored = get(&connection, 620).expect("leer metadatos ricos");
        assert_eq!(stored.detailed_description, Some(description()));
        assert_eq!(stored.supported_languages.as_deref(), Some("Español, Inglés"));
        assert_eq!(stored.metacritic_score, Some(95));
        assert_eq!(stored.controller_support.as_deref(), Some("full"));
        assert_eq!(stored.drm.state, DrmState::DrmFree);
        assert_eq!(stored.drm.evidence.len(), 1);
        assert!(stored.drm_checked_at.is_some());
        assert_eq!(stored.screenshots.len(), 2);
        assert!(stored.movies.is_empty());
        assert_eq!(stored.logo_position.expect("posición").width_pct, 43.0);
    }

    #[test]
    fn drm_evidence_is_stored_with_the_documented_json_shape() {
        let mut connection = database(&[620]);
        save(
            &mut connection,
            620,
            &RichMetadataUpdate {
                drm: Some(DrmAssessment {
                    state: DrmState::ThirdPartyDrm,
                    evidence: vec![DrmEvidence::new("drmNotice", "Denuvo Anti-Tamper")],
                }),
                ..RichMetadataUpdate::default()
            },
        )
        .expect("guardar DRM");
        let (state, evidence): (String, String) = connection
            .query_row(
                "SELECT drm_state, drm_evidence_json FROM games WHERE app_id = 620",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .expect("leer columnas de DRM");
        assert_eq!(state, "third_party_drm");
        assert_eq!(
            evidence,
            r#"[{"source":"drmNotice","match":"Denuvo Anti-Tamper"}]"#
        );
    }

    #[test]
    fn applying_the_same_update_twice_leaves_the_same_state() {
        let mut connection = database(&[620]);
        let update = RichMetadataUpdate {
            detailed_description: Some(description()),
            media: Some(vec![screenshot(1, 0), screenshot(2, 1)]),
            drm: Some(DrmAssessment {
                state: DrmState::SteamDrm,
                evidence: vec![DrmEvidence::new("drmNotice", "Steamworks DRM")],
            }),
            ..RichMetadataUpdate::default()
        };
        save(&mut connection, 620, &update).expect("primer guardado");
        let first = get(&connection, 620).expect("primera lectura");
        save(&mut connection, 620, &update).expect("segundo guardado");
        let second = get(&connection, 620).expect("segunda lectura");
        assert_eq!(first.detailed_description, second.detailed_description);
        assert_eq!(first.screenshots, second.screenshots);
        assert_eq!(first.drm, second.drm);
    }

    #[test]
    fn a_partial_response_never_erases_data_already_verified() {
        let mut connection = database(&[620]);
        save(
            &mut connection,
            620,
            &RichMetadataUpdate {
                detailed_description: Some(description()),
                supported_languages: Some("Español".into()),
                ..RichMetadataUpdate::default()
            },
        )
        .expect("guardado completo");
        save(
            &mut connection,
            620,
            &RichMetadataUpdate {
                metacritic_score: Some(95),
                ..RichMetadataUpdate::default()
            },
        )
        .expect("guardado parcial");
        let stored = get(&connection, 620).expect("leer");
        assert_eq!(stored.detailed_description, Some(description()));
        assert_eq!(stored.supported_languages.as_deref(), Some("Español"));
        assert_eq!(stored.metacritic_score, Some(95));
        assert_eq!(stored.drm.state, DrmState::Unknown);
        assert!(stored.drm_checked_at.is_none());
    }

    #[test]
    fn replacing_media_removes_what_no_longer_comes_and_keeps_positions() {
        let mut connection = database(&[620]);
        replace_media(
            &mut connection,
            620,
            &[screenshot(1, 0), screenshot(2, 1), screenshot(3, 2)],
        )
        .expect("primer conjunto");
        let mut moved = screenshot(3, 0);
        moved.alt_url = None;
        replace_media(&mut connection, 620, &[moved, screenshot(1, 1)])
            .expect("conjunto reemplazado");
        let stored = get(&connection, 620).expect("leer");
        assert_eq!(
            stored
                .screenshots
                .iter()
                .map(|item| (item.media_id.as_str(), item.position))
                .collect::<Vec<_>>(),
            vec![("screenshot:3", 0), ("screenshot:1", 1)]
        );
    }

    #[test]
    fn an_explicit_empty_media_set_clears_the_table_but_none_leaves_it_untouched() {
        let mut connection = database(&[620]);
        replace_media(&mut connection, 620, &[screenshot(1, 0)]).expect("conjunto inicial");
        save(
            &mut connection,
            620,
            &RichMetadataUpdate {
                supported_languages: Some("Español".into()),
                ..RichMetadataUpdate::default()
            },
        )
        .expect("actualización sin medios");
        assert_eq!(get(&connection, 620).expect("leer").screenshots.len(), 1);

        save(
            &mut connection,
            620,
            &RichMetadataUpdate {
                media: Some(Vec::new()),
                ..RichMetadataUpdate::default()
            },
        )
        .expect("vaciado explícito");
        assert!(get(&connection, 620).expect("leer").screenshots.is_empty());
    }

    #[test]
    fn rejects_invalid_scores_urls_and_duplicated_media() {
        let mut connection = database(&[620]);
        let invalid_score = save(
            &mut connection,
            620,
            &RichMetadataUpdate {
                metacritic_score: Some(140),
                ..RichMetadataUpdate::default()
            },
        )
        .expect_err("rechazar puntuación fuera de rango");
        assert_eq!(invalid_score.code, "validation");

        let insecure_url = save(
            &mut connection,
            620,
            &RichMetadataUpdate {
                website_url: Some("http://ejemplo.test/".into()),
                ..RichMetadataUpdate::default()
            },
        )
        .expect_err("rechazar URL sin HTTPS");
        assert_eq!(insecure_url.code, "validation");

        let duplicated = replace_media(
            &mut connection,
            620,
            &[screenshot(1, 0), screenshot(1, 1)],
        )
        .expect_err("rechazar medios duplicados");
        assert_eq!(duplicated.code, "validation");

        let unsupported_pad = save(
            &mut connection,
            620,
            &RichMetadataUpdate {
                controller_support: Some("gamepad".into()),
                ..RichMetadataUpdate::default()
            },
        )
        .expect_err("rechazar soporte de mando desconocido");
        assert_eq!(unsupported_pad.code, "validation");
    }

    #[test]
    fn rejects_writes_for_games_that_left_the_library() {
        let mut connection = database(&[620]);
        let error = save(
            &mut connection,
            730,
            &RichMetadataUpdate {
                supported_languages: Some("Español".into()),
                ..RichMetadataUpdate::default()
            },
        )
        .expect_err("rechazar AppID ausente");
        assert_eq!(error.code, "not_found");
    }

    #[test]
    fn a_corrupted_description_degrades_only_that_field() {
        let mut connection = database(&[620]);
        save(
            &mut connection,
            620,
            &RichMetadataUpdate {
                detailed_description: Some(description()),
                supported_languages: Some("Español".into()),
                ..RichMetadataUpdate::default()
            },
        )
        .expect("guardar");
        connection
            .execute(
                "UPDATE games SET detailed_description = '{no es json' WHERE app_id = 620",
                [],
            )
            .expect("corromper columna");
        let stored = get(&connection, 620).expect("leer pese a la corrupción");
        assert_eq!(stored.detailed_description, None);
        assert_eq!(stored.supported_languages.as_deref(), Some("Español"));
    }

    #[test]
    fn the_structured_description_serializes_with_the_contract_the_frontend_expects() {
        let json = serde_json::to_string(&description()).expect("serializar descripción");
        assert_eq!(
            json,
            r#"{"blocks":[{"kind":"heading","level":2,"text":"Sobre el juego"},{"kind":"paragraph","text":"Un párrafo con acentuación: cañón, ártico."},{"kind":"list","ordered":false,"items":["Cooperativo","Un jugador"]}]}"#
        );
    }

    #[test]
    fn media_and_drm_states_serialize_with_the_documented_values() {
        assert_eq!(
            serde_json::to_string(&screenshot(7, 3)).expect("serializar medio"),
            r#"{"mediaId":"screenshot:7","kind":"screenshot","thumbnailUrl":"https://shared.steamstatic.com/store_item_assets/steam/apps/620/ss_7.600x338.jpg","fullUrl":"https://shared.steamstatic.com/store_item_assets/steam/apps/620/ss_7.1920x1080.jpg","altUrl":null,"position":3}"#
        );
        assert_eq!(
            serde_json::to_string(&DrmState::ThirdPartyDrm).expect("serializar estado"),
            r#""third_party_drm""#
        );
        assert_eq!(
            serde_json::to_string(&super::DrmStateCounts::default()).expect("serializar recuentos"),
            r#"{"unknown":0,"drmFree":0,"thirdPartyDrm":0,"steamDrm":0}"#
        );
    }

    #[test]
    fn counts_games_by_drm_state_for_library_filters() {
        let mut connection = database(&[10, 20, 30, 40]);
        let states = [
            (10, DrmState::DrmFree),
            (20, DrmState::ThirdPartyDrm),
            (30, DrmState::ThirdPartyDrm),
        ];
        for (app_id, state) in states {
            save(
                &mut connection,
                app_id,
                &RichMetadataUpdate {
                    drm: Some(DrmAssessment {
                        state,
                        evidence: Vec::new(),
                    }),
                    ..RichMetadataUpdate::default()
                },
            )
            .expect("clasificar");
        }
        let counts = drm_state_counts(&connection).expect("contar");
        assert_eq!(counts.drm_free, 1);
        assert_eq!(counts.third_party_drm, 2);
        assert_eq!(counts.steam_drm, 0);
        assert_eq!(counts.unknown, 1);
    }

    #[test]
    fn media_rows_disappear_with_the_game_they_belong_to() {
        let mut connection = database(&[620]);
        replace_media(&mut connection, 620, &[screenshot(1, 0)]).expect("guardar medios");
        connection
            .execute("DELETE FROM games WHERE app_id = 620", [])
            .expect("borrar juego");
        let remaining: i64 = connection
            .query_row("SELECT COUNT(*) FROM game_media", [], |row| row.get(0))
            .expect("contar medios");
        assert_eq!(remaining, 0);
    }
}
