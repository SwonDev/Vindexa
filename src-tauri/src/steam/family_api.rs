//! Catálogo de una Familia de Steam por los servicios `IFamilyGroupsService`.
//!
//! # Por qué esta vía y no la otra
//!
//! Preguntar `IPlayerService/GetOwnedGames` por cada miembro sólo devuelve algo
//! si ese miembro tiene su biblioteca pública para la Web API Key de quien
//! pregunta, y casi nadie la tiene. Los servicios de Familia son los que usa el
//! propio cliente de Steam y se autentican con un **testigo de sesión**, así que
//! ven el catálogo completo sin depender de la privacidad de cada uno.
//!
//! El testigo lo obtiene [`crate::steam::family_session`] de la sesión abierta
//! en el navegador integrado.
//!
//! # Qué se ha comprobado y qué no
//!
//! Los dos métodos existen: pedirlos sin credencial responde `401 Unauthorized`,
//! mientras que un método inventado del mismo servicio responde `404`. Los
//! nombres de los campos de la respuesta, en cambio, **no** se han podido
//! comprobar sin una sesión real, así que aquí se leen con tolerancia y, cuando
//! no aparece lo que hace falta, se dice que Steam respondió con una forma
//! desconocida. Nunca se rellena un hueco.
//!
//! # Lo que no se guarda
//!
//! De los demás miembros no se conserva nada: ni nombre, ni avatar, ni quién
//! presta qué. Sólo los AppID del catálogo. Y catálogo visible no es licencia:
//! lo que llega por aquí entra como disponibilidad **por confirmar**, y sólo la
//! evidencia local la confirma.

use crate::error::{AppError, AppResult};
use crate::steam::family_session::SessionToken;
use crate::steam::web_api;
use serde::Deserialize;

const FAMILY_GROUP_ENDPOINT: &str =
    "https://api.steampowered.com/IFamilyGroupsService/GetFamilyGroupForUser/v1/";
const SHARED_LIBRARY_ENDPOINT: &str =
    "https://api.steampowered.com/IFamilyGroupsService/GetSharedLibraryApps/v1/";

/// Tope de aplicaciones que se aceptan de una respuesta.
///
/// Una Familia son seis cuentas; veinte mil juegos entre todas es holgado y
/// pone un límite a lo que se persiste de una sola vez.
const MAX_SHARED_APPS: usize = 20_000;

/// Longitud máxima admitida para el título que publica Steam.
const MAX_TITLE_CHARS: usize = 200;

/// Grupo familiar de la cuenta enlazada.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FamilyGroup {
    /// La cuenta pertenece a un grupo, con su identificador.
    Member { group_id: String },
    /// La cuenta no está en ninguna Familia. No es un error: es una respuesta.
    None,
}

/// Un juego del catálogo compartido.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SharedApp {
    pub app_id: u32,
    /// Título publicado por Steam. `None` cuando la respuesta no lo trae: sin
    /// título no se puede construir una ficha honesta, y quien persista decide
    /// qué hacer con ello.
    pub title: Option<String>,
}

/// Lo que devuelve una lectura del catálogo compartido.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SharedLibrary {
    pub apps: Vec<SharedApp>,
    /// Aplicaciones que la respuesta traía sin AppID utilizable. Se cuentan
    /// aparte en lugar de descartarse en silencio.
    pub unusable: usize,
}

/// Estado del vínculo con la sesión de Steam, tal y como lo enseña Ajustes.
///
/// `linked` sólo dice que hay un testigo guardado, no que siga sirviendo: eso
/// únicamente lo sabe Steam y comprobarlo costaría una petición. La caducidad
/// aparece cuando se usa, y entonces se cuenta en `last_error_code`.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FamilySessionStatus {
    pub linked: bool,
    /// Cuándo terminó bien la última sincronización.
    pub last_sync_at: Option<String>,
    /// Cuántos juegos trajo esa sincronización.
    pub last_app_count: Option<u32>,
    /// Código del último fallo, si el último intento falló.
    pub last_error_code: Option<String>,
}

/// Lo que deja una sincronización del catálogo compartido.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FamilySyncReport {
    /// Juegos del catálogo compartido que se han guardado.
    pub imported: u32,
    /// Entradas que la respuesta traía sin AppID utilizable.
    pub unusable: u32,
    /// Entradas sin título publicado, que no se pueden presentar como ficha.
    pub without_title: u32,
    /// La cuenta no pertenece a ninguna Familia. No es un fallo.
    pub no_family: bool,
}

#[derive(Debug, Deserialize)]
struct FamilyGroupEnvelope {
    #[serde(default)]
    response: Option<FamilyGroupResponse>,
}

#[derive(Debug, Deserialize)]
struct FamilyGroupResponse {
    /// Steam devuelve el identificador como texto, pero un número tampoco
    /// sorprendería: se acepta cualquiera de los dos y se normaliza a texto.
    #[serde(default)]
    family_groupid: Option<serde_json::Value>,
    #[serde(default)]
    is_not_member_of_any_group: Option<bool>,
}

#[derive(Debug, Deserialize)]
struct SharedLibraryEnvelope {
    #[serde(default)]
    response: Option<SharedLibraryResponse>,
}

#[derive(Debug, Deserialize)]
struct SharedLibraryResponse {
    #[serde(default)]
    apps: Option<Vec<SharedAppPayload>>,
}

#[derive(Debug, Deserialize)]
struct SharedAppPayload {
    #[serde(default)]
    appid: Option<serde_json::Value>,
    #[serde(default)]
    name: Option<String>,
}

/// Grupo familiar al que pertenece `steam_id`.
pub async fn fetch_family_group(token: &SessionToken, steam_id: &str) -> AppResult<FamilyGroup> {
    let client = web_api::build_client()?;
    let envelope: FamilyGroupEnvelope = web_api::get_json(
        &client,
        FAMILY_GROUP_ENDPOINT,
        &[("access_token", token.as_str()), ("steamid", steam_id)],
    )
    .await
    .map_err(session_aware)?;
    parse_family_group(envelope)
}

/// Catálogo compartido del grupo.
pub async fn fetch_shared_library(
    token: &SessionToken,
    group_id: &str,
) -> AppResult<SharedLibrary> {
    let client = web_api::build_client()?;
    let envelope: SharedLibraryEnvelope = web_api::get_json(
        &client,
        SHARED_LIBRARY_ENDPOINT,
        &[
            ("access_token", token.as_str()),
            ("family_groupid", group_id),
            // El catálogo interesa entero: lo propio ya se distingue después por
            // la biblioteca de la cuenta, y lo excluido sigue siendo parte de lo
            // que la Familia tiene.
            ("include_own", "true"),
            ("include_excluded", "true"),
            ("language", "spanish"),
        ],
    )
    .await
    .map_err(session_aware)?;
    parse_shared_library(envelope)
}

fn parse_family_group(envelope: FamilyGroupEnvelope) -> AppResult<FamilyGroup> {
    let Some(response) = envelope.response else {
        return Err(unknown_shape());
    };
    if response.is_not_member_of_any_group == Some(true) {
        return Ok(FamilyGroup::None);
    }
    let Some(raw) = response.family_groupid.as_ref() else {
        // Sin identificador y sin la marca de «no pertenece a ninguno», Steam no
        // ha dicho ni una cosa ni la otra. Adivinar cualquiera de las dos sería
        // inventar.
        return Err(unknown_shape());
    };
    let group_id = match raw {
        serde_json::Value::String(value) => value.trim().to_owned(),
        serde_json::Value::Number(value) => value.to_string(),
        _ => return Err(unknown_shape()),
    };
    if group_id.is_empty() || group_id == "0" {
        return Ok(FamilyGroup::None);
    }
    if !group_id.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(unknown_shape());
    }
    Ok(FamilyGroup::Member { group_id })
}

fn parse_shared_library(envelope: SharedLibraryEnvelope) -> AppResult<SharedLibrary> {
    let Some(response) = envelope.response else {
        return Err(unknown_shape());
    };
    // Una respuesta sin `apps` es una Familia sin catálogo visible, que es
    // distinto de una respuesta que no se entiende: la primera trae `response`.
    let payload = response.apps.unwrap_or_default();
    if payload.len() > MAX_SHARED_APPS {
        return Err(AppError::new(
            "steam_family_too_many_apps",
            format!(
                "Steam devolvió más de {MAX_SHARED_APPS} juegos compartidos, que es más de lo que Vindexa importa de una vez."
            ),
        ));
    }

    let mut apps = Vec::with_capacity(payload.len());
    let mut unusable = 0;
    let mut vistos = std::collections::HashSet::new();
    for item in payload {
        let Some(app_id) = item.appid.as_ref().and_then(as_app_id) else {
            unusable += 1;
            continue;
        };
        if !vistos.insert(app_id) {
            continue;
        }
        let title = item
            .name
            .map(|name| name.trim().to_owned())
            .filter(|name| !name.is_empty())
            .map(|mut name| {
                // Un título desmesurado es una respuesta rara, no un motivo para
                // rechazar el juego entero: se recorta por caracteres para no
                // partir un carácter multibyte por la mitad.
                if name.chars().count() > MAX_TITLE_CHARS {
                    name = name.chars().take(MAX_TITLE_CHARS).collect();
                }
                name
            });
        apps.push(SharedApp { app_id, title });
    }
    Ok(SharedLibrary { apps, unusable })
}

/// Lee un AppID venga como número o como texto, que Steam alterna según el
/// servicio.
fn as_app_id(value: &serde_json::Value) -> Option<u32> {
    let numero = match value {
        serde_json::Value::Number(numero) => numero.as_u64()?,
        serde_json::Value::String(texto) => texto.trim().parse::<u64>().ok()?,
        _ => return None,
    };
    let app_id = u32::try_from(numero).ok()?;
    (app_id != 0).then_some(app_id)
}

/// Traduce el rechazo de la Web API al lenguaje de esta credencial.
///
/// `get_json` habla de la Web API Key porque es lo que usa el resto del cajón.
/// Aquí la credencial es el testigo de sesión, y decirle a alguien que revise
/// una clave que no interviene lo manda a buscar un problema que no existe.
fn session_aware(error: AppError) -> AppError {
    if error.code == "steam_api_unauthorized" {
        return AppError::new(
            "steam_family_session_expired",
            "Tu sesión de Steam ha caducado. Vuelve a iniciarla en el navegador integrado y repite la sincronización.",
        );
    }
    error
}

fn unknown_shape() -> AppError {
    AppError::new(
        "steam_family_unknown_shape",
        "Steam respondió al catálogo de Familia con una forma que Vindexa no reconoce. No se ha importado nada.",
    )
}

#[cfg(test)]
mod tests {
    use super::{
        FamilyGroup, FamilyGroupEnvelope, SharedLibraryEnvelope, parse_family_group,
        parse_shared_library, session_aware,
    };
    use crate::error::AppError;

    fn grupo(raw: &str) -> FamilyGroupEnvelope {
        serde_json::from_str(raw).expect("respuesta de prueba válida")
    }

    fn catalogo(raw: &str) -> SharedLibraryEnvelope {
        serde_json::from_str(raw).expect("respuesta de prueba válida")
    }

    #[test]
    fn lee_el_grupo_venga_como_texto_o_como_numero() {
        let texto = parse_family_group(grupo(r#"{"response":{"family_groupid":"12345"}}"#))
            .expect("interpretar");
        assert_eq!(
            texto,
            FamilyGroup::Member {
                group_id: "12345".to_string()
            }
        );
        let numero = parse_family_group(grupo(r#"{"response":{"family_groupid":12345}}"#))
            .expect("interpretar");
        assert_eq!(
            numero,
            FamilyGroup::Member {
                group_id: "12345".to_string()
            }
        );
    }

    #[test]
    fn no_pertenecer_a_ninguna_familia_es_una_respuesta_no_un_fallo() {
        let marcado =
            parse_family_group(grupo(r#"{"response":{"is_not_member_of_any_group":true}}"#))
                .expect("interpretar");
        assert_eq!(marcado, FamilyGroup::None);

        let cero = parse_family_group(grupo(r#"{"response":{"family_groupid":"0"}}"#))
            .expect("interpretar");
        assert_eq!(cero, FamilyGroup::None);
    }

    #[test]
    fn sin_identificador_ni_marca_no_se_adivina() {
        // Steam no ha dicho ni que pertenezca ni que no. Elegir una de las dos
        // sería inventar el dato.
        let error = parse_family_group(grupo(r#"{"response":{}}"#)).expect_err("debe fallar");
        assert_eq!(error.code, "steam_family_unknown_shape");

        let vacia = parse_family_group(grupo(r#"{}"#)).expect_err("debe fallar");
        assert_eq!(vacia.code, "steam_family_unknown_shape");
    }

    #[test]
    fn recoge_el_catalogo_y_descarta_lo_inservible_contandolo() {
        let library = parse_shared_library(catalogo(
            r#"{"response":{"apps":[
                {"appid":1245620,"name":"ELDEN RING"},
                {"appid":"292030","name":"The Witcher 3"},
                {"appid":0,"name":"AppID inválido"},
                {"name":"Sin AppID"},
                {"appid":1091500}
            ]}}"#,
        ))
        .expect("interpretar");

        assert_eq!(library.apps.len(), 3);
        assert_eq!(library.apps[0].app_id, 1245620);
        assert_eq!(library.apps[0].title.as_deref(), Some("ELDEN RING"));
        assert_eq!(library.apps[1].app_id, 292030);
        // Sin nombre no se inventa uno: el hueco viaja como tal.
        assert_eq!(library.apps[2].app_id, 1091500);
        assert_eq!(library.apps[2].title, None);
        assert_eq!(library.unusable, 2);
    }

    #[test]
    fn un_appid_repetido_entra_una_sola_vez() {
        let library = parse_shared_library(catalogo(
            r#"{"response":{"apps":[{"appid":10,"name":"Uno"},{"appid":10,"name":"Uno otra vez"}]}}"#,
        ))
        .expect("interpretar");
        assert_eq!(library.apps.len(), 1);
        assert_eq!(library.apps[0].title.as_deref(), Some("Uno"));
    }

    #[test]
    fn una_familia_sin_catalogo_visible_no_es_un_error() {
        // Trae `response`, así que Steam ha contestado: simplemente no hay nada
        // compartido. Distinto de una respuesta que no se entiende.
        let library = parse_shared_library(catalogo(r#"{"response":{}}"#)).expect("interpretar");
        assert!(library.apps.is_empty());
        assert_eq!(library.unusable, 0);

        let error = parse_shared_library(catalogo(r#"{}"#)).expect_err("debe fallar");
        assert_eq!(error.code, "steam_family_unknown_shape");
    }

    #[test]
    fn un_titulo_desmesurado_se_recorta_sin_partir_un_caracter() {
        let largo = "ñ".repeat(500);
        let library = parse_shared_library(catalogo(&format!(
            r#"{{"response":{{"apps":[{{"appid":10,"name":"{largo}"}}]}}}}"#
        )))
        .expect("interpretar");
        let titulo = library.apps[0].title.as_deref().expect("hay título");
        assert_eq!(titulo.chars().count(), 200);
        assert!(titulo.chars().all(|caracter| caracter == 'ñ'));
    }

    #[test]
    fn el_rechazo_habla_del_testigo_y_no_de_la_clave_web_api() {
        let traducido = session_aware(AppError::new("steam_api_unauthorized", "da igual"));
        assert_eq!(traducido.code, "steam_family_session_expired");
        assert!(traducido.message.contains("sesión de Steam"));

        // Cualquier otro error pasa tal cual: traducirlo todo escondería la
        // causa real.
        let intacto = session_aware(AppError::new("steam_rate_limited", "espera"));
        assert_eq!(intacto.code, "steam_rate_limited");
    }
}
