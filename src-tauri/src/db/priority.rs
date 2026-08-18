//! Motor de prioridad dinámica y modelo de gustos local (migración 024).
//!
//! # Qué resuelve
//!
//! La prioridad de un juego no es un número que se fija una vez: cambia con lo
//! que va pasando. Terminar un juego debe bajarlo aunque se siga jugando, y
//! otros deben subir en su lugar. Este módulo calcula esa puntuación a partir de
//! hechos que ya están en SQLite —estado, progreso, sesiones, fechas, tiempo
//! jugado— y guarda **por qué** ha salido cada número, para que la interfaz
//! pueda explicarlo con palabras en vez de enseñar un ranking opaco.
//!
//! # Tres promesas
//!
//! 1. **Determinista.** Con el mismo `now` y la misma base, dos ejecuciones dan
//!    exactamente el mismo resultado. No hay aleatoriedad, no se lee el reloj
//!    del sistema dentro del cálculo y ningún recorrido de `HashMap` llega a la
//!    salida: la acumulación usa `BTreeMap` y todos los desempates son
//!    explícitos.
//! 2. **Explicable.** Cada juego guarda sus señales en `priority_signals` con su
//!    peso y una frase legible en español, y una razón corta en
//!    `game_personal.priority_reason`.
//! 3. **Auditable.** Se cumple la identidad
//!    `BASE_SCORE + Σ pesos == priority_score`, incluidos el techo de juego
//!    cerrado y el recorte a la escala 0-100, que también se registran como
//!    señales. Si sumas lo que ves, sale el número que se guardó. La prueba
//!    `signal_weights_reconstruct_the_stored_score` lo verifica.
//!
//! # La prioridad manual manda
//!
//! `priority_locked = 1` significa que la persona usuaria ancló su prioridad
//! manual (`game_personal.priority`, 0-5). En ese caso la puntuación derivada
//! **se sigue calculando y guardando**, pero no decide el orden: el orden lo
//! decide la prioridad manual proyectada sobre la misma escala 0-100 (véase
//! [`effective_score`]). Así la interfaz puede enseñar las dos a la vez —«tu
//! prioridad manual dice 5, las señales dicen 2»— en lugar de pisar una con otra
//! en silencio. [`PriorityExplanation::manual_override`] entrega esa frase ya
//! redactada.
//!
//! # Privacidad
//!
//! El modelo de gustos ([`learn_taste`]) se calcula **íntegramente en local**, a
//! partir de filas que ya están en la base del equipo, y **jamás sale de él**:
//! no se envía a ningún servidor, no viaja en telemetría (que no existe, según
//! `PRIVACY.md`) y no se usa como parámetro de ninguna petición remota. Es el
//! modelo el que va al dato ya descargado, nunca al revés: [`score_upcoming`]
//! puntúa en local candidatos que otro módulo trajo, sin contarle a nadie qué
//! géneros, estudios o etiquetas prefiere la persona usuaria. Una copia de
//! seguridad sí incluye `taste_weights`, igual que el resto de la base.
//!
//! # Fronteras con el resto de `db`
//!
//! - Este módulo **no habla con la red**. `upcoming_releases` la rellena quien
//!   descargue los candidatos, llamando a [`upsert_upcoming`].
//! - No toca `game_personal.updated_at`: recalcular no es una edición de la
//!   persona usuaria y ensuciar esa marca rompería los recibos de
//!   `db::library_dnd` y el historial de actividad.

use crate::error::{AppError, AppResult};
use chrono::{DateTime, NaiveDate, SecondsFormat, Utc};
use rusqlite::{Connection, OptionalExtension, Row, params};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use url::Url;
use uuid::Uuid;

// ---------------------------------------------------------------------------
// Constantes del motor de prioridad
//
// Ninguna de estas constantes es un número mágico: cada una lleva el criterio
// que la justifica. Cambiarlas cambia el orden que ve la persona usuaria, así
// que se tocan a conciencia y con las pruebas delante.
// ---------------------------------------------------------------------------

/// Punto de partida neutro de la escala. Un juego recién importado, sin ninguna
/// señal, se queda aquí: ni arriba ni abajo. Se elige 40 y no 50 para que el
/// espacio por encima (60 puntos) dé margen a los impulsos acumulados sin
/// saturar constantemente, y el espacio por debajo (40) baste para hundir un
/// juego cerrado.
pub const BASE_SCORE: f64 = 40.0;

/// Penalización máxima por haber terminado un juego, aplicada el mismo día.
/// Es fuerte porque es exactamente lo que pidió la persona usuaria: terminar
/// algo debe bajarlo aunque se siga jugando. No es total porque seguir jugando
/// un juego terminado es legítimo y no debe desaparecer de la lista.
const COMPLETED_PENALTY: f64 = 30.0;

/// Semivida de la penalización por terminar: a los 120 días pesa la mitad.
/// Cuatro meses es el orden de magnitud en el que un juego terminado deja de
/// competir con lo que está vivo, pero todavía se recuerda como reciente.
const COMPLETED_HALF_LIFE_DAYS: f64 = 120.0;

/// Factor aplicado cuando sabemos que un juego está terminado (progreso 100)
/// pero no tenemos ninguna fecha con la que decaer. No podemos afirmar que sea
/// antiguo, así que se aplica una penalización intermedia en vez de la máxima.
const COMPLETED_WITHOUT_DATE_FACTOR: f64 = 0.75;

/// Penalización máxima por abandonar. Mayor que la de terminar: terminar es un
/// final feliz y abandonar es una renuncia explícita, que además la persona
/// usuaria tuvo que escribir a mano.
const ABANDONED_PENALTY: f64 = 45.0;

/// Semivida de la penalización por abandonar: 240 días. Dura el doble que la de
/// terminar porque la decisión es más firme y revertirla exige retomar el juego,
/// no solo que pase el tiempo.
const ABANDONED_HALF_LIFE_DAYS: f64 = 240.0;

/// Progreso mínimo para considerar que hay una partida viva que retomar.
const PROGRESS_ALIVE_MIN: u8 = 20;
/// Progreso máximo del tramo vivo. Por encima el juego está en su recta final y
/// el impulso baja: a un 95 % le queda poco y no compite con lo que está a medias.
const PROGRESS_ALIVE_MAX: u8 = 90;
/// Progreso donde el impulso es máximo. A mitad larga es donde más cuesta
/// retomar y donde más se pierde si el juego se enfría.
const PROGRESS_ALIVE_PEAK: f64 = 60.0;
/// Impulso garantizado en los extremos del tramo vivo (20 % y 90 %).
const PROGRESS_ALIVE_MIN_WEIGHT: f64 = 12.0;
/// Impulso en el punto de máxima urgencia (60 %).
const PROGRESS_ALIVE_MAX_WEIGHT: f64 = 24.0;

/// Suelo garantizado de un juego con progreso vivo. Como las únicas señales
/// negativas son «terminado» y «abandonado» —incompatibles por definición con el
/// progreso vivo—, cualquier juego en el tramo 20-90 % puntúa al menos esto.
const LIVE_PROGRESS_FLOOR: f64 = BASE_SCORE + PROGRESS_ALIVE_MIN_WEIGHT;

/// Techo de un juego cerrado (terminado o abandonado). Es estrictamente menor
/// que [`LIVE_PROGRESS_FLOOR`], y por eso la promesa «ninguno sube por encima de
/// uno sin terminar con progreso vivo» se cumple SIEMPRE, sin depender del
/// decaimiento ni de cuántos impulsos acumule el juego cerrado. La prueba
/// `settled_games_never_outrank_live_progress` lo fija.
const SETTLED_CEILING: f64 = LIVE_PROGRESS_FLOOR - 2.0;

/// La promesa «un juego cerrado nunca adelanta a uno con progreso vivo» no se
/// comprueba en una prueba: se comprueba al compilar. Si alguien toca una
/// constante y la rompe, el proyecto deja de compilar.
const _: () = assert!(SETTLED_CEILING < LIVE_PROGRESS_FLOOR);

/// Impulso máximo por actividad reciente, sumando las tres evidencias.
const RECENT_ACTIVITY_MAX: f64 = 18.0;
/// Tramo del impulso reciente que aporta el tiempo jugado de las dos últimas
/// semanas que publica Steam.
const RECENT_PLAYTIME_WEIGHT: f64 = 10.0;
/// Saturación del tiempo reciente: 10 h en dos semanas ya es dedicación plena.
const RECENT_PLAYTIME_SATURATION_MINUTES: f64 = 600.0;
/// Tramo del impulso reciente que aporta la última partida registrada.
const LAST_PLAYED_WEIGHT: f64 = 6.0;
/// Semivida de la última partida: tres semanas. Es el plazo tras el cual una
/// partida deja de sentirse «en curso».
const LAST_PLAYED_HALF_LIFE_DAYS: f64 = 21.0;
/// Tramo del impulso reciente que aportan las sesiones anotadas a mano.
const SESSION_WEIGHT: f64 = 4.0;
/// Ventana de sesiones consideradas recientes: seis semanas.
const SESSION_WINDOW_DAYS: i64 = 42;
/// Sesiones a partir de las cuales el tramo de sesiones satura.
const SESSION_SATURATION: f64 = 4.0;

/// Impulso de una fecha objetivo que aún no ha llegado, el mismo día del plazo.
const TARGET_UPCOMING_MAX: f64 = 16.0;
/// Semivida del impulso de una fecha objetivo futura: dos semanas.
const TARGET_HALF_LIFE_DAYS: f64 = 14.0;
/// Impulso al que tiende una fecha objetivo ya vencida. Supera al de la fecha
/// futura porque un plazo incumplido es la señal más accionable que existe.
const TARGET_OVERDUE_MAX: f64 = 20.0;
/// Semivida con la que el retraso acerca el impulso a [`TARGET_OVERDUE_MAX`].
const TARGET_OVERDUE_HALF_LIFE_DAYS: f64 = 7.0;

/// Impulso por estar fijado. Es intención explícita reciente y por eso pesa más
/// que cualquier otra señal binaria.
const PINNED_WEIGHT: f64 = 12.0;
/// Impulso por estar en seguimiento: interés declarado, pero menos comprometido
/// que fijar.
const TRACKING_WEIGHT: f64 = 6.0;
/// Impulso por estar instalado: elimina la fricción de empezar, pero no dice
/// nada sobre las ganas. Pequeño a propósito.
const INSTALLED_WEIGHT: f64 = 4.0;

/// Impulso máximo por biblioteca muerta: juegos que nunca se estrenaron y llevan
/// mucho en la estantería. Deliberadamente por debajo de
/// [`PROGRESS_ALIVE_MIN_WEIGHT`] para que rescatar nunca adelante a retomar.
const DORMANT_MAX: f64 = 8.0;
/// Antigüedad mínima para hablar de biblioteca muerta: medio año.
const DORMANT_MIN_DAYS: f64 = 180.0;
/// Antigüedad a la que el impulso de biblioteca muerta llega a la mitad de su
/// techo: un año.
const DORMANT_HALF_LIFE_DAYS: f64 = 365.0;

/// Valoración a partir de la cual empieza a haber impulso.
const RATING_THRESHOLD: u8 = 8;
/// Impulso máximo por valoración (un 10 sobre 10).
const RATING_WEIGHT: f64 = 5.0;

/// Impulso máximo por afinidad con el modelo de gustos local. Pequeño porque el
/// modelo es una inferencia, no un hecho: no debe mandar sobre el progreso real.
const TASTE_AFFINITY_WEIGHT: f64 = 4.0;

/// Señales que existen para cuadrar la aritmética y no para explicar nada. Se
/// guardan (la identidad de suma debe cerrar) pero nunca encabezan la razón.
const BOOKKEEPING_SIGNALS: [&str; 2] = ["settled_ceiling", "scale_clamp"];

/// Peso mínimo para que una señal secundaria merezca aparecer en la razón.
const REASON_SECONDARY_THRESHOLD: f64 = 3.0;

/// Número de juegos destacados que devuelve un recálculo.
const RECOMPUTE_HIGHLIGHTS: usize = 8;

/// Tope de juegos que se recalculan de una sentada. Una biblioteca de Steam
/// grande ronda los pocos miles; este límite protege de una base manipulada.
const MAX_RECOMPUTE_GAMES: usize = 100_000;

// ---------------------------------------------------------------------------
// Constantes del modelo de gustos
// ---------------------------------------------------------------------------

/// Saturación del tiempo jugado como evidencia de gusto: 50 h. Más allá, más
/// horas no dicen mucho más («me encanta» ya estaba dicho).
const TASTE_PLAYTIME_SATURATION_MINUTES: f64 = 3_000.0;
/// Tramo positivo que aporta el tiempo jugado. Es la evidencia más honesta que
/// existe: lo que se juega, gusta.
const TASTE_PLAYTIME_PULL: f64 = 0.60;
/// Aporte positivo por haber terminado el juego.
const TASTE_COMPLETED_PULL: f64 = 0.30;
/// Aporte positivo por tenerlo fijado.
const TASTE_PINNED_PULL: f64 = 0.15;
/// Aporte positivo máximo por valoración alta (un 10 sobre 10).
const TASTE_HIGH_RATING_PULL: f64 = 0.25;
/// Aporte negativo máximo por valoración baja (un 1 sobre 10).
const TASTE_LOW_RATING_PUSH: f64 = 0.35;
/// Valoración a partir de la cual la nota cuenta como positiva.
const TASTE_HIGH_RATING_THRESHOLD: u8 = 8;
/// Valoración por debajo de la cual la nota cuenta como negativa.
const TASTE_LOW_RATING_THRESHOLD: u8 = 4;
/// Aporte negativo por haber abandonado el juego.
const TASTE_ABANDONED_PUSH: f64 = 0.50;
/// Aporte negativo por tenerlo instalado desde hace mucho y sin estrenar: se
/// eligió, se descargó y aun así no apetece.
const TASTE_SHELVED_PUSH: f64 = 0.25;
/// Antigüedad mínima para que «instalado y sin estrenar» sea evidencia.
const TASTE_SHELVED_MIN_DAYS: f64 = 180.0;
/// Aporte negativo de un `not_interested` explícito sobre un juego propio.
const TASTE_NOT_INTERESTED_PUSH: f64 = 0.40;
/// Aporte positivo de un `interested` explícito.
const TASTE_INTERESTED_PULL: f64 = 0.20;
/// Aporte negativo de un próximo lanzamiento descartado. Menor que abandonar un
/// juego propio: descartar una sugerencia es evidencia más débil que renunciar a
/// algo que ya se estaba jugando.
const TASTE_DISMISSED_UPCOMING_PUSH: f64 = 0.35;

/// Fuerza del previo bayesiano, en número de juegos neutros equivalentes. Con
/// cinco, un género con 3 juegos entusiastas se queda en 3/8 = 0,375 y uno con
/// 60 llega a 60/65 = 0,923: exactamente la asimetría que se busca.
const TASTE_PRIOR_STRENGTH: f64 = 5.0;
/// Media previa. Sin evidencia, ninguna faceta gusta ni disgusta.
const TASTE_PRIOR_MEAN: f64 = 0.0;
/// Peso por debajo del cual una faceta no se guarda: es ruido y ensuciaría la
/// tabla con miles de filas irrelevantes.
const TASTE_MIN_STORED_WEIGHT: f64 = 0.01;
/// Facetas destacadas que devuelve un aprendizaje.
const TASTE_HIGHLIGHTS: usize = 12;

/// Denominador de la escala de coincidencia. Dos coincidencias fuertes —por
/// ejemplo un género muy jugado y el estudio— ya saturan el 100 %.
const UPCOMING_MATCH_SATURATION: f64 = 2.0;
/// Facetas como mucho que se nombran en un `match_reason`.
const UPCOMING_REASON_FACETS: usize = 3;
/// Minutos a partir de los cuales la razón menciona horas concretas en vez de
/// hablar de interés en abstracto. Una hora es el mínimo defendible.
const UPCOMING_HOURS_THRESHOLD_MINUTES: u64 = 60;
/// Peso mínimo para que una faceta merezca aparecer en la razón.
const UPCOMING_REASON_MIN_WEIGHT: f64 = 0.05;

// ---------------------------------------------------------------------------
// Límites de validación
// ---------------------------------------------------------------------------

/// Veredictos admitidos, iguales al `CHECK` de la migración 024.
pub const TASTE_VERDICTS: [&str; 3] = ["interested", "not_interested", "owned_already"];
/// Superficies desde las que se puede opinar. Es una lista cerrada para que la
/// columna no acabe siendo texto libre venido del frontend.
pub const TASTE_SURFACES: [&str; 4] = ["upcoming", "library", "discovery", "detail"];
/// Procedencias admitidas de un próximo lanzamiento, iguales al `CHECK` de 024.
pub const UPCOMING_SOURCES: [&str; 3] = ["store", "library_relation", "manual"];

/// Tope de candidatos importados de una sentada.
const MAX_UPCOMING_BATCH: usize = 2_000;
/// Longitud máxima del título de un candidato.
const MAX_UPCOMING_TITLE: usize = 200;
/// Longitud máxima de la descripción corta de un candidato.
const MAX_UPCOMING_DESCRIPTION: usize = 2_000;
/// Número máximo de géneros o categorías por candidato.
const MAX_UPCOMING_FACETS: usize = 30;
/// Longitud máxima de un valor de faceta.
const MAX_FACET_VALUE: usize = 120;
/// Longitud máxima de cualquier URL persistida.
const MAX_URL_LENGTH: usize = 2_048;
/// Tope de resultados de [`list_upcoming`] y [`list_priority_ranking`].
const MAX_LIST_LIMIT: u32 = 500;

/// Hosts oficiales desde los que Steam sirve el arte de una ficha. Es la misma
/// familia de hosts que ya autoriza la CSP de la ventana principal; se repite
/// aquí porque la lista equivalente de `steam::store_api` es privada. Validar
/// aquí evita persistir una URL que después la ventana no podría pintar.
const ALLOWED_ART_HOSTS: [&str; 9] = [
    "shared.steamstatic.com",
    "shared.cloudflare.steamstatic.com",
    "shared.akamai.steamstatic.com",
    "shared.fastly.steamstatic.com",
    "cdn.cloudflare.steamstatic.com",
    "cdn.akamai.steamstatic.com",
    "cdn.fastly.steamstatic.com",
    "store.akamai.steamstatic.com",
    "media.steampowered.com",
];

// ---------------------------------------------------------------------------
// Facetas
// ---------------------------------------------------------------------------

/// Dimensiones sobre las que se aprende el gusto. Coinciden con el `CHECK` de
/// `taste_weights.facet` en la migración 024.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Facet {
    Genre,
    Category,
    Developer,
    Publisher,
    Tag,
}

impl Facet {
    /// Clave persistida en `taste_weights.facet`.
    pub const fn key(self) -> &'static str {
        match self {
            Self::Genre => "genre",
            Self::Category => "category",
            Self::Developer => "developer",
            Self::Publisher => "publisher",
            Self::Tag => "tag",
        }
    }

    /// Peso relativo de la faceta al puntuar una coincidencia.
    ///
    /// El estudio pesa más que el género porque «lo hace la misma gente» predice
    /// mejor que «es del mismo género»; la categoría pesa menos porque etiquetas
    /// como «Un jugador» las comparte media tienda y separan poco.
    const fn importance(self) -> f64 {
        match self {
            Self::Developer => 1.2,
            Self::Genre | Self::Tag => 1.0,
            Self::Publisher => 0.8,
            Self::Category => 0.6,
        }
    }

    fn from_key(value: &str) -> Option<Self> {
        match value {
            "genre" => Some(Self::Genre),
            "category" => Some(Self::Category),
            "developer" => Some(Self::Developer),
            "publisher" => Some(Self::Publisher),
            "tag" => Some(Self::Tag),
            _ => None,
        }
    }
}

/// Clave de acumulación: faceta más valor normalizado en minúsculas. La
/// normalización solo se usa para agrupar; lo que se guarda y se enseña es el
/// representante original (véase [`FacetEvidence::display`]).
type FacetKey = (Facet, String);

fn normalize_facet_value(value: &str) -> String {
    value.trim().to_lowercase()
}

// ---------------------------------------------------------------------------
// Modelos del motor de prioridad
// ---------------------------------------------------------------------------

/// Una señal con su peso y su explicación en español.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PrioritySignal {
    /// Clave estable de la señal, en `snake_case`. Es la que va a
    /// `priority_signals.signal` y la que puede usar la interfaz para elegir un
    /// icono. No se traduce.
    pub signal: String,
    /// Aporte a la puntuación. Positivo sube, negativo baja.
    pub weight: f64,
    /// Frase completa en español que explica la señal.
    pub detail: String,
}

/// Resultado del cálculo puro: exactamente puntuación, señales y razón.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PriorityOutcome {
    pub score: f64,
    pub signals: Vec<PrioritySignal>,
    pub reason: String,
}

/// Todo lo que el cálculo necesita saber de un juego. Es un `struct` plano y sin
/// dependencias de SQLite para que la función de cálculo sea pura y se pueda
/// probar sin base de datos.
#[derive(Debug, Clone, PartialEq)]
pub struct PriorityInput {
    pub app_id: u32,
    pub title: String,
    /// Progreso declarado, 0-100.
    pub progress: u8,
    /// Prioridad manual, 0-5.
    pub manual_priority: u8,
    /// La persona usuaria ancló su prioridad manual.
    pub locked: bool,
    pub installed: bool,
    pub pinned: bool,
    pub tracking: bool,
    /// Valoración personal, 1-10.
    pub rating: Option<u8>,
    pub completed_at: Option<NaiveDate>,
    pub abandoned_at: Option<NaiveDate>,
    pub target_date: Option<NaiveDate>,
    pub playtime_minutes: u64,
    /// Minutos de las dos últimas semanas, tal y como los publica Steam.
    pub playtime_recent_minutes: u64,
    pub last_played_at: Option<DateTime<Utc>>,
    /// Momento en que el juego entró en la biblioteca. Es lo más parecido a
    /// «cuándo lo compré» que existe en local.
    pub imported_at: Option<DateTime<Utc>>,
    /// Sesiones terminadas dentro de [`SESSION_WINDOW_DAYS`].
    pub recent_sessions: u32,
    /// Afinidad con el modelo de gustos, 0-1.
    pub taste_affinity: f64,
}

/// Un juego en el ranking, con las dos prioridades a la vista.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PriorityRanking {
    pub app_id: u32,
    pub title: String,
    /// Puntuación derivada, 0-100.
    pub score: f64,
    /// Puntuación que decide el orden: la derivada, o la manual proyectada si
    /// está anclada.
    pub effective_score: f64,
    /// Prioridad 0-5 que se deduce de la puntuación derivada.
    pub derived_priority: u8,
    /// Prioridad 0-5 escrita a mano.
    pub manual_priority: u8,
    pub locked: bool,
    pub reason: String,
}

/// Resumen de un recálculo completo.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PriorityRecomputeReport {
    /// Juegos con organización personal considerados.
    pub evaluated: u32,
    /// Juegos cuya puntuación cambió respecto a la guardada.
    pub updated: u32,
    /// Juegos con la prioridad manual anclada.
    pub locked: u32,
    /// Juegos cerrados (terminados o abandonados).
    pub settled: u32,
    /// Filas escritas en `priority_signals`.
    pub signals_written: u32,
    /// Cabecera del ranking resultante.
    pub highlights: Vec<PriorityRanking>,
    pub computed_at: String,
}

/// Explicación completa de la prioridad de un juego.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PriorityExplanation {
    pub app_id: u32,
    pub title: String,
    pub score: f64,
    pub effective_score: f64,
    pub derived_priority: u8,
    pub manual_priority: u8,
    pub locked: bool,
    pub reason: String,
    pub computed_at: Option<String>,
    /// Frase que compara ambas prioridades cuando la manual está anclada y no
    /// coincide con lo que dicen las señales. `None` si no hay conflicto que
    /// contar.
    pub manual_override: Option<String>,
    /// Señales ordenadas por peso absoluto descendente.
    pub signals: Vec<PrioritySignal>,
}

// ---------------------------------------------------------------------------
// Núcleo puro del cálculo
// ---------------------------------------------------------------------------

fn saturate(value: f64) -> f64 {
    value.clamp(0.0, 1.0)
}

/// Decaimiento exponencial por semivida: a `half_life_days` vale 0,5.
fn half_life_decay(days: f64, half_life_days: f64) -> f64 {
    if days <= 0.0 {
        return 1.0;
    }
    0.5_f64.powf(days / half_life_days)
}

/// Redondeo a cuatro decimales. Mantiene la puntuación legible y hace que
/// guardar y volver a leer devuelva exactamente el mismo `f64`.
fn round4(value: f64) -> f64 {
    (value * 10_000.0).round() / 10_000.0
}

fn signal(name: &str, weight: f64, detail: impl Into<String>) -> PrioritySignal {
    PrioritySignal {
        signal: name.to_string(),
        weight: round4(weight),
        detail: detail.into(),
    }
}

fn days_between(from: NaiveDate, to: NaiveDate) -> f64 {
    (to - from).num_days() as f64
}

/// Formatea una cantidad de días en español, sin decimales y sin plural roto.
fn humanize_days(days: f64) -> String {
    let days = days.round().max(0.0) as i64;
    match days {
        0 => "hoy".to_string(),
        1 => "hace 1 día".to_string(),
        _ => format!("hace {days} días"),
    }
}

/// Proyecta la prioridad manual (0-5) sobre la escala derivada (0-100). Cada
/// punto manual vale 20 para que ambas fuentes se puedan ordenar juntas sin que
/// una aplaste a la otra.
pub fn effective_score(locked: bool, manual_priority: u8, score: f64) -> f64 {
    if locked {
        round4(f64::from(manual_priority.min(5)) * 20.0)
    } else {
        round4(score)
    }
}

/// Traduce una puntuación 0-100 a la escala 0-5 que la interfaz ya conoce.
/// Los cortes están repartidos para que la franja neutra (`BASE_SCORE`) caiga en
/// 2 y hagan falta señales reales para llegar a 4 o 5.
pub fn derived_priority(score: f64) -> u8 {
    match score {
        value if value >= 82.0 => 5,
        value if value >= 68.0 => 4,
        value if value >= 54.0 => 3,
        value if value >= 40.0 => 2,
        value if value >= 24.0 => 1,
        _ => 0,
    }
}

/// Calcula puntuación, señales y razón de un juego. Es una función **pura**: con
/// el mismo `input` y el mismo `now` devuelve siempre lo mismo, sin tocar
/// SQLite, la red ni el reloj.
pub fn evaluate_priority(input: &PriorityInput, now: DateTime<Utc>) -> PriorityOutcome {
    let today = now.date_naive();
    let mut signals: Vec<PrioritySignal> = Vec::new();

    // ── Juego cerrado ──────────────────────────────────────────────────────
    // Terminado: `completed_at` explícito, o progreso al 100 % aunque no haya
    // fecha. Se prefiere la fecha de finalización; si no existe se usa la última
    // partida, y si tampoco, un factor intermedio documentado.
    let completed = input.completed_at.is_some() || input.progress >= 100;
    let abandoned = input.abandoned_at.is_some();
    let settled = completed || abandoned;

    if completed {
        let (factor, detail) = match input
            .completed_at
            .or_else(|| input.last_played_at.map(|moment| moment.date_naive()))
        {
            Some(date) => {
                let days = days_between(date, today);
                let factor = half_life_decay(days, COMPLETED_HALF_LIFE_DAYS);
                (
                    factor,
                    format!(
                        "Terminado {}: la prioridad baja aunque sigas jugándolo.",
                        humanize_days(days)
                    ),
                )
            }
            None => (
                COMPLETED_WITHOUT_DATE_FACTOR,
                "Terminado al 100 % sin fecha registrada: la prioridad baja igual."
                    .to_string(),
            ),
        };
        signals.push(signal("completed", -COMPLETED_PENALTY * factor, detail));
    }

    if let Some(date) = input.abandoned_at {
        let days = days_between(date, today);
        let factor = half_life_decay(days, ABANDONED_HALF_LIFE_DAYS);
        signals.push(signal(
            "abandoned",
            -ABANDONED_PENALTY * factor,
            format!(
                "Abandonado {}: se queda al final hasta que decidas retomarlo.",
                humanize_days(days)
            ),
        ));
    }

    // ── Progreso vivo ──────────────────────────────────────────────────────
    if !settled && (PROGRESS_ALIVE_MIN..=PROGRESS_ALIVE_MAX).contains(&input.progress) {
        let progress = f64::from(input.progress);
        let span = (PROGRESS_ALIVE_PEAK - f64::from(PROGRESS_ALIVE_MIN))
            .max(f64::from(PROGRESS_ALIVE_MAX) - PROGRESS_ALIVE_PEAK);
        let shape = saturate(1.0 - (progress - PROGRESS_ALIVE_PEAK).abs() / span);
        let weight = PROGRESS_ALIVE_MIN_WEIGHT
            + (PROGRESS_ALIVE_MAX_WEIGHT - PROGRESS_ALIVE_MIN_WEIGHT) * shape;
        signals.push(signal(
            "progress_alive",
            weight,
            format!(
                "Progreso al {} %: es de lo que más merece que retomes.",
                input.progress
            ),
        ));
    }

    // ── Actividad reciente ─────────────────────────────────────────────────
    // Tres evidencias distintas del mismo hecho («esto está en marcha»), por eso
    // se suman y se recortan juntas en vez de competir como señales separadas.
    if !settled {
        let mut parts: Vec<String> = Vec::new();
        let mut activity = 0.0;

        if input.playtime_recent_minutes > 0 {
            let ratio = saturate(
                input.playtime_recent_minutes as f64 / RECENT_PLAYTIME_SATURATION_MINUTES,
            );
            activity += RECENT_PLAYTIME_WEIGHT * ratio;
            parts.push(format!(
                "{} min en las dos últimas semanas",
                input.playtime_recent_minutes
            ));
        }
        if let Some(moment) = input.last_played_at {
            let days = days_between(moment.date_naive(), today);
            let factor = half_life_decay(days, LAST_PLAYED_HALF_LIFE_DAYS);
            if factor > 0.0 {
                activity += LAST_PLAYED_WEIGHT * factor;
                parts.push(format!("última partida {}", humanize_days(days)));
            }
        }
        if input.recent_sessions > 0 {
            let ratio = saturate(f64::from(input.recent_sessions) / SESSION_SATURATION);
            activity += SESSION_WEIGHT * ratio;
            parts.push(format!(
                "{} sesiones anotadas en {SESSION_WINDOW_DAYS} días",
                input.recent_sessions
            ));
        }

        let activity = activity.min(RECENT_ACTIVITY_MAX);
        if activity > 0.0 {
            signals.push(signal(
                "recent_activity",
                activity,
                format!("Actividad reciente: {}.", parts.join(", ")),
            ));
        }
    }

    // ── Fecha objetivo ─────────────────────────────────────────────────────
    if !settled
        && let Some(target) = input.target_date
    {
        let days_left = (target - today).num_days();
        if days_left < 0 {
            let overdue = -days_left as f64;
            let weight = TARGET_UPCOMING_MAX
                + (TARGET_OVERDUE_MAX - TARGET_UPCOMING_MAX)
                    * (1.0 - half_life_decay(overdue, TARGET_OVERDUE_HALF_LIFE_DAYS));
            signals.push(signal(
                "target_date",
                weight,
                format!("Fecha objetivo vencida {}.", humanize_days(overdue)),
            ));
        } else {
            let weight =
                TARGET_UPCOMING_MAX * half_life_decay(days_left as f64, TARGET_HALF_LIFE_DAYS);
            let detail = match days_left {
                0 => "Fecha objetivo hoy mismo.".to_string(),
                1 => "Fecha objetivo mañana.".to_string(),
                _ => format!("Fecha objetivo dentro de {days_left} días."),
            };
            signals.push(signal("target_date", weight, detail));
        }
    }

    // ── Intención explícita y disponibilidad ───────────────────────────────
    if input.pinned {
        signals.push(signal(
            "pinned",
            PINNED_WEIGHT,
            "Fijado a mano en la biblioteca.",
        ));
    }
    if input.tracking {
        signals.push(signal(
            "tracking",
            TRACKING_WEIGHT,
            "En seguimiento activo.",
        ));
    }
    if input.installed {
        signals.push(signal(
            "installed",
            INSTALLED_WEIGHT,
            "Instalado y listo para jugar.",
        ));
    }

    // ── Biblioteca muerta ──────────────────────────────────────────────────
    if !settled
        && input.playtime_minutes == 0
        && input.last_played_at.is_none()
        && let Some(imported) = input.imported_at
    {
        let days = days_between(imported.date_naive(), today);
        if days >= DORMANT_MIN_DAYS {
            let weight = DORMANT_MAX * (1.0 - half_life_decay(days, DORMANT_HALF_LIFE_DAYS));
            signals.push(signal(
                "dormant_backlog",
                weight,
                format!(
                    "En la biblioteca desde {} y todavía sin estrenar.",
                    humanize_days(days)
                ),
            ));
        }
    }

    // ── Valoración y gustos ────────────────────────────────────────────────
    if let Some(rating) = input.rating
        && rating >= RATING_THRESHOLD
    {
        let span = f64::from(10 - RATING_THRESHOLD + 1);
        let weight = RATING_WEIGHT * f64::from(rating - RATING_THRESHOLD + 1) / span;
        signals.push(signal(
            "rating",
            weight,
            format!("Lo valoraste con un {rating} sobre 10."),
        ));
    }
    if input.taste_affinity > 0.0 {
        let affinity = saturate(input.taste_affinity);
        signals.push(signal(
            "taste_affinity",
            TASTE_AFFINITY_WEIGHT * affinity,
            format!(
                "Encaja al {} % con tu modelo de gustos local.",
                (affinity * 100.0).round() as i64
            ),
        ));
    }

    // ── Cierre aritmético ──────────────────────────────────────────────────
    // A partir de aquí solo se ajusta la escala, y cada ajuste se registra como
    // señal para que `BASE_SCORE + Σ pesos == score` siga siendo cierto.
    let raw = BASE_SCORE + signals.iter().map(|item| item.weight).sum::<f64>();
    let mut score = raw;

    if raw > 100.0 {
        signals.push(signal(
            "scale_clamp",
            100.0 - raw,
            "Ajuste al máximo de la escala 0-100.",
        ));
        score = 100.0;
    } else if raw < 0.0 {
        signals.push(signal(
            "scale_clamp",
            -raw,
            "Ajuste al mínimo de la escala 0-100.",
        ));
        score = 0.0;
    }

    if settled && score > SETTLED_CEILING {
        signals.push(signal(
            "settled_ceiling",
            SETTLED_CEILING - score,
            "Techo de juego cerrado: no adelanta a uno con progreso vivo.",
        ));
        score = SETTLED_CEILING;
    }

    let score = round4(score);
    let reason = compose_reason(&signals, settled);

    // Orden estable: peso absoluto descendente y, a igualdad, clave alfabética.
    signals.sort_by(|left, right| {
        right
            .weight
            .abs()
            .partial_cmp(&left.weight.abs())
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| left.signal.cmp(&right.signal))
    });

    PriorityOutcome {
        score,
        signals,
        reason,
    }
}

/// Redacta la razón corta a partir de las señales explicativas. Las señales de
/// cuadre ([`BOOKKEEPING_SIGNALS`]) no encabezan nunca: explican aritmética, no
/// motivos.
fn compose_reason(signals: &[PrioritySignal], settled: bool) -> String {
    let mut explanatory: Vec<&PrioritySignal> = signals
        .iter()
        .filter(|item| !BOOKKEEPING_SIGNALS.contains(&item.signal.as_str()))
        .collect();
    explanatory.sort_by(|left, right| {
        right
            .weight
            .abs()
            .partial_cmp(&left.weight.abs())
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| left.signal.cmp(&right.signal))
    });

    let Some(main) = explanatory.first() else {
        return if settled {
            "Juego cerrado sin más señales: se queda al final.".to_string()
        } else {
            "Sin señales todavía: se queda en la mitad de la lista.".to_string()
        };
    };

    let mut reason = main.detail.clone();
    if let Some(secondary) = explanatory
        .get(1)
        .filter(|item| item.weight.abs() >= REASON_SECONDARY_THRESHOLD)
    {
        reason.push(' ');
        reason.push_str(&secondary.detail);
    }
    reason
}

// ---------------------------------------------------------------------------
// Persistencia del motor de prioridad
// ---------------------------------------------------------------------------

/// Fila cruda de la biblioteca, tal y como sale de SQLite antes de convertirse
/// en [`PriorityInput`].
struct PriorityRow {
    input: PriorityInput,
    stored_score: f64,
    facets: Vec<(Facet, String)>,
}

fn timestamp(now: DateTime<Utc>) -> String {
    // Mismo formato que `strftime('%Y-%m-%dT%H:%M:%fZ', 'now')`, el `DEFAULT` de
    // las tablas de la migración 024.
    now.to_rfc3339_opts(SecondsFormat::Millis, true)
}

/// Acepta tanto una fecha ISO (`2026-08-18`) como una marca RFC 3339 completa.
/// La migración 012 valida esas columnas con `date(...)`, que admite ambas.
fn parse_flexible_date(value: Option<String>) -> Option<NaiveDate> {
    let value = value?;
    let value = value.trim();
    if let Ok(date) = NaiveDate::parse_from_str(value, "%Y-%m-%d") {
        return Some(date);
    }
    DateTime::parse_from_rfc3339(value)
        .ok()
        .map(|moment| moment.with_timezone(&Utc).date_naive())
}

fn parse_moment(value: Option<String>) -> Option<DateTime<Utc>> {
    let value = value?;
    DateTime::parse_from_rfc3339(value.trim())
        .ok()
        .map(|moment| moment.with_timezone(&Utc))
}

fn parse_facet_list(raw: &str) -> Vec<String> {
    // Un `genres_json` corrupto no puede tumbar el recálculo de toda la
    // biblioteca: se trata como «sin facetas» y el resto de señales sigue en pie.
    serde_json::from_str::<Vec<String>>(raw).unwrap_or_default()
}

fn get_u64(row: &Row<'_>, index: usize) -> rusqlite::Result<u64> {
    let value: i64 = row.get(index)?;
    Ok(value.max(0) as u64)
}

/// Recalcula la puntuación de toda la biblioteca en una única transacción.
///
/// Escribe `game_personal.priority_score`, `priority_reason` y
/// `priority_computed_at`, y reemplaza por completo `priority_signals`. No toca
/// `game_personal.updated_at`, `priority` ni `priority_locked`: recalcular no es
/// una edición de la persona usuaria.
pub fn recompute_priorities(
    connection: &mut Connection,
    now: DateTime<Utc>,
) -> AppResult<PriorityRecomputeReport> {
    let computed_at = timestamp(now);
    let weights = load_taste_weights(connection)?;
    let sessions = load_recent_sessions(connection, now)?;
    let rows = load_priority_rows(connection, &sessions, &weights)?;

    let transaction = connection.transaction()?;
    // Recálculo completo: la tabla de señales se reconstruye entera para que no
    // queden señales huérfanas de una versión anterior del motor.
    transaction.execute("DELETE FROM priority_signals", [])?;

    let mut report = PriorityRecomputeReport {
        evaluated: 0,
        updated: 0,
        locked: 0,
        settled: 0,
        signals_written: 0,
        highlights: Vec::new(),
        computed_at: computed_at.clone(),
    };
    let mut ranking: Vec<PriorityRanking> = Vec::with_capacity(rows.len());

    {
        let mut update = transaction.prepare_cached(
            "UPDATE game_personal
                SET priority_score = ?2,
                    priority_reason = ?3,
                    priority_computed_at = ?4
              WHERE app_id = ?1",
        )?;
        let mut insert_signal = transaction.prepare_cached(
            "INSERT INTO priority_signals(app_id, signal, weight, detail, computed_at)
             VALUES (?1, ?2, ?3, ?4, ?5)",
        )?;

        for row in &rows {
            let outcome = evaluate_priority(&row.input, now);
            report.evaluated += 1;
            if row.input.locked {
                report.locked += 1;
            }
            if outcome
                .signals
                .iter()
                .any(|item| item.signal == "completed" || item.signal == "abandoned")
            {
                report.settled += 1;
            }
            // Comparar `f64` con `!=` es correcto aquí: ambos lados vienen de
            // `round4`, así que o son el mismo número o cambió de verdad.
            if (row.stored_score - outcome.score).abs() > f64::EPSILON {
                report.updated += 1;
            }
            update.execute(params![
                row.input.app_id,
                outcome.score,
                outcome.reason,
                computed_at,
            ])?;
            for item in &outcome.signals {
                insert_signal.execute(params![
                    row.input.app_id,
                    item.signal,
                    item.weight,
                    item.detail,
                    computed_at,
                ])?;
                report.signals_written += 1;
            }
            ranking.push(PriorityRanking {
                app_id: row.input.app_id,
                title: row.input.title.clone(),
                score: outcome.score,
                effective_score: effective_score(
                    row.input.locked,
                    row.input.manual_priority,
                    outcome.score,
                ),
                derived_priority: derived_priority(outcome.score),
                manual_priority: row.input.manual_priority,
                locked: row.input.locked,
                reason: outcome.reason,
            });
        }
    }

    transaction.commit()?;

    sort_ranking(&mut ranking);
    ranking.truncate(RECOMPUTE_HIGHLIGHTS);
    report.highlights = ranking;
    Ok(report)
}

/// Orden del ranking: puntuación efectiva descendente y, a igualdad, AppID
/// ascendente. El desempate por AppID es lo que hace el orden reproducible.
fn sort_ranking(ranking: &mut [PriorityRanking]) {
    ranking.sort_by(|left, right| {
        right
            .effective_score
            .partial_cmp(&left.effective_score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| left.app_id.cmp(&right.app_id))
    });
}

fn load_recent_sessions(
    connection: &Connection,
    now: DateTime<Utc>,
) -> AppResult<BTreeMap<u32, u32>> {
    let since = timestamp(now - chrono::Duration::days(SESSION_WINDOW_DAYS));
    let mut statement = connection.prepare(
        "SELECT app_id, COUNT(*) FROM game_sessions
          WHERE ended_at IS NOT NULL
            AND datetime(ended_at) >= datetime(?1)
          GROUP BY app_id",
    )?;
    let rows = statement
        .query_map([since], |row| Ok((row.get::<_, u32>(0)?, row.get::<_, u32>(1)?)))?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows.into_iter().collect())
}

fn load_priority_rows(
    connection: &Connection,
    sessions: &BTreeMap<u32, u32>,
    weights: &BTreeMap<FacetKey, f64>,
) -> AppResult<Vec<PriorityRow>> {
    let tags = load_game_tags(connection)?;
    let mut statement = connection.prepare(
        "SELECT g.app_id, g.title, g.playtime_minutes, g.playtime_recent_minutes,
                g.last_played_at, g.imported_at, g.genres_json, g.categories_json,
                g.developer, g.publisher,
                p.progress, p.priority, p.priority_locked, p.priority_score,
                p.installed, p.pinned, p.tracking, p.rating,
                p.completed_at, p.abandoned_at, p.target_date
           FROM games g
           JOIN game_personal p ON p.app_id = g.app_id
          ORDER BY g.app_id ASC",
    )?;
    let rows = statement
        .query_map([], |row| {
            let app_id: u32 = row.get(0)?;
            let genres = parse_facet_list(&row.get::<_, String>(6)?);
            let categories = parse_facet_list(&row.get::<_, String>(7)?);
            let developer: Option<String> = row.get(8)?;
            let publisher: Option<String> = row.get(9)?;
            let mut facets: Vec<(Facet, String)> = Vec::new();
            for value in genres {
                facets.push((Facet::Genre, value));
            }
            for value in categories {
                facets.push((Facet::Category, value));
            }
            if let Some(value) = developer.clone() {
                facets.push((Facet::Developer, value));
            }
            if let Some(value) = publisher.clone() {
                facets.push((Facet::Publisher, value));
            }
            if let Some(values) = tags.get(&app_id) {
                for value in values {
                    facets.push((Facet::Tag, value.clone()));
                }
            }
            facets.retain(|(_, value)| !value.trim().is_empty());
            Ok(PriorityRow {
                input: PriorityInput {
                    app_id,
                    title: row.get(1)?,
                    progress: row.get::<_, i64>(10)?.clamp(0, 100) as u8,
                    manual_priority: row.get::<_, i64>(11)?.clamp(0, 5) as u8,
                    locked: row.get::<_, i64>(12)? != 0,
                    installed: row.get::<_, i64>(14)? != 0,
                    pinned: row.get::<_, i64>(15)? != 0,
                    tracking: row.get::<_, i64>(16)? != 0,
                    rating: row.get::<_, Option<i64>>(17)?.map(|value| value.clamp(1, 10) as u8),
                    completed_at: parse_flexible_date(row.get(18)?),
                    abandoned_at: parse_flexible_date(row.get(19)?),
                    target_date: parse_flexible_date(row.get(20)?),
                    playtime_minutes: get_u64(row, 2)?,
                    playtime_recent_minutes: get_u64(row, 3)?,
                    last_played_at: parse_moment(row.get(4)?),
                    imported_at: parse_moment(row.get(5)?),
                    recent_sessions: sessions.get(&app_id).copied().unwrap_or(0),
                    taste_affinity: 0.0,
                },
                stored_score: row.get(13)?,
                facets,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;

    if rows.len() > MAX_RECOMPUTE_GAMES {
        return Err(AppError::validation(
            "La biblioteca supera el límite seguro de recálculo de prioridades.",
        ));
    }

    Ok(rows
        .into_iter()
        .map(|mut row| {
            row.input.taste_affinity = affinity_for(&row.facets, weights).score;
            row
        })
        .collect())
}

fn load_game_tags(connection: &Connection) -> AppResult<BTreeMap<u32, Vec<String>>> {
    let mut statement = connection.prepare(
        "SELECT gt.app_id, t.name
           FROM game_tags gt
           JOIN tags t ON t.id = gt.tag_id
          ORDER BY gt.app_id ASC, t.name COLLATE NOCASE ASC, t.id ASC",
    )?;
    let rows = statement
        .query_map([], |row| {
            Ok((row.get::<_, u32>(0)?, row.get::<_, String>(1)?))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    let mut grouped: BTreeMap<u32, Vec<String>> = BTreeMap::new();
    for (app_id, name) in rows {
        grouped.entry(app_id).or_default().push(name);
    }
    Ok(grouped)
}

/// Ancla o suelta la prioridad manual de un juego.
///
/// Anclar no borra la puntuación derivada: se sigue calculando y guardando para
/// poder enseñar las dos a la vez.
pub fn set_priority_lock(connection: &Connection, app_id: u32, locked: bool) -> AppResult<()> {
    if app_id == 0 {
        return Err(AppError::validation("El AppID indicado no es válido."));
    }
    let changed = connection.execute(
        "UPDATE game_personal SET priority_locked = ?2 WHERE app_id = ?1",
        params![app_id, i64::from(locked)],
    )?;
    if changed == 0 {
        return Err(AppError::not_found("El juego ya no está en la biblioteca."));
    }
    Ok(())
}

/// Devuelve la explicación guardada de la prioridad de un juego, con las señales
/// ordenadas por peso absoluto descendente.
pub fn explain_priority(connection: &Connection, app_id: u32) -> AppResult<PriorityExplanation> {
    if app_id == 0 {
        return Err(AppError::validation("El AppID indicado no es válido."));
    }
    let (title, score, manual_priority, locked, reason, computed_at) = connection
        .query_row(
            "SELECT g.title, p.priority_score, p.priority, p.priority_locked,
                    p.priority_reason, p.priority_computed_at
               FROM games g
               JOIN game_personal p ON p.app_id = g.app_id
              WHERE g.app_id = ?1",
            [app_id],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, f64>(1)?,
                    row.get::<_, i64>(2)?.clamp(0, 5) as u8,
                    row.get::<_, i64>(3)? != 0,
                    row.get::<_, Option<String>>(4)?,
                    row.get::<_, Option<String>>(5)?,
                ))
            },
        )
        .optional()?
        .ok_or_else(|| AppError::not_found("El juego ya no está en la biblioteca."))?;

    let mut statement = connection.prepare(
        "SELECT signal, weight, detail FROM priority_signals
          WHERE app_id = ?1
          ORDER BY abs(weight) DESC, signal ASC",
    )?;
    let signals = statement
        .query_map([app_id], |row| {
            Ok(PrioritySignal {
                signal: row.get(0)?,
                weight: row.get(1)?,
                detail: row.get(2)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;

    let derived = derived_priority(score);
    // El conflicto solo existe si la persona usuaria ancló su prioridad y las
    // señales dicen otra cosa. Si coinciden, no hay nada que contar.
    let manual_override = (locked && derived != manual_priority).then(|| {
        format!(
            "Tu prioridad manual dice {manual_priority}; las señales dicen {derived}. Manda la tuya."
        )
    });

    Ok(PriorityExplanation {
        app_id,
        title,
        score: round4(score),
        effective_score: effective_score(locked, manual_priority, score),
        derived_priority: derived,
        manual_priority,
        locked,
        reason: reason.unwrap_or_else(|| {
            "Todavía no se ha calculado la prioridad de este juego.".to_string()
        }),
        computed_at,
        manual_override,
        signals,
    })
}

/// Ranking guardado, ya ordenado por la puntuación que manda en cada fila.
pub fn list_priority_ranking(
    connection: &Connection,
    limit: u32,
) -> AppResult<Vec<PriorityRanking>> {
    let limit = limit.clamp(1, MAX_LIST_LIMIT);
    let mut statement = connection.prepare(
        "SELECT g.app_id, g.title, p.priority_score, p.priority, p.priority_locked,
                p.priority_reason
           FROM games g
           JOIN game_personal p ON p.app_id = g.app_id
          ORDER BY CASE WHEN p.priority_locked = 1
                        THEN MIN(p.priority, 5) * 20.0
                        ELSE p.priority_score
                   END DESC,
                   g.app_id ASC
          LIMIT ?1",
    )?;
    let items = statement
        .query_map([limit], |row| {
            let score: f64 = row.get(2)?;
            let manual_priority = row.get::<_, i64>(3)?.clamp(0, 5) as u8;
            let locked = row.get::<_, i64>(4)? != 0;
            Ok(PriorityRanking {
                app_id: row.get(0)?,
                title: row.get(1)?,
                score: round4(score),
                effective_score: effective_score(locked, manual_priority, score),
                derived_priority: derived_priority(score),
                manual_priority,
                locked,
                reason: row.get::<_, Option<String>>(5)?.unwrap_or_default(),
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(items)
}

// ---------------------------------------------------------------------------
// Modelo de gustos con aprendizaje local
// ---------------------------------------------------------------------------

/// Una faceta aprendida, con su peso normalizado y el tamaño de la muestra que
/// lo sostiene.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TasteFacet {
    /// Clave persistida (`genre`, `category`, `developer`, `publisher`, `tag`).
    pub facet: String,
    /// Etiqueta en español para la interfaz.
    pub facet_label: String,
    pub value: String,
    /// Peso normalizado en `-1..1`. Positivo gusta, negativo no.
    pub weight: f64,
    pub positive_samples: u32,
    pub negative_samples: u32,
}

/// Resumen de un aprendizaje del modelo de gustos.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TasteReport {
    /// Juegos de la biblioteca que aportaron evidencia.
    pub games_analyzed: u32,
    /// Próximos lanzamientos descartados usados como evidencia negativa.
    pub dismissed_upcoming_used: u32,
    /// Facetas con peso suficiente para guardarse.
    pub facets_learned: u32,
    pub positive_facets: u32,
    pub negative_facets: u32,
    /// Facetas más marcadas, ordenadas por peso absoluto.
    pub highlights: Vec<TasteFacet>,
    pub computed_at: String,
}

impl Facet {
    /// Etiqueta en español, para textos de interfaz.
    pub const fn label(self) -> &'static str {
        match self {
            Self::Genre => "género",
            Self::Category => "categoría",
            Self::Developer => "estudio",
            Self::Publisher => "distribuidora",
            Self::Tag => "etiqueta",
        }
    }
}

/// Evidencia acumulada de una faceta antes de normalizar.
#[derive(Debug, Default)]
struct FacetEvidence {
    /// Representante que se guarda y se enseña: el menor lexicográficamente de
    /// las variantes vistas. Elegirlo así mantiene el resultado reproducible aun
    /// cuando la biblioteca mezcla `Action` y `action`.
    display: String,
    sum: f64,
    count: u32,
    positive: u32,
    negative: u32,
    minutes: u64,
}

impl FacetEvidence {
    fn observe(&mut self, display: &str, sample: f64, minutes: u64) {
        let display = display.trim();
        if self.display.is_empty() || display < self.display.as_str() {
            self.display = display.to_string();
        }
        self.sum += sample;
        self.count += 1;
        if sample > 0.0 {
            self.positive += 1;
        } else if sample < 0.0 {
            self.negative += 1;
        }
        self.minutes = self.minutes.saturating_add(minutes);
    }

    /// Media bayesiana (media suavizada por encogimiento hacia el previo):
    ///
    /// ```text
    ///           Σ muestras + FUERZA_PREVIO · MEDIA_PREVIA
    /// peso  =  ───────────────────────────────────────────
    ///                  n muestras + FUERZA_PREVIO
    /// ```
    ///
    /// Con `MEDIA_PREVIA = 0` se reduce a `Σ / (n + 5)`. Es exactamente lo que
    /// hace falta para que un género con 3 juegos entusiastas (3/8 = 0,375) no
    /// pese lo mismo que uno con 60 (60/65 = 0,923): la confianza crece con la
    /// muestra en lugar de darse por supuesta.
    fn weight(&self) -> f64 {
        let numerator = self.sum + TASTE_PRIOR_STRENGTH * TASTE_PRIOR_MEAN;
        let denominator = f64::from(self.count) + TASTE_PRIOR_STRENGTH;
        round4((numerator / denominator).clamp(-1.0, 1.0))
    }
}

/// Puntuación de una faceta concreta al comparar un candidato con el modelo.
#[derive(Debug, Clone, PartialEq)]
struct MatchedFacet {
    facet: Facet,
    display: String,
    weight: f64,
    contribution: f64,
}

/// Resultado de comparar un conjunto de facetas contra el modelo de gustos.
#[derive(Debug, Clone, PartialEq)]
struct FacetMatch {
    /// Afinidad 0-1.
    score: f64,
    positives: Vec<MatchedFacet>,
    negatives: Vec<MatchedFacet>,
}

/// Compara las facetas de un juego con el modelo aprendido.
///
/// Es una función pura sobre el mapa de pesos: no consulta SQLite. La usan tanto
/// el motor de prioridad (para la señal `taste_affinity`) como el puntuador de
/// próximos lanzamientos, de modo que ambos hablan del mismo modelo.
fn affinity_for(facets: &[(Facet, String)], weights: &BTreeMap<FacetKey, f64>) -> FacetMatch {
    let mut seen: BTreeSet<FacetKey> = BTreeSet::new();
    let mut positives: Vec<MatchedFacet> = Vec::new();
    let mut negatives: Vec<MatchedFacet> = Vec::new();

    for (facet, value) in facets {
        let key = (*facet, normalize_facet_value(value));
        if key.1.is_empty() || !seen.insert(key.clone()) {
            continue;
        }
        let Some(weight) = weights.get(&key).copied() else {
            continue;
        };
        let matched = MatchedFacet {
            facet: *facet,
            display: value.trim().to_string(),
            weight,
            contribution: weight * facet.importance(),
        };
        if weight > 0.0 {
            positives.push(matched);
        } else if weight < 0.0 {
            negatives.push(matched);
        }
    }

    let order = |left: &MatchedFacet, right: &MatchedFacet| {
        right
            .contribution
            .abs()
            .partial_cmp(&left.contribution.abs())
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| left.facet.cmp(&right.facet))
            .then_with(|| left.display.cmp(&right.display))
    };
    positives.sort_by(order);
    negatives.sort_by(order);

    let raw = positives.iter().map(|item| item.contribution).sum::<f64>()
        - negatives
            .iter()
            .map(|item| item.contribution.abs())
            .sum::<f64>();
    FacetMatch {
        score: round4(saturate(raw / UPCOMING_MATCH_SATURATION)),
        positives,
        negatives,
    }
}

fn load_taste_weights(connection: &Connection) -> AppResult<BTreeMap<FacetKey, f64>> {
    let mut statement =
        connection.prepare("SELECT facet, value, weight FROM taste_weights ORDER BY facet, value")?;
    let rows = statement
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, f64>(2)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    let mut weights = BTreeMap::new();
    for (facet, value, weight) in rows {
        if let Some(facet) = Facet::from_key(&facet) {
            weights.insert((facet, normalize_facet_value(&value)), weight);
        }
    }
    Ok(weights)
}

/// Último veredicto explícito por juego. «Último» se decide por fecha y, a
/// igualdad, por identificador: dos filas con la misma marca temporal no pueden
/// dar resultados distintos entre ejecuciones.
fn load_latest_feedback(connection: &Connection) -> AppResult<BTreeMap<u32, String>> {
    let mut statement = connection.prepare(
        "SELECT app_id, verdict FROM taste_feedback
          WHERE app_id IS NOT NULL
          ORDER BY datetime(created_at) ASC, id ASC",
    )?;
    let rows = statement
        .query_map([], |row| {
            Ok((row.get::<_, u32>(0)?, row.get::<_, String>(1)?))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    let mut latest = BTreeMap::new();
    for (app_id, verdict) in rows {
        latest.insert(app_id, verdict);
    }
    Ok(latest)
}

/// Evidencia que aporta un juego propio, en `-1..1`.
fn taste_sample(input: &PriorityInput, verdict: Option<&str>, today: NaiveDate) -> f64 {
    let mut sample =
        TASTE_PLAYTIME_PULL * saturate(input.playtime_minutes as f64 / TASTE_PLAYTIME_SATURATION_MINUTES);

    if input.completed_at.is_some() || input.progress >= 100 {
        sample += TASTE_COMPLETED_PULL;
    }
    if input.pinned {
        sample += TASTE_PINNED_PULL;
    }
    if let Some(rating) = input.rating {
        if rating >= TASTE_HIGH_RATING_THRESHOLD {
            let span = f64::from(10 - TASTE_HIGH_RATING_THRESHOLD + 1);
            sample += TASTE_HIGH_RATING_PULL * f64::from(rating - TASTE_HIGH_RATING_THRESHOLD + 1)
                / span;
        } else if rating <= TASTE_LOW_RATING_THRESHOLD {
            let span = f64::from(TASTE_LOW_RATING_THRESHOLD);
            sample -= TASTE_LOW_RATING_PUSH * f64::from(TASTE_LOW_RATING_THRESHOLD + 1 - rating)
                / span;
        }
    }
    if input.abandoned_at.is_some() {
        sample -= TASTE_ABANDONED_PUSH;
    }
    // Instalado, sin estrenar y con meses encima: se eligió, se descargó y aun
    // así no apetece. Es la evidencia negativa más silenciosa que existe.
    if input.installed
        && input.playtime_minutes == 0
        && input.last_played_at.is_none()
        && let Some(imported) = input.imported_at
        && days_between(imported.date_naive(), today) >= TASTE_SHELVED_MIN_DAYS
    {
        sample -= TASTE_SHELVED_PUSH;
    }
    match verdict {
        Some("not_interested") => sample -= TASTE_NOT_INTERESTED_PUSH,
        Some("interested") => sample += TASTE_INTERESTED_PULL,
        // `owned_already` habla del inventario, no del gusto: no mueve el modelo.
        _ => {}
    }
    sample.clamp(-1.0, 1.0)
}

/// Recorre la biblioteca y reconstruye `taste_weights` en una transacción.
///
/// # Privacidad
///
/// Todo ocurre dentro del equipo: entra lo que ya está en SQLite y sale a la
/// misma base. Ninguna faceta, peso o recuento viaja a ningún servidor.
pub fn learn_taste(connection: &mut Connection, now: DateTime<Utc>) -> AppResult<TasteReport> {
    let computed_at = timestamp(now);
    let today = now.date_naive();
    let feedback = load_latest_feedback(connection)?;
    let rows = load_priority_rows(connection, &BTreeMap::new(), &BTreeMap::new())?;
    let dismissed = load_dismissed_upcoming_facets(connection)?;

    let mut evidence: BTreeMap<FacetKey, FacetEvidence> = BTreeMap::new();
    for row in &rows {
        let sample = taste_sample(
            &row.input,
            feedback.get(&row.input.app_id).map(String::as_str),
            today,
        );
        for (facet, value) in &row.facets {
            let key = (*facet, normalize_facet_value(value));
            if key.1.is_empty() {
                continue;
            }
            evidence
                .entry(key)
                .or_default()
                .observe(value, sample, row.input.playtime_minutes);
        }
    }
    for facets in &dismissed {
        for (facet, value) in facets {
            let key = (*facet, normalize_facet_value(value));
            if key.1.is_empty() {
                continue;
            }
            evidence
                .entry(key)
                .or_default()
                .observe(value, -TASTE_DISMISSED_UPCOMING_PUSH, 0);
        }
    }

    let mut learned: Vec<TasteFacet> = Vec::new();
    for ((facet, _), entry) in &evidence {
        let weight = entry.weight();
        if weight.abs() < TASTE_MIN_STORED_WEIGHT {
            continue;
        }
        learned.push(TasteFacet {
            facet: facet.key().to_string(),
            facet_label: facet.label().to_string(),
            value: entry.display.clone(),
            weight,
            positive_samples: entry.positive,
            negative_samples: entry.negative,
        });
    }

    let transaction = connection.transaction()?;
    // Aprendizaje completo: la tabla se reconstruye entera. Conservar filas de
    // una biblioteca anterior daría pesos que ya nada sostiene.
    transaction.execute("DELETE FROM taste_weights", [])?;
    {
        let mut insert = transaction.prepare_cached(
            "INSERT INTO taste_weights(
                facet, value, weight, positive_samples, negative_samples, updated_at
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        )?;
        for item in &learned {
            insert.execute(params![
                item.facet,
                item.value,
                item.weight,
                item.positive_samples,
                item.negative_samples,
                computed_at,
            ])?;
        }
    }
    transaction.commit()?;

    let positive_facets = learned.iter().filter(|item| item.weight > 0.0).count() as u32;
    let negative_facets = learned.iter().filter(|item| item.weight < 0.0).count() as u32;
    let mut highlights = learned.clone();
    highlights.sort_by(|left, right| {
        right
            .weight
            .abs()
            .partial_cmp(&left.weight.abs())
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| left.facet.cmp(&right.facet))
            .then_with(|| left.value.cmp(&right.value))
    });
    highlights.truncate(TASTE_HIGHLIGHTS);

    Ok(TasteReport {
        games_analyzed: rows.len() as u32,
        dismissed_upcoming_used: dismissed.len() as u32,
        facets_learned: learned.len() as u32,
        positive_facets,
        negative_facets,
        highlights,
        computed_at,
    })
}

/// Minutos jugados acumulados por faceta. Se recalcula al puntuar porque
/// `taste_weights` guarda el peso normalizado, no las horas que lo sostienen, y
/// la razón de coincidencia necesita nombrarlas («tus 62 h en metroidvania»).
fn facet_minutes(rows: &[PriorityRow]) -> BTreeMap<FacetKey, u64> {
    let mut minutes: BTreeMap<FacetKey, u64> = BTreeMap::new();
    for row in rows {
        let mut seen: BTreeSet<FacetKey> = BTreeSet::new();
        for (facet, value) in &row.facets {
            let key = (*facet, normalize_facet_value(value));
            if key.1.is_empty() || !seen.insert(key.clone()) {
                continue;
            }
            let entry = minutes.entry(key).or_insert(0);
            *entry = entry.saturating_add(row.input.playtime_minutes);
        }
    }
    minutes
}

/// Registra una opinión explícita sobre un juego o un próximo lanzamiento.
///
/// # Por qué a veces se guarda sin AppID
///
/// La migración 024 ata `taste_feedback.app_id` a `games(app_id)`. Un próximo
/// lanzamiento todavía no está en `games`, así que con
/// `PRAGMA foreign_keys = ON` esa fila no se puede guardar con su AppID. Cuando
/// el juego no está en la biblioteca la opinión se guarda sin AppID **y** se
/// refleja en `upcoming_releases.dismissed_at`, que es donde el modelo la lee
/// después: no se pierde ninguna información, solo cambia dónde vive. Si en el
/// futuro una migración suelta esa clave foránea, esta función empezará a
/// guardar el AppID sin más cambios.
pub fn record_taste_feedback(
    connection: &Connection,
    app_id: u32,
    verdict: &str,
    surface: &str,
) -> AppResult<()> {
    if app_id == 0 {
        return Err(AppError::validation("El AppID indicado no es válido."));
    }
    let verdict = verdict.trim();
    if !TASTE_VERDICTS.contains(&verdict) {
        return Err(AppError::validation(
            "Ese veredicto no es uno de los admitidos.",
        ));
    }
    let surface = surface.trim();
    if !TASTE_SURFACES.contains(&surface) {
        return Err(AppError::validation(
            "Esa superficie de opinión no es una de las admitidas.",
        ));
    }

    let in_library: bool = connection.query_row(
        "SELECT EXISTS(SELECT 1 FROM games WHERE app_id = ?1)",
        [app_id],
        |row| row.get::<_, i64>(0).map(|value| value != 0),
    )?;
    let in_upcoming: bool = connection.query_row(
        "SELECT EXISTS(SELECT 1 FROM upcoming_releases WHERE app_id = ?1)",
        [app_id],
        |row| row.get::<_, i64>(0).map(|value| value != 0),
    )?;
    if !in_library && !in_upcoming {
        return Err(AppError::not_found(
            "No conocemos ese juego ni en la biblioteca ni entre los próximos lanzamientos.",
        ));
    }

    connection.execute(
        "INSERT INTO taste_feedback(id, app_id, verdict, surface) VALUES (?1, ?2, ?3, ?4)",
        params![
            Uuid::new_v4().to_string(),
            in_library.then_some(app_id),
            verdict,
            surface,
        ],
    )?;

    if in_upcoming {
        // `interested` devuelve el candidato a la lista; los otros dos lo
        // retiran: uno porque no gusta y otro porque ya se tiene.
        match verdict {
            "interested" => {
                connection.execute(
                    "UPDATE upcoming_releases
                        SET dismissed_at = NULL,
                            updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
                      WHERE app_id = ?1",
                    [app_id],
                )?;
            }
            _ => {
                connection.execute(
                    "UPDATE upcoming_releases
                        SET dismissed_at = COALESCE(
                                dismissed_at,
                                strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
                            ),
                            updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
                      WHERE app_id = ?1",
                    [app_id],
                )?;
            }
        }
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Próximos lanzamientos
// ---------------------------------------------------------------------------

/// Candidato tal y como lo entrega quien lo descargó. Este módulo no habla con
/// la red: solo valida y persiste.
#[derive(Debug, Clone, PartialEq)]
pub struct ImportedUpcomingRelease {
    pub app_id: u32,
    pub title: String,
    pub capsule_url: Option<String>,
    pub header_url: Option<String>,
    /// Fecha ISO (`2026-11-04`) si `release_date_is_exact`; si no, una etiqueta
    /// corta tal cual la publica la tienda («Q4 2026», «Próximamente»).
    pub release_date: Option<String>,
    pub release_date_is_exact: bool,
    pub genres: Vec<String>,
    pub categories: Vec<String>,
    pub developer: Option<String>,
    pub publisher: Option<String>,
    pub short_description: Option<String>,
    pub source: String,
}

/// Un próximo lanzamiento ya puntuado contra el modelo local.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpcomingRelease {
    pub app_id: u32,
    pub title: String,
    pub capsule_url: Option<String>,
    pub header_url: Option<String>,
    pub release_date: Option<String>,
    pub release_date_is_exact: bool,
    pub genres: Vec<String>,
    pub categories: Vec<String>,
    pub developer: Option<String>,
    pub publisher: Option<String>,
    pub short_description: Option<String>,
    /// Coincidencia con el modelo de gustos, 0-1.
    pub match_score: f64,
    /// Frase en español que nombra las facetas concretas que motivan la
    /// coincidencia. Nunca menciona una faceta que el candidato no tenga.
    pub match_reason: String,
    pub source: String,
    pub dismissed_at: Option<String>,
    pub discovered_at: String,
    pub updated_at: String,
}

/// Resultado de importar candidatos.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpcomingImportSummary {
    pub received: u32,
    pub inserted: u32,
    pub updated: u32,
}

fn validate_art_url(value: Option<&str>) -> AppResult<Option<String>> {
    let Some(value) = value.map(str::trim).filter(|value| !value.is_empty()) else {
        return Ok(None);
    };
    if value.len() > MAX_URL_LENGTH {
        return Err(AppError::validation(
            "La dirección del arte del lanzamiento es demasiado larga.",
        ));
    }
    let parsed = Url::parse(value)
        .map_err(|_| AppError::validation("La dirección del arte del lanzamiento no es válida."))?;
    let allowed = parsed.scheme() == "https"
        && parsed
            .host_str()
            .is_some_and(|host| ALLOWED_ART_HOSTS.contains(&host));
    if !allowed {
        return Err(AppError::validation(
            "El arte de un lanzamiento solo puede venir de los servidores oficiales de Steam.",
        ));
    }
    Ok(Some(value.to_string()))
}

fn validate_facet_values(values: &[String], label: &str) -> AppResult<Vec<String>> {
    if values.len() > MAX_UPCOMING_FACETS {
        return Err(AppError::validation(format!(
            "El lanzamiento declara demasiadas entradas de {label}."
        )));
    }
    let mut seen: BTreeSet<String> = BTreeSet::new();
    let mut cleaned = Vec::with_capacity(values.len());
    for value in values {
        let value = value.trim();
        if value.is_empty() {
            continue;
        }
        if value.chars().count() > MAX_FACET_VALUE {
            return Err(AppError::validation(format!(
                "Una entrada de {label} del lanzamiento es demasiado larga."
            )));
        }
        if !seen.insert(normalize_facet_value(value)) {
            continue;
        }
        cleaned.push(value.to_string());
    }
    Ok(cleaned)
}

fn validate_optional_text(
    value: Option<&str>,
    limit: usize,
    message: &str,
) -> AppResult<Option<String>> {
    let Some(value) = value.map(str::trim).filter(|value| !value.is_empty()) else {
        return Ok(None);
    };
    if value.chars().count() > limit {
        return Err(AppError::validation(message.to_string()));
    }
    Ok(Some(value.to_string()))
}

fn validate_release_date(value: Option<&str>, is_exact: bool) -> AppResult<Option<String>> {
    let Some(value) = value.map(str::trim).filter(|value| !value.is_empty()) else {
        return Ok(None);
    };
    if is_exact {
        // Una fecha marcada como exacta tiene que serlo de verdad: si no, la
        // interfaz prometería una precisión que la tienda no dio.
        NaiveDate::parse_from_str(value, "%Y-%m-%d").map_err(|_| {
            AppError::validation("La fecha exacta del lanzamiento no es una fecha ISO válida.")
        })?;
        return Ok(Some(value.to_string()));
    }
    if value.chars().count() > 40 || value.chars().any(char::is_control) {
        return Err(AppError::validation(
            "La etiqueta de lanzamiento aproximada no es válida.",
        ));
    }
    Ok(Some(value.to_string()))
}

/// Inserta o actualiza candidatos a próximo lanzamiento.
///
/// Conserva `match_score`, `match_reason` y `dismissed_at`: puntuar es trabajo de
/// [`score_upcoming`] y descartar es una decisión de la persona usuaria; una
/// reimportación no debe pisar ninguna de las dos.
pub fn upsert_upcoming(
    connection: &mut Connection,
    items: &[ImportedUpcomingRelease],
) -> AppResult<UpcomingImportSummary> {
    if items.len() > MAX_UPCOMING_BATCH {
        return Err(AppError::validation(
            "El lote de próximos lanzamientos supera el límite seguro de importación.",
        ));
    }
    let mut seen: BTreeSet<u32> = BTreeSet::new();
    let mut summary = UpcomingImportSummary {
        received: items.len() as u32,
        inserted: 0,
        updated: 0,
    };

    let transaction = connection.transaction()?;
    {
        let mut exists = transaction
            .prepare_cached("SELECT EXISTS(SELECT 1 FROM upcoming_releases WHERE app_id = ?1)")?;
        let mut upsert = transaction.prepare_cached(
            "INSERT INTO upcoming_releases(
                app_id, title, capsule_url, header_url, release_date, release_date_is_exact,
                genres_json, categories_json, developer, publisher, short_description, source
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)
             ON CONFLICT(app_id) DO UPDATE SET
                title = excluded.title,
                capsule_url = COALESCE(excluded.capsule_url, upcoming_releases.capsule_url),
                header_url = COALESCE(excluded.header_url, upcoming_releases.header_url),
                release_date = excluded.release_date,
                release_date_is_exact = excluded.release_date_is_exact,
                genres_json = excluded.genres_json,
                categories_json = excluded.categories_json,
                developer = COALESCE(excluded.developer, upcoming_releases.developer),
                publisher = COALESCE(excluded.publisher, upcoming_releases.publisher),
                short_description = COALESCE(
                    excluded.short_description, upcoming_releases.short_description
                ),
                source = excluded.source,
                updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')",
        )?;

        for item in items {
            if item.app_id == 0 {
                return Err(AppError::validation(
                    "Un próximo lanzamiento llegó sin AppID válido.",
                ));
            }
            if !seen.insert(item.app_id) {
                return Err(AppError::validation(
                    "El lote de próximos lanzamientos repite un AppID.",
                ));
            }
            let title = item.title.trim();
            if title.is_empty() || title.chars().count() > MAX_UPCOMING_TITLE {
                return Err(AppError::validation(
                    "El título de un próximo lanzamiento no es válido.",
                ));
            }
            if !UPCOMING_SOURCES.contains(&item.source.trim()) {
                return Err(AppError::validation(
                    "La procedencia de un próximo lanzamiento no es una de las admitidas.",
                ));
            }
            let genres = validate_facet_values(&item.genres, "géneros")?;
            let categories = validate_facet_values(&item.categories, "categorías")?;
            let developer = validate_optional_text(
                item.developer.as_deref(),
                MAX_FACET_VALUE,
                "El estudio del lanzamiento es demasiado largo.",
            )?;
            let publisher = validate_optional_text(
                item.publisher.as_deref(),
                MAX_FACET_VALUE,
                "La distribuidora del lanzamiento es demasiado larga.",
            )?;
            let short_description = validate_optional_text(
                item.short_description.as_deref(),
                MAX_UPCOMING_DESCRIPTION,
                "La descripción del lanzamiento es demasiado larga.",
            )?;
            let release_date =
                validate_release_date(item.release_date.as_deref(), item.release_date_is_exact)?;
            let capsule_url = validate_art_url(item.capsule_url.as_deref())?;
            let header_url = validate_art_url(item.header_url.as_deref())?;
            let genres_json = serde_json::to_string(&genres).map_err(|_| {
                AppError::new(
                    "database_data",
                    "No se pudieron serializar los géneros del lanzamiento.",
                )
            })?;
            let categories_json = serde_json::to_string(&categories).map_err(|_| {
                AppError::new(
                    "database_data",
                    "No se pudieron serializar las categorías del lanzamiento.",
                )
            })?;

            let existed: bool = exists.query_row([item.app_id], |row| {
                row.get::<_, i64>(0).map(|value| value != 0)
            })?;
            upsert.execute(params![
                item.app_id,
                title,
                capsule_url,
                header_url,
                release_date,
                i64::from(item.release_date_is_exact),
                genres_json,
                categories_json,
                developer,
                publisher,
                short_description,
                item.source.trim(),
            ])?;
            if existed {
                summary.updated += 1;
            } else {
                summary.inserted += 1;
            }
        }
    }
    transaction.commit()?;
    Ok(summary)
}

fn upcoming_facets(
    genres: &[String],
    categories: &[String],
    developer: Option<&String>,
    publisher: Option<&String>,
) -> Vec<(Facet, String)> {
    let mut facets: Vec<(Facet, String)> = Vec::new();
    for value in genres {
        facets.push((Facet::Genre, value.clone()));
    }
    for value in categories {
        facets.push((Facet::Category, value.clone()));
    }
    if let Some(value) = developer {
        facets.push((Facet::Developer, value.clone()));
    }
    if let Some(value) = publisher {
        facets.push((Facet::Publisher, value.clone()));
    }
    facets.retain(|(_, value)| !value.trim().is_empty());
    facets
}

fn load_dismissed_upcoming_facets(
    connection: &Connection,
) -> AppResult<Vec<Vec<(Facet, String)>>> {
    let mut statement = connection.prepare(
        "SELECT genres_json, categories_json, developer, publisher
           FROM upcoming_releases
          WHERE dismissed_at IS NOT NULL
          ORDER BY app_id ASC",
    )?;
    let rows = statement
        .query_map([], |row| {
            let genres = parse_facet_list(&row.get::<_, String>(0)?);
            let categories = parse_facet_list(&row.get::<_, String>(1)?);
            let developer: Option<String> = row.get(2)?;
            let publisher: Option<String> = row.get(3)?;
            Ok(upcoming_facets(
                &genres,
                &categories,
                developer.as_ref(),
                publisher.as_ref(),
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

/// Redacta la razón de coincidencia nombrando **solo** facetas del candidato.
///
/// La entrada es el resultado de comparar las facetas de ese candidato con el
/// modelo, así que por construcción no puede aparecer una faceta que el juego no
/// tenga. La prueba `match_reason_only_names_facets_the_game_has` lo verifica.
fn compose_match_reason(result: &FacetMatch, minutes: &BTreeMap<FacetKey, u64>) -> String {
    let notable: Vec<&MatchedFacet> = result
        .positives
        .iter()
        .filter(|item| item.weight >= UPCOMING_REASON_MIN_WEIGHT)
        .take(UPCOMING_REASON_FACETS)
        .collect();

    if notable.is_empty() {
        if let Some(worst) = result.negatives.first() {
            return format!(
                "No encaja con tus señales: descartaste cosas parecidas en {} ({}).",
                worst.display,
                worst.facet.label()
            );
        }
        return "Todavía no hay señales en tu biblioteca que lo relacionen con nada.".to_string();
    }

    let fragments: Vec<String> = notable
        .iter()
        .map(|item| {
            let key = (item.facet, normalize_facet_value(&item.display));
            let played = minutes.get(&key).copied().unwrap_or(0);
            match item.facet {
                Facet::Developer | Facet::Publisher => item.display.clone(),
                _ if played >= UPCOMING_HOURS_THRESHOLD_MINUTES => {
                    format!("tus {} h en {}", played / 60, item.display)
                }
                _ => format!("tu interés por {}", item.display),
            }
        })
        .collect();

    let joined = match fragments.len() {
        1 => fragments[0].clone(),
        2 => format!("{} y con {}", fragments[0], fragments[1]),
        _ => format!(
            "{} y con {}",
            fragments[..fragments.len() - 1].join(", con "),
            fragments[fragments.len() - 1]
        ),
    };
    format!("Coincide con {joined}.")
}

/// Puntúa los candidatos que no están descartados contra el modelo local y
/// guarda `match_score` y `match_reason`. Devuelve cuántas filas se puntuaron.
pub fn score_upcoming(connection: &mut Connection, now: DateTime<Utc>) -> AppResult<usize> {
    let weights = load_taste_weights(connection)?;
    let library = load_priority_rows(connection, &BTreeMap::new(), &BTreeMap::new())?;
    let minutes = facet_minutes(&library);
    let updated_at = timestamp(now);

    struct Candidate {
        app_id: u32,
        facets: Vec<(Facet, String)>,
    }

    let candidates = {
        let mut statement = connection.prepare(
            "SELECT app_id, genres_json, categories_json, developer, publisher
               FROM upcoming_releases
              WHERE dismissed_at IS NULL
              ORDER BY app_id ASC",
        )?;
        statement
            .query_map([], |row| {
                let genres = parse_facet_list(&row.get::<_, String>(1)?);
                let categories = parse_facet_list(&row.get::<_, String>(2)?);
                let developer: Option<String> = row.get(3)?;
                let publisher: Option<String> = row.get(4)?;
                Ok(Candidate {
                    app_id: row.get(0)?,
                    facets: upcoming_facets(
                        &genres,
                        &categories,
                        developer.as_ref(),
                        publisher.as_ref(),
                    ),
                })
            })?
            .collect::<Result<Vec<_>, _>>()?
    };

    let transaction = connection.transaction()?;
    {
        let mut update = transaction.prepare_cached(
            "UPDATE upcoming_releases
                SET match_score = ?2, match_reason = ?3, updated_at = ?4
              WHERE app_id = ?1",
        )?;
        for candidate in &candidates {
            let result = affinity_for(&candidate.facets, &weights);
            let reason = compose_match_reason(&result, &minutes);
            update.execute(params![candidate.app_id, result.score, reason, updated_at])?;
        }
    }
    transaction.commit()?;
    Ok(candidates.len())
}

fn map_upcoming(row: &Row<'_>) -> rusqlite::Result<UpcomingRelease> {
    Ok(UpcomingRelease {
        app_id: row.get(0)?,
        title: row.get(1)?,
        capsule_url: row.get(2)?,
        header_url: row.get(3)?,
        release_date: row.get(4)?,
        release_date_is_exact: row.get::<_, i64>(5)? != 0,
        genres: parse_facet_list(&row.get::<_, String>(6)?),
        categories: parse_facet_list(&row.get::<_, String>(7)?),
        developer: row.get(8)?,
        publisher: row.get(9)?,
        short_description: row.get(10)?,
        match_score: round4(row.get::<_, f64>(11)?),
        match_reason: row.get(12)?,
        source: row.get(13)?,
        dismissed_at: row.get(14)?,
        discovered_at: row.get(15)?,
        updated_at: row.get(16)?,
    })
}

/// Próximos lanzamientos vivos, de mejor coincidencia a peor y, a igualdad, por
/// fecha más cercana. Los que tienen fecha van antes que los que no: una fecha
/// concreta es información y «próximamente» no lo es.
pub fn list_upcoming(connection: &Connection, limit: u32) -> AppResult<Vec<UpcomingRelease>> {
    let limit = limit.clamp(1, MAX_LIST_LIMIT);
    let mut statement = connection.prepare(
        "SELECT app_id, title, capsule_url, header_url, release_date, release_date_is_exact,
                genres_json, categories_json, developer, publisher, short_description,
                match_score, match_reason, source, dismissed_at, discovered_at, updated_at
           FROM upcoming_releases
          WHERE dismissed_at IS NULL
          ORDER BY match_score DESC,
                   (release_date IS NULL) ASC,
                   release_date ASC,
                   app_id ASC
          LIMIT ?1",
    )?;
    let items = statement
        .query_map([limit], map_upcoming)?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(items)
}

/// Descarta un próximo lanzamiento. La decisión se conserva: el candidato deja
/// de aparecer y sus facetas pasan a contar como evidencia negativa en el
/// siguiente [`learn_taste`].
pub fn dismiss_upcoming(connection: &Connection, app_id: u32) -> AppResult<()> {
    if app_id == 0 {
        return Err(AppError::validation("El AppID indicado no es válido."));
    }
    let changed = connection.execute(
        "UPDATE upcoming_releases
            SET dismissed_at = COALESCE(dismissed_at, strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
                updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
          WHERE app_id = ?1",
        [app_id],
    )?;
    if changed == 0 {
        return Err(AppError::not_found(
            "Ese lanzamiento ya no está entre los candidatos.",
        ));
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Pruebas
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::migrations;

    /// Instante fijo de referencia. Todas las pruebas usan el mismo `now` para
    /// que los números sean comprobables a mano.
    fn now() -> DateTime<Utc> {
        DateTime::parse_from_rfc3339("2026-08-18T12:00:00Z")
            .expect("instante de referencia")
            .with_timezone(&Utc)
    }

    fn database() -> Connection {
        let mut connection = Connection::open_in_memory().expect("abrir SQLite");
        migrations::migrate(&mut connection).expect("aplicar migraciones");
        // Las claves foráneas se activan como en producción: si el motor
        // dependiese de tenerlas apagadas, se vería aquí y no en el equipo de
        // la persona usuaria.
        connection
            .execute_batch(
                "PRAGMA foreign_keys = ON;
                 INSERT INTO statuses(id, name, color, position, built_in)
                 VALUES ('unclassified', 'Sin clasificar', '#71838E', 0, 1);",
            )
            .expect("sembrar estados");
        connection
    }

    fn insert_game(connection: &Connection, app_id: u32, title: &str, genres: &[&str]) {
        insert_game_full(connection, app_id, title, genres, None, 0, "2024-01-01T00:00:00Z");
    }

    fn insert_game_full(
        connection: &Connection,
        app_id: u32,
        title: &str,
        genres: &[&str],
        developer: Option<&str>,
        playtime_minutes: u64,
        imported_at: &str,
    ) {
        connection
            .execute(
                "INSERT INTO games(
                    app_id, title, genres_json, categories_json, developer,
                    playtime_minutes, imported_at
                 ) VALUES (?1, ?2, ?3, '[]', ?4, ?5, ?6)",
                params![
                    app_id,
                    title,
                    serde_json::to_string(genres).expect("serializar géneros"),
                    developer,
                    playtime_minutes as i64,
                    imported_at,
                ],
            )
            .expect("insertar juego");
        connection
            .execute(
                "INSERT INTO game_personal(app_id, status_id) VALUES (?1, 'unclassified')",
                [app_id],
            )
            .expect("insertar organización personal");
    }

    fn stored_score(connection: &Connection, app_id: u32) -> f64 {
        connection
            .query_row(
                "SELECT priority_score FROM game_personal WHERE app_id = ?1",
                [app_id],
                |row| row.get(0),
            )
            .expect("leer puntuación")
    }

    fn stored_reason(connection: &Connection, app_id: u32) -> String {
        connection
            .query_row(
                "SELECT COALESCE(priority_reason, '') FROM game_personal WHERE app_id = ?1",
                [app_id],
                |row| row.get(0),
            )
            .expect("leer razón")
    }

    /// Vuelca todo lo que escribe un recálculo, para poder comparar dos
    /// ejecuciones byte a byte.
    fn dump(connection: &Connection) -> String {
        let mut lines = Vec::new();
        let mut personal = connection
            .prepare(
                "SELECT app_id, priority_score, priority_locked,
                        COALESCE(priority_reason, ''), COALESCE(priority_computed_at, '')
                   FROM game_personal ORDER BY app_id",
            )
            .expect("preparar volcado personal");
        let rows = personal
            .query_map([], |row| {
                Ok(format!(
                    "personal {} {:?} {} {} {}",
                    row.get::<_, u32>(0)?,
                    row.get::<_, f64>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                ))
            })
            .expect("volcar personal")
            .collect::<Result<Vec<_>, _>>()
            .expect("recoger personal");
        lines.extend(rows);

        let mut signals = connection
            .prepare(
                "SELECT app_id, signal, weight, detail, computed_at
                   FROM priority_signals ORDER BY app_id, signal",
            )
            .expect("preparar volcado de señales");
        let rows = signals
            .query_map([], |row| {
                Ok(format!(
                    "signal {} {} {:?} {} {}",
                    row.get::<_, u32>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, f64>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                ))
            })
            .expect("volcar señales")
            .collect::<Result<Vec<_>, _>>()
            .expect("recoger señales");
        lines.extend(rows);
        lines.join("\n")
    }

    fn base_input(app_id: u32) -> PriorityInput {
        PriorityInput {
            app_id,
            title: format!("Juego {app_id}"),
            progress: 0,
            manual_priority: 0,
            locked: false,
            installed: false,
            pinned: false,
            tracking: false,
            rating: None,
            completed_at: None,
            abandoned_at: None,
            target_date: None,
            playtime_minutes: 0,
            playtime_recent_minutes: 0,
            last_played_at: None,
            imported_at: None,
            recent_sessions: 0,
            taste_affinity: 0.0,
        }
    }

    fn date(value: &str) -> NaiveDate {
        NaiveDate::parse_from_str(value, "%Y-%m-%d").expect("fecha ISO")
    }

    // -- Motor de prioridad -------------------------------------------------

    #[test]
    fn finishing_a_game_lowers_it_below_one_with_live_progress() {
        let connection = database();
        insert_game(&connection, 10, "A medias", &[]);
        insert_game(&connection, 20, "Terminado", &[]);
        connection
            .execute_batch(
                "UPDATE game_personal SET progress = 45 WHERE app_id = 10;
                 UPDATE game_personal SET progress = 100, completed_at = '2026-08-06'
                  WHERE app_id = 20;",
            )
            .expect("preparar estados");

        let mut connection = connection;
        let report = recompute_priorities(&mut connection, now()).expect("recalcular");
        assert_eq!(report.evaluated, 2);
        assert_eq!(report.settled, 1);

        let live = stored_score(&connection, 10);
        let finished = stored_score(&connection, 20);
        assert!(
            finished < live,
            "terminar debe bajar la prioridad: {finished} debería ser menor que {live}"
        );
        assert_eq!(
            stored_reason(&connection, 20),
            "Terminado hace 12 días: la prioridad baja aunque sigas jugándolo."
        );
    }

    #[test]
    fn the_completion_penalty_decays_with_time() {
        let recent = PriorityInput {
            completed_at: Some(date("2026-08-17")),
            ..base_input(10)
        };
        let ancient = PriorityInput {
            completed_at: Some(date("2024-08-18")),
            ..base_input(20)
        };
        let weight = |input: &PriorityInput| {
            evaluate_priority(input, now())
                .signals
                .into_iter()
                .find(|item| item.signal == "completed")
                .expect("señal de terminado")
                .weight
        };
        let recent_weight = weight(&recent);
        let ancient_weight = weight(&ancient);
        assert!(recent_weight < 0.0 && ancient_weight < 0.0);
        assert!(
            ancient_weight > recent_weight,
            "la penalización de hace dos años ({ancient_weight}) debe ser más suave que la de ayer ({recent_weight})"
        );
        // Dos años son algo más de seis semividas de 120 días: queda menos del 2 %.
        assert!(ancient_weight.abs() < COMPLETED_PENALTY * 0.02);
        assert!(recent_weight.abs() > COMPLETED_PENALTY * 0.99);
    }

    #[test]
    fn settled_games_never_outrank_live_progress() {
        // Un juego terminado con todos los impulsos disponibles a la vez.
        let settled = PriorityInput {
            progress: 100,
            completed_at: Some(date("2020-01-01")),
            installed: true,
            pinned: true,
            tracking: true,
            rating: Some(10),
            target_date: Some(date("2026-01-01")),
            playtime_recent_minutes: 5_000,
            last_played_at: Some(now()),
            recent_sessions: 20,
            taste_affinity: 1.0,
            ..base_input(10)
        };
        // Un juego con el progreso vivo más flojo posible y ninguna otra señal.
        let live = PriorityInput {
            progress: PROGRESS_ALIVE_MIN,
            ..base_input(20)
        };
        let settled_score = evaluate_priority(&settled, now()).score;
        let live_score = evaluate_priority(&live, now()).score;
        assert!(settled_score <= SETTLED_CEILING);
        assert!(live_score >= LIVE_PROGRESS_FLOOR);
        assert!(settled_score < live_score);
    }

    #[test]
    fn abandoning_hurts_more_than_finishing_on_the_same_day() {
        let completed = PriorityInput {
            completed_at: Some(date("2026-08-18")),
            ..base_input(10)
        };
        let abandoned = PriorityInput {
            abandoned_at: Some(date("2026-08-18")),
            ..base_input(20)
        };
        assert!(
            evaluate_priority(&abandoned, now()).score < evaluate_priority(&completed, now()).score
        );
    }

    #[test]
    fn signal_weights_reconstruct_the_stored_score() {
        let inputs = [
            PriorityInput {
                progress: 60,
                pinned: true,
                tracking: true,
                installed: true,
                rating: Some(10),
                target_date: Some(date("2026-08-01")),
                playtime_recent_minutes: 900,
                last_played_at: Some(now()),
                recent_sessions: 9,
                taste_affinity: 1.0,
                ..base_input(10)
            },
            PriorityInput {
                progress: 100,
                completed_at: Some(date("2026-08-17")),
                pinned: true,
                installed: true,
                rating: Some(10),
                ..base_input(20)
            },
            PriorityInput {
                abandoned_at: Some(date("2026-08-17")),
                ..base_input(30)
            },
            base_input(40),
        ];
        for input in &inputs {
            let outcome = evaluate_priority(input, now());
            let total = BASE_SCORE + outcome.signals.iter().map(|item| item.weight).sum::<f64>();
            assert!(
                (total - outcome.score).abs() < 1e-6,
                "la suma de señales ({total}) debe reconstruir la puntuación ({}) del juego {}",
                outcome.score,
                input.app_id
            );
        }
    }

    #[test]
    fn an_overdue_target_date_pushes_harder_than_an_upcoming_one() {
        let overdue = PriorityInput {
            target_date: Some(date("2026-08-01")),
            ..base_input(10)
        };
        let upcoming = PriorityInput {
            target_date: Some(date("2026-08-18")),
            ..base_input(20)
        };
        let far = PriorityInput {
            target_date: Some(date("2026-12-01")),
            ..base_input(30)
        };
        let score = |input: &PriorityInput| evaluate_priority(input, now()).score;
        assert!(score(&overdue) > score(&upcoming));
        assert!(score(&upcoming) > score(&far));
    }

    #[test]
    fn a_manual_locked_priority_is_never_overwritten() {
        let connection = database();
        insert_game(&connection, 10, "Anclado", &[]);
        insert_game(&connection, 20, "Vivo", &[]);
        connection
            .execute_batch(
                "UPDATE game_personal
                    SET priority = 5, priority_locked = 1, progress = 100,
                        completed_at = '2026-08-17'
                  WHERE app_id = 10;
                 UPDATE game_personal SET progress = 60 WHERE app_id = 20;",
            )
            .expect("anclar prioridad");

        let mut connection = connection;
        let report = recompute_priorities(&mut connection, now()).expect("recalcular");
        assert_eq!(report.locked, 1);

        let (priority, locked): (i64, i64) = connection
            .query_row(
                "SELECT priority, priority_locked FROM game_personal WHERE app_id = 10",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .expect("releer prioridad manual");
        assert_eq!(priority, 5, "la prioridad manual no se toca");
        assert_eq!(locked, 1, "el anclaje no se toca");

        let explanation = explain_priority(&connection, 10).expect("explicar");
        assert!(explanation.locked);
        assert_eq!(explanation.manual_priority, 5);
        assert_eq!(explanation.effective_score, 100.0);
        assert!(explanation.derived_priority < 5);
        assert_eq!(
            explanation.manual_override.as_deref(),
            Some(
                format!(
                    "Tu prioridad manual dice 5; las señales dicen {}. Manda la tuya.",
                    explanation.derived_priority
                )
                .as_str()
            )
        );

        // Y el anclaje decide el orden aunque las señales digan lo contrario.
        let ranking = list_priority_ranking(&connection, 10).expect("ranking");
        assert_eq!(ranking.first().map(|item| item.app_id), Some(10));
        assert!(ranking[0].score < ranking[1].score);
    }

    #[test]
    fn the_lock_can_be_set_and_released_and_rejects_unknown_games() {
        let connection = database();
        insert_game(&connection, 10, "Anclable", &[]);
        set_priority_lock(&connection, 10, true).expect("anclar");
        assert!(explain_priority(&connection, 10).expect("explicar").locked);
        set_priority_lock(&connection, 10, false).expect("soltar");
        assert!(!explain_priority(&connection, 10).expect("explicar").locked);
        let error = set_priority_lock(&connection, 999, true).expect_err("rechazar desconocido");
        assert_eq!(error.code, "not_found");
        let error = set_priority_lock(&connection, 0, true).expect_err("rechazar AppID cero");
        assert_eq!(error.code, "validation");
    }

    #[test]
    fn two_runs_with_the_same_instant_are_byte_identical() {
        let connection = database();
        insert_game_full(
            &connection,
            10,
            "Metroidvania",
            &["Metroidvania", "Acción"],
            Some("Team Cherry"),
            4_000,
            "2023-01-01T00:00:00Z",
        );
        insert_game_full(
            &connection,
            20,
            "Estrategia",
            &["Estrategia"],
            Some("Otro estudio"),
            120,
            "2022-05-05T00:00:00Z",
        );
        insert_game(&connection, 30, "Sin estrenar", &[]);
        connection
            .execute_batch(
                "UPDATE game_personal
                    SET progress = 55, installed = 1, tracking = 1, rating = 9,
                        target_date = '2026-08-25'
                  WHERE app_id = 10;
                 UPDATE game_personal
                    SET progress = 100, completed_at = '2026-02-02', pinned = 1
                  WHERE app_id = 20;
                 INSERT INTO game_sessions(id, app_id, started_at, ended_at)
                 VALUES ('s1', 10, '2026-08-10T10:00:00Z', '2026-08-10T12:00:00Z'),
                        ('s2', 10, '2026-08-12T10:00:00Z', '2026-08-12T12:00:00Z');",
            )
            .expect("preparar biblioteca");

        let mut connection = connection;
        let first = recompute_priorities(&mut connection, now()).expect("primer recálculo");
        let first_dump = dump(&connection);
        let second = recompute_priorities(&mut connection, now()).expect("segundo recálculo");
        let second_dump = dump(&connection);

        assert_eq!(first_dump, second_dump);
        assert_eq!(first.highlights, second.highlights);
        assert_eq!(first.evaluated, second.evaluated);
        // El segundo recálculo no encuentra nada que cambiar.
        assert_eq!(second.updated, 0);
    }

    #[test]
    fn an_empty_library_recomputes_without_drama() {
        let mut connection = database();
        let report = recompute_priorities(&mut connection, now()).expect("recalcular vacío");
        assert_eq!(report.evaluated, 0);
        assert_eq!(report.updated, 0);
        assert_eq!(report.signals_written, 0);
        assert!(report.highlights.is_empty());
        assert!(
            list_priority_ranking(&connection, 10)
                .expect("ranking vacío")
                .is_empty()
        );

        let taste = learn_taste(&mut connection, now()).expect("aprender sin biblioteca");
        assert_eq!(taste.games_analyzed, 0);
        assert_eq!(taste.facets_learned, 0);
        assert!(taste.highlights.is_empty());

        assert_eq!(
            score_upcoming(&mut connection, now()).expect("puntuar sin candidatos"),
            0
        );
        assert!(
            list_upcoming(&connection, 10)
                .expect("listar sin candidatos")
                .is_empty()
        );
    }

    #[test]
    fn a_single_game_library_still_produces_a_reason_and_a_ranking() {
        let connection = database();
        // Importado hace tres días: todavía no hay ninguna señal que contar.
        insert_game_full(
            &connection,
            10,
            "Único",
            &["Aventura"],
            None,
            0,
            "2026-08-15T00:00:00Z",
        );
        let mut connection = connection;
        let report = recompute_priorities(&mut connection, now()).expect("recalcular");
        assert_eq!(report.evaluated, 1);
        assert_eq!(report.highlights.len(), 1);
        assert_eq!(stored_score(&connection, 10), BASE_SCORE);
        assert_eq!(
            stored_reason(&connection, 10),
            "Sin señales todavía: se queda en la mitad de la lista."
        );

        let explanation = explain_priority(&connection, 10).expect("explicar");
        assert!(explanation.signals.is_empty());
        assert_eq!(explanation.derived_priority, derived_priority(BASE_SCORE));
        assert_eq!(explanation.manual_override, None);

        let taste = learn_taste(&mut connection, now()).expect("aprender con un juego");
        assert_eq!(taste.games_analyzed, 1);
        // Un único juego neutro no genera peso suficiente para guardarse.
        assert_eq!(taste.facets_learned, 0);
    }

    #[test]
    fn an_untouched_game_bought_long_ago_gets_a_rescue_boost() {
        let connection = database();
        // Importado el 1 de enero de 2024: 960 días antes del instante de
        // referencia. El impulso es `DORMANT_MAX · (1 − 0,5^(960/365))`.
        insert_game(&connection, 10, "Sin estrenar", &[]);
        let mut connection = connection;
        recompute_priorities(&mut connection, now()).expect("recalcular");

        let expected = round4(DORMANT_MAX * (1.0 - 0.5_f64.powf(960.0 / DORMANT_HALF_LIFE_DAYS)));
        assert_eq!(stored_score(&connection, 10), round4(BASE_SCORE + expected));
        assert_eq!(
            stored_reason(&connection, 10),
            "En la biblioteca desde hace 960 días y todavía sin estrenar."
        );
        // Y aun así se queda por debajo de cualquier progreso vivo.
        assert!(BASE_SCORE + expected < LIVE_PROGRESS_FLOOR);
    }

    #[test]
    fn explaining_an_unknown_game_fails_cleanly() {
        let connection = database();
        let error = explain_priority(&connection, 4_242).expect_err("rechazar desconocido");
        assert_eq!(error.code, "not_found");
    }

    // -- Modelo de gustos ---------------------------------------------------

    #[test]
    fn the_taste_model_normalises_unequal_sample_sizes() {
        let connection = database();
        // Tres juegos entusiastas de «Corto» y doce idénticos de «Largo».
        for app_id in 1..=15u32 {
            let genre = if app_id <= 3 { "Corto" } else { "Largo" };
            insert_game_full(
                &connection,
                app_id,
                &format!("Juego {app_id}"),
                &[genre],
                None,
                5_000,
                "2023-01-01T00:00:00Z",
            );
        }
        connection
            .execute(
                "UPDATE game_personal
                    SET progress = 100, completed_at = '2026-01-01', pinned = 1, rating = 10",
                [],
            )
            .expect("marcar evidencia positiva idéntica");

        let mut connection = connection;
        let report = learn_taste(&mut connection, now()).expect("aprender");
        assert_eq!(report.games_analyzed, 15);

        let weight = |value: &str| -> f64 {
            connection
                .query_row(
                    "SELECT weight FROM taste_weights WHERE facet = 'genre' AND value = ?1",
                    [value],
                    |row| row.get(0),
                )
                .expect("leer peso")
        };
        // Media bayesiana con previo 0 y fuerza 5: Σ / (n + 5).
        assert_eq!(weight("Corto"), round4(3.0 / 8.0));
        assert_eq!(weight("Largo"), round4(12.0 / 17.0));
        assert!(
            weight("Largo") > weight("Corto"),
            "doce juegos deben pesar más que tres con la misma evidencia por juego"
        );
    }

    #[test]
    fn abandoning_and_dismissing_push_a_facet_into_negative_territory() {
        let connection = database();
        for app_id in 1..=4u32 {
            insert_game_full(
                &connection,
                app_id,
                &format!("Rechazado {app_id}"),
                &["Terror"],
                None,
                10,
                "2023-01-01T00:00:00Z",
            );
        }
        connection
            .execute(
                "UPDATE game_personal SET abandoned_at = '2026-03-01', rating = 2",
                [],
            )
            .expect("marcar abandono y nota baja");

        let mut connection = connection;
        learn_taste(&mut connection, now()).expect("aprender");
        let weight: f64 = connection
            .query_row(
                "SELECT weight FROM taste_weights WHERE facet = 'genre' AND value = 'Terror'",
                [],
                |row| row.get(0),
            )
            .expect("leer peso");
        assert!(weight < 0.0, "el peso debería ser negativo, es {weight}");
    }

    #[test]
    fn the_taste_affinity_signal_only_appears_when_the_model_says_so() {
        let connection = database();
        for app_id in 1..=10u32 {
            insert_game_full(
                &connection,
                app_id,
                &format!("Favorito {app_id}"),
                &["Metroidvania"],
                Some("Team Cherry"),
                5_000,
                "2023-01-01T00:00:00Z",
            );
        }
        connection
            .execute(
                "UPDATE game_personal SET progress = 100, completed_at = '2026-01-01', rating = 10",
                [],
            )
            .expect("marcar evidencia");
        insert_game_full(
            &connection,
            99,
            "Ajeno",
            &["Deportes"],
            Some("Nadie"),
            0,
            "2026-08-01T00:00:00Z",
        );

        let mut connection = connection;
        learn_taste(&mut connection, now()).expect("aprender");
        recompute_priorities(&mut connection, now()).expect("recalcular");

        let has_affinity = |app_id: u32| -> bool {
            connection
                .query_row(
                    "SELECT EXISTS(
                        SELECT 1 FROM priority_signals
                         WHERE app_id = ?1 AND signal = 'taste_affinity'
                     )",
                    [app_id],
                    |row| row.get::<_, i64>(0).map(|value| value != 0),
                )
                .expect("consultar señal")
        };
        assert!(has_affinity(1), "un favorito debe tener afinidad");
        assert!(!has_affinity(99), "un género ajeno no debe inventar afinidad");
    }

    // -- Próximos lanzamientos ---------------------------------------------

    fn upcoming(app_id: u32, title: &str, genres: &[&str], developer: Option<&str>)
    -> ImportedUpcomingRelease {
        ImportedUpcomingRelease {
            app_id,
            title: title.to_string(),
            capsule_url: None,
            header_url: None,
            release_date: Some("2026-11-04".to_string()),
            release_date_is_exact: true,
            genres: genres.iter().map(|value| (*value).to_string()).collect(),
            categories: Vec::new(),
            developer: developer.map(str::to_owned),
            publisher: None,
            short_description: None,
            source: "store".to_string(),
        }
    }

    /// Biblioteca con un gusto marcado por «Metroidvania» y por «Estrategia»,
    /// para poder comprobar que la razón solo nombra lo que el candidato tiene.
    fn library_with_two_tastes() -> Connection {
        let connection = database();
        for app_id in 1..=6u32 {
            insert_game_full(
                &connection,
                app_id,
                &format!("Metroid {app_id}"),
                &["Metroidvania"],
                Some("Team Cherry"),
                4_000,
                "2023-01-01T00:00:00Z",
            );
        }
        for app_id in 7..=12u32 {
            insert_game_full(
                &connection,
                app_id,
                &format!("Estratega {app_id}"),
                &["Estrategia"],
                Some("Otro estudio"),
                4_000,
                "2023-01-01T00:00:00Z",
            );
        }
        connection
            .execute(
                "UPDATE game_personal SET progress = 100, completed_at = '2026-01-01', rating = 10",
                [],
            )
            .expect("marcar evidencia positiva");
        connection
    }

    #[test]
    fn match_reason_only_names_facets_the_game_has() {
        let mut connection = library_with_two_tastes();
        learn_taste(&mut connection, now()).expect("aprender");
        upsert_upcoming(
            &mut connection,
            &[upcoming(500, "Silksong", &["Metroidvania"], Some("Team Cherry"))],
        )
        .expect("importar candidato");
        assert_eq!(score_upcoming(&mut connection, now()).expect("puntuar"), 1);

        let items = list_upcoming(&connection, 10).expect("listar");
        assert_eq!(items.len(), 1);
        let reason = &items[0].match_reason;
        assert!(
            reason.contains("Metroidvania"),
            "la razón debe nombrar el género real: {reason}"
        );
        assert!(
            reason.contains("Team Cherry"),
            "la razón debe nombrar el estudio real: {reason}"
        );
        assert!(
            !reason.contains("Estrategia"),
            "la razón no puede nombrar una faceta que el juego no tiene: {reason}"
        );
        assert!(
            !reason.contains("Otro estudio"),
            "la razón no puede nombrar un estudio ajeno: {reason}"
        );
        // Y menciona horas concretas porque las hay: 6 × 4.000 min = 400 h.
        assert!(reason.contains("400 h"), "{reason}");
        assert!(items[0].match_score > 0.5);
    }

    #[test]
    fn a_candidate_with_no_shared_facets_scores_zero_and_says_so() {
        let mut connection = library_with_two_tastes();
        learn_taste(&mut connection, now()).expect("aprender");
        upsert_upcoming(
            &mut connection,
            &[upcoming(600, "Deportivo", &["Deportes"], Some("Nadie"))],
        )
        .expect("importar candidato");
        score_upcoming(&mut connection, now()).expect("puntuar");

        let items = list_upcoming(&connection, 10).expect("listar");
        assert_eq!(items[0].match_score, 0.0);
        assert_eq!(
            items[0].match_reason,
            "Todavía no hay señales en tu biblioteca que lo relacionen con nada."
        );
    }

    #[test]
    fn scoring_and_listing_are_deterministic_and_ordered() {
        let mut connection = library_with_two_tastes();
        learn_taste(&mut connection, now()).expect("aprender");
        upsert_upcoming(
            &mut connection,
            &[
                upcoming(500, "Silksong", &["Metroidvania"], Some("Team Cherry")),
                upcoming(600, "Deportivo", &["Deportes"], None),
                upcoming(700, "Estratega nuevo", &["Estrategia"], None),
            ],
        )
        .expect("importar candidatos");

        score_upcoming(&mut connection, now()).expect("primera pasada");
        let first = list_upcoming(&connection, 10).expect("listar");
        score_upcoming(&mut connection, now()).expect("segunda pasada");
        let second = list_upcoming(&connection, 10).expect("listar de nuevo");
        assert_eq!(first, second);

        let order: Vec<u32> = first.iter().map(|item| item.app_id).collect();
        assert_eq!(order, vec![500, 700, 600]);
    }

    #[test]
    fn dismissing_a_candidate_hides_it_and_teaches_the_model() {
        let mut connection = library_with_two_tastes();
        upsert_upcoming(
            &mut connection,
            &[upcoming(700, "Estratega nuevo", &["Estrategia"], None)],
        )
        .expect("importar candidato");
        dismiss_upcoming(&connection, 700).expect("descartar");
        assert!(list_upcoming(&connection, 10).expect("listar").is_empty());

        let report = learn_taste(&mut connection, now()).expect("aprender");
        assert_eq!(report.dismissed_upcoming_used, 1);

        let error = dismiss_upcoming(&connection, 999).expect_err("rechazar desconocido");
        assert_eq!(error.code, "not_found");
    }

    #[test]
    fn feedback_is_recorded_for_library_games_and_for_candidates() {
        let mut connection = library_with_two_tastes();
        upsert_upcoming(
            &mut connection,
            &[upcoming(700, "Estratega nuevo", &["Estrategia"], None)],
        )
        .expect("importar candidato");

        // Juego propio: la fila conserva su AppID.
        record_taste_feedback(&connection, 1, "not_interested", "library").expect("opinar");
        let stored: Option<u32> = connection
            .query_row(
                "SELECT app_id FROM taste_feedback WHERE surface = 'library'",
                [],
                |row| row.get(0),
            )
            .expect("leer opinión propia");
        assert_eq!(stored, Some(1));

        // Candidato que no está en `games`: se guarda sin AppID (la clave
        // foránea de la migración 024 no admite otra cosa) y se refleja en
        // `upcoming_releases`, que es de donde lo lee el modelo.
        record_taste_feedback(&connection, 700, "not_interested", "upcoming").expect("opinar");
        let stored: Option<u32> = connection
            .query_row(
                "SELECT app_id FROM taste_feedback WHERE surface = 'upcoming'",
                [],
                |row| row.get(0),
            )
            .expect("leer opinión de candidato");
        assert_eq!(stored, None);
        assert!(list_upcoming(&connection, 10).expect("listar").is_empty());

        // Y volver a decir que sí lo devuelve a la lista.
        record_taste_feedback(&connection, 700, "interested", "upcoming").expect("rectificar");
        assert_eq!(list_upcoming(&connection, 10).expect("listar").len(), 1);
    }

    #[test]
    fn feedback_validates_its_inputs() {
        let connection = library_with_two_tastes();
        assert_eq!(
            record_taste_feedback(&connection, 1, "meh", "library")
                .expect_err("veredicto inventado")
                .code,
            "validation"
        );
        assert_eq!(
            record_taste_feedback(&connection, 1, "interested", "inventada")
                .expect_err("superficie inventada")
                .code,
            "validation"
        );
        assert_eq!(
            record_taste_feedback(&connection, 0, "interested", "library")
                .expect_err("AppID cero")
                .code,
            "validation"
        );
        assert_eq!(
            record_taste_feedback(&connection, 9_999, "interested", "library")
                .expect_err("juego desconocido")
                .code,
            "not_found"
        );
    }

    #[test]
    fn importing_candidates_validates_and_preserves_local_decisions() {
        let mut connection = database();
        let summary = upsert_upcoming(
            &mut connection,
            &[upcoming(500, "Silksong", &["Metroidvania"], Some("Team Cherry"))],
        )
        .expect("importar");
        assert_eq!(summary.inserted, 1);
        assert_eq!(summary.updated, 0);

        dismiss_upcoming(&connection, 500).expect("descartar");
        let summary = upsert_upcoming(
            &mut connection,
            &[upcoming(500, "Silksong", &["Metroidvania"], Some("Team Cherry"))],
        )
        .expect("reimportar");
        assert_eq!(summary.updated, 1);
        // Reimportar no resucita algo que la persona usuaria descartó.
        assert!(list_upcoming(&connection, 10).expect("listar").is_empty());

        let mut invalid = upcoming(0, "Sin AppID", &[], None);
        invalid.app_id = 0;
        assert_eq!(
            upsert_upcoming(&mut connection, &[invalid])
                .expect_err("AppID cero")
                .code,
            "validation"
        );

        let mut bad_source = upcoming(600, "Origen raro", &[], None);
        bad_source.source = "inventado".to_string();
        assert_eq!(
            upsert_upcoming(&mut connection, &[bad_source])
                .expect_err("origen inventado")
                .code,
            "validation"
        );

        let mut bad_date = upcoming(700, "Fecha rara", &[], None);
        bad_date.release_date = Some("cuando sea".to_string());
        assert_eq!(
            upsert_upcoming(&mut connection, &[bad_date])
                .expect_err("fecha exacta inválida")
                .code,
            "validation"
        );

        let mut approximate = upcoming(800, "Aproximado", &[], None);
        approximate.release_date = Some("Q4 2026".to_string());
        approximate.release_date_is_exact = false;
        upsert_upcoming(&mut connection, &[approximate]).expect("admitir etiqueta aproximada");

        let mut hostile_art = upcoming(900, "Arte ajeno", &[], None);
        hostile_art.capsule_url = Some("https://ejemplo.invalido/capsule.jpg".to_string());
        assert_eq!(
            upsert_upcoming(&mut connection, &[hostile_art])
                .expect_err("host no oficial")
                .code,
            "validation"
        );

        let mut official_art = upcoming(910, "Arte oficial", &[], None);
        official_art.capsule_url =
            Some("https://shared.steamstatic.com/store_item_assets/steam/apps/910/capsule.jpg"
                .to_string());
        upsert_upcoming(&mut connection, &[official_art]).expect("admitir arte oficial");

        let duplicated = vec![
            upcoming(920, "Uno", &[], None),
            upcoming(920, "Otro", &[], None),
        ];
        assert_eq!(
            upsert_upcoming(&mut connection, &duplicated)
                .expect_err("AppID repetido")
                .code,
            "validation"
        );
    }

    #[test]
    fn facet_values_are_grouped_case_insensitively_with_a_stable_representative() {
        let connection = database();
        insert_game_full(
            &connection,
            10,
            "Mayúsculas",
            &["Metroidvania"],
            None,
            5_000,
            "2023-01-01T00:00:00Z",
        );
        insert_game_full(
            &connection,
            20,
            "Minúsculas",
            &["metroidvania"],
            None,
            5_000,
            "2023-01-01T00:00:00Z",
        );
        connection
            .execute("UPDATE game_personal SET rating = 10, pinned = 1", [])
            .expect("marcar evidencia");

        let mut connection = connection;
        learn_taste(&mut connection, now()).expect("aprender");
        let rows: Vec<(String, f64)> = {
            let mut statement = connection
                .prepare("SELECT value, weight FROM taste_weights WHERE facet = 'genre'")
                .expect("preparar");
            statement
                .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
                .expect("consultar")
                .collect::<Result<Vec<_>, _>>()
                .expect("recoger")
        };
        assert_eq!(rows.len(), 1, "las dos grafías deben ser la misma faceta");
        // El representante es el menor lexicográficamente: «Metroidvania» < «metroidvania».
        assert_eq!(rows[0].0, "Metroidvania");
    }

    #[test]
    fn a_recompute_does_not_touch_the_personal_edit_marker() {
        let connection = database();
        insert_game(&connection, 10, "Intacto", &[]);
        connection
            .execute(
                "UPDATE game_personal SET updated_at = '2020-01-01T00:00:00.000Z' WHERE app_id = 10",
                [],
            )
            .expect("fijar marca de edición");

        let mut connection = connection;
        recompute_priorities(&mut connection, now()).expect("recalcular");
        let updated_at: String = connection
            .query_row(
                "SELECT updated_at FROM game_personal WHERE app_id = 10",
                [],
                |row| row.get(0),
            )
            .expect("releer marca");
        assert_eq!(updated_at, "2020-01-01T00:00:00.000Z");
    }
}
