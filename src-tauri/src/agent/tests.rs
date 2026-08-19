//! Batería del puente de agentes.
//!
//! ## Nota sobre el arranque de la base
//!
//! El resto de `db` usa `Connection::open_in_memory()` seguido de
//! `crate::db::migrations::migrate`. Ese módulo está declarado como
//! `mod migrations;` —privado— dentro de `db/mod.rs`, así que no es alcanzable
//! desde `crate::agent`. Aquí se usa la ruta pública equivalente,
//! `Database::initialize()`, que aplica exactamente las mismas migraciones y
//! además siembra estados y columnas de planificador reales, que es lo que
//! estas pruebas necesitan. El informe de integración propone el cambio de una
//! línea (`pub(crate) mod migrations;`) para poder usar el patrón en memoria.

use super::*;
use crate::agent::audit::AuditResult;
use crate::agent::clients::NewAgentClient;
use crate::agent::crypto::{from_hex, hmac_sha256, pbkdf2_hmac_sha256, sha256, to_hex};
use crate::agent::executor::{Resolved, resolve};
use crate::agent::intent::{AgentIntent, AgentQuery, EntitySelector, GameSelector};
use crate::agent::matching::{GameIndexEntry, GameMatch};
use crate::agent::ratelimit::RateLimiter;
use crate::agent::token::TokenPolicy;
use crate::db::Database;
use rusqlite::Connection;
use tempfile::TempDir;

// ---------------------------------------------------------------------------
// Utilidades
// ---------------------------------------------------------------------------

struct Fixture {
    _directory: TempDir,
    connection: Connection,
    limiter: RateLimiter,
}

fn fixture() -> Fixture {
    let directory = tempfile::tempdir().expect("crear directorio temporal");
    let database = Database::new(directory.path().join("vindexa.sqlite3"));
    database.initialize().expect("inicializar la base");
    let connection = database.open().expect("abrir la base");
    Fixture {
        _directory: directory,
        connection,
        limiter: RateLimiter::default(),
    }
}

fn add_game(connection: &Connection, app_id: u32, title: &str) {
    connection
        .execute(
            "INSERT INTO games(app_id, title) VALUES (?1, ?2)",
            rusqlite::params![app_id, title],
        )
        .expect("insertar juego");
    connection
        .execute(
            "INSERT INTO game_personal(app_id, status_id) VALUES (?1, 'backlog')",
            [app_id],
        )
        .expect("insertar ficha personal");
}

fn issue_client(connection: &mut Connection, name: &str, scopes: &[&str]) -> String {
    clients::issue(
        connection,
        &NewAgentClient {
            name: name.to_string(),
            kind: "hermes".to_string(),
            scopes: scopes.iter().map(|scope| (*scope).to_string()).collect(),
        },
        TokenPolicy::for_tests(),
    )
    .expect("emitir cliente")
    .token
}

fn request(token: &str, utterance: &str, intent: AgentIntent) -> AgentRequest {
    AgentRequest {
        token: token.to_string(),
        utterance: utterance.to_string(),
        intent,
    }
}

fn by_name(name: &str) -> GameSelector {
    GameSelector {
        app_id: None,
        name: Some(name.to_string()),
    }
}

fn by_id(app_id: u32) -> GameSelector {
    GameSelector {
        app_id: Some(app_id),
        name: None,
    }
}

fn audit_rows(connection: &Connection) -> Vec<(String, String, Option<String>)> {
    let mut statement = connection
        .prepare(
            "SELECT intent, result, error_message FROM agent_audit_log ORDER BY created_at, rowid",
        )
        .expect("preparar consulta de auditoría");
    let rows = statement
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
        .expect("consultar auditoría");
    rows.collect::<Result<Vec<_>, _>>().expect("leer auditoría")
}

fn personal_field<T: rusqlite::types::FromSql>(
    connection: &Connection,
    app_id: u32,
    column: &str,
) -> T {
    connection
        .query_row(
            &format!("SELECT {column} FROM game_personal WHERE app_id = ?1"),
            [app_id],
            |row| row.get(0),
        )
        .expect("leer campo personal")
}

// ---------------------------------------------------------------------------
// Criptografía: vectores oficiales
// ---------------------------------------------------------------------------

#[test]
fn sha256_reproduce_los_vectores_de_fips_180_4() {
    assert_eq!(
        to_hex(&sha256(b"")),
        "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
    );
    assert_eq!(
        to_hex(&sha256(b"abc")),
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
    assert_eq!(
        to_hex(&sha256(
            b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq"
        )),
        "248d6a61d20638b8e5c026930c3e6039a33ce45964ff2167f6ecedd419db06c1"
    );
    // Mensaje largo: obliga a recorrer varios bloques y a rellenar en el límite.
    assert_eq!(
        to_hex(&sha256(&[b'a'; 1_000_000])),
        "cdc76e5c9914fb9281a1c7e284d73e67f1809a48a497200e046d39ccc7112cd0"
    );
}

#[test]
fn hmac_sha256_reproduce_los_vectores_de_rfc_4231() {
    assert_eq!(
        to_hex(&hmac_sha256(&[0x0b; 20], b"Hi There")),
        "b0344c61d8db38535ca8afceaf0bf12b881dc200c9833da726e9376c2e32cff7"
    );
    assert_eq!(
        to_hex(&hmac_sha256(b"Jefe", b"what do ya want for nothing?")),
        "5bdcc146bf60754e6a042426089575c75a003f089d2739839dec58b964ec3843"
    );
    // Clave más larga que el bloque: obliga a resumirla antes.
    assert_eq!(
        to_hex(&hmac_sha256(
            &[0xaa; 131],
            b"Test Using Larger Than Block-Size Key - Hash Key First"
        )),
        "60e431591ee0b67f0d8a26aacbf5b77f8e0bc6213728c5140546040f0ee37f54"
    );
}

#[test]
fn pbkdf2_reproduce_los_vectores_publicados() {
    // Vectores PBKDF2-HMAC-SHA256 ampliamente citados (equivalentes SHA-256 de
    // los de RFC 6070).
    let mut derived = [0u8; 32];
    pbkdf2_hmac_sha256(b"password", b"salt", 1, &mut derived);
    assert_eq!(
        to_hex(&derived),
        "120fb6cffcf8b32c43e7225256c4f837a86548c92ccc35480805987cb70be17b"
    );
    pbkdf2_hmac_sha256(b"password", b"salt", 2, &mut derived);
    assert_eq!(
        to_hex(&derived),
        "ae4d0c95af6b46d32d0adff928f06dd02a303f8ef3c251dfd6e2d85a95474c43"
    );

    // RFC 7914, §11: salida más larga que un bloque de SHA-256.
    let mut long = [0u8; 64];
    pbkdf2_hmac_sha256(b"passwd", b"salt", 1, &mut long);
    assert_eq!(
        to_hex(&long),
        "55ac046e56e3089fec1691c22544b605f94185216dde0465e68b9d57c20dacbc\
         49ca9cccf179b64599166 4b39d77ef317c71b845b1e30bd509112041d3a19783"
            .replace(' ', "")
    );
}

#[test]
fn el_hexadecimal_va_y_vuelve() {
    let bytes = crypto::random_bytes(48);
    assert_eq!(from_hex(&to_hex(&bytes)), Some(bytes));
    assert_eq!(from_hex("abc"), None);
    assert_eq!(from_hex("zz"), None);
}

// ---------------------------------------------------------------------------
// Tokens
// ---------------------------------------------------------------------------

#[test]
fn el_token_en_claro_nunca_llega_a_sqlite() {
    let mut fixture = fixture();
    let issued = clients::issue(
        &mut fixture.connection,
        &NewAgentClient {
            name: "Hermes".to_string(),
            kind: "hermes".to_string(),
            scopes: vec!["biblioteca:leer".to_string()],
        },
        TokenPolicy::for_tests(),
    )
    .expect("emitir cliente");

    let stored: String = fixture
        .connection
        .query_row(
            "SELECT token_hash FROM agent_clients WHERE id = ?1",
            [&issued.client.id],
            |row| row.get(0),
        )
        .expect("leer resumen");

    let secret = issued.token.rsplit('_').next().expect("secreto");
    assert!(!stored.contains(secret), "el secreto no puede persistirse");
    assert!(!stored.contains(&issued.token));
    assert!(stored.starts_with("pbkdf2-sha256$1000$"));
    assert_eq!(
        stored.split('$').count(),
        4,
        "algoritmo, coste, sal y resumen"
    );

    assert!(token::verify(secret, &stored));
    assert!(!token::verify("00", &stored));

    // Dos clientes con la misma configuración obtienen sales distintas.
    let second = clients::issue(
        &mut fixture.connection,
        &NewAgentClient {
            name: "Otro".to_string(),
            kind: "generic".to_string(),
            scopes: vec!["biblioteca:leer".to_string()],
        },
        TokenPolicy::for_tests(),
    )
    .expect("emitir segundo cliente");
    let other: String = fixture
        .connection
        .query_row(
            "SELECT token_hash FROM agent_clients WHERE id = ?1",
            [&second.client.id],
            |row| row.get(0),
        )
        .expect("leer resumen");
    let salt_of = |value: &str| value.split('$').nth(2).unwrap_or_default().to_string();
    assert_ne!(salt_of(&stored), salt_of(&other), "la sal debe ser única");
}

#[test]
fn el_analisis_del_token_rechaza_lo_malformado() {
    for candidate in [
        "",
        "vdx",
        "vdx_",
        "otro_prefijo_00",
        "vdx_no-es-uuid_00",
        "vdx_8b4c2a1e-0000-4000-8000-000000000000_zz",
    ] {
        assert!(
            token::parse(candidate).is_err(),
            "debería rechazar «{candidate}»"
        );
    }
    let minted = token::mint(TokenPolicy::for_tests());
    let parsed = token::parse(&minted.plaintext).expect("token válido");
    assert_eq!(parsed.client_id, minted.client_id);
}

#[test]
fn un_resumen_manipulado_no_autentica() {
    let minted = token::mint(TokenPolicy::for_tests());
    let secret = minted.plaintext.rsplit('_').next().expect("secreto");
    assert!(token::verify(secret, &minted.hash));
    for corrupted in [
        "",
        "pbkdf2-sha256$1000$aa$bb",
        "argon2id$1000$00112233445566778899aabbccddeeff$00",
        &minted.hash.replace("pbkdf2-sha256", "sha256"),
        // Coste por debajo del mínimo aceptado.
        &minted.hash.replace("$1000$", "$1$"),
    ] {
        assert!(
            !token::verify(secret, corrupted),
            "«{corrupted}» no debería valer"
        );
    }
}

// ---------------------------------------------------------------------------
// Resolución de nombres
// ---------------------------------------------------------------------------

fn catalog(titles: &[(u32, &str)]) -> Vec<GameIndexEntry> {
    titles
        .iter()
        .map(|(app_id, title)| GameIndexEntry {
            app_id: *app_id,
            title: (*title).to_string(),
        })
        .collect()
}

#[test]
fn resuelve_nombres_exactos_con_tildes_y_puntuacion() {
    let games = catalog(&[
        (10, "Pokémon Rojo"),
        (20, "Hollow Knight"),
        (30, "El Señor de los Anillos: La Batalla"),
    ]);
    for query in [
        "Pokémon Rojo",
        "pokemon rojo",
        "POKEMON, ROJO!",
        "  Pokemon   Rojo  ",
    ] {
        match matching::resolve(query, &games) {
            GameMatch::Resolved(candidate) => {
                assert_eq!(candidate.app_id, 10, "consulta «{query}»")
            }
            other => panic!("«{query}» debería resolverse, no {other:?}"),
        }
    }
    match matching::resolve("el senor de los anillos la batalla", &games) {
        GameMatch::Resolved(candidate) => assert_eq!(candidate.app_id, 30),
        other => panic!("las eñes deben plegarse, no {other:?}"),
    }
}

#[test]
fn resuelve_un_nombre_mal_escrito_o_pegado() {
    let games = catalog(&[
        (10, "Dragon's Word: Awakening"),
        (20, "Hollow Knight"),
        (30, "Stardew Valley"),
    ]);
    // La frase literal de la persona usuaria.
    match matching::resolve("DragonsWord Awakening", &games) {
        GameMatch::Resolved(candidate) => assert_eq!(candidate.app_id, 10),
        other => panic!("debería resolverse, no {other:?}"),
    }
    // Erratas de tecleo.
    for query in ["holow knight", "Hollow Knigth", "stardew vally"] {
        assert!(
            matches!(matching::resolve(query, &games), GameMatch::Resolved(_)),
            "«{query}» debería resolverse"
        );
    }
}

#[test]
fn los_numeros_romanos_se_pliegan_a_cifras() {
    let games = catalog(&[(10, "Final Fantasy 7"), (20, "Hollow Knight")]);
    match matching::resolve("Final Fantasy VII", &games) {
        GameMatch::Resolved(candidate) => assert_eq!(candidate.app_id, 10),
        other => panic!("VII debería equivaler a 7, no {other:?}"),
    }
}

#[test]
fn un_nombre_parcial_prefiere_el_titulo_mas_ajustado() {
    let games = catalog(&[(10, "Portal"), (20, "Portal 2"), (30, "Hollow Knight")]);
    match matching::resolve("portal", &games) {
        GameMatch::Resolved(candidate) => assert_eq!(candidate.app_id, 10),
        other => panic!("«portal» debería resolverse a Portal, no {other:?}"),
    }
    match matching::resolve("portal 2", &games) {
        GameMatch::Resolved(candidate) => assert_eq!(candidate.app_id, 20),
        other => panic!("«portal 2» debería resolverse a Portal 2, no {other:?}"),
    }
}

#[test]
fn ante_la_duda_devuelve_candidatos_en_lugar_de_acertar() {
    let games = catalog(&[
        (10, "Dragon Age: Origins"),
        (20, "Dragon Age: Inquisition"),
        (30, "Hollow Knight"),
    ]);
    match matching::resolve("dragon age", &games) {
        GameMatch::Ambiguous(candidates) => {
            assert!(candidates.len() >= 2, "debería ofrecer las dos entregas");
            let ids = candidates.iter().map(|c| c.app_id).collect::<Vec<_>>();
            assert!(ids.contains(&10) && ids.contains(&20));
            assert!(!ids.contains(&30), "Hollow Knight no viene a cuento");
        }
        other => panic!("«dragon age» es ambiguo, no {other:?}"),
    }
}

#[test]
fn un_texto_sin_relacion_no_encuentra_nada() {
    let games = catalog(&[(10, "Hollow Knight"), (20, "Stardew Valley")]);
    for query in ["contabilidad trimestral", "zzzz", ""] {
        assert_eq!(
            matching::resolve(query, &games),
            GameMatch::NotFound,
            "«{query}» no debería encontrar nada"
        );
    }
}

#[test]
fn los_candidatos_estan_acotados_y_ordenados() {
    let titles = (0..12)
        .map(|index| (index + 1, format!("Serie Alfa {index}")))
        .collect::<Vec<_>>();
    let games = titles
        .iter()
        .map(|(app_id, title)| GameIndexEntry {
            app_id: *app_id,
            title: title.clone(),
        })
        .collect::<Vec<_>>();
    match matching::resolve("serie alfa", &games) {
        GameMatch::Ambiguous(candidates) => {
            assert!(candidates.len() <= matching::MAX_CANDIDATES);
            for pair in candidates.windows(2) {
                assert!(pair[0].score >= pair[1].score, "orden descendente");
            }
        }
        other => panic!("doce títulos casi iguales son ambiguos, no {other:?}"),
    }
}

// ---------------------------------------------------------------------------
// Esquemas de argumentos
// ---------------------------------------------------------------------------

#[test]
fn el_selector_de_juego_admite_el_identificador_el_nombre_o_los_dos() {
    assert!(by_id(10).validate().is_ok());
    assert!(by_name("Hollow Knight").validate().is_ok());
    // Los dos a la vez valen: es lo que manda cualquier modelo que tiene el
    // identificador y el título delante, y obligarle a quitar uno le hacía
    // adivinar. Que hablen del mismo juego se comprueba al resolver, contra la
    // biblioteca, que es donde se puede comprobar de verdad.
    assert!(
        GameSelector {
            app_id: Some(10),
            name: Some("Hollow Knight".to_string())
        }
        .validate()
        .is_ok()
    );
    assert!(
        GameSelector {
            app_id: None,
            name: None
        }
        .validate()
        .is_err()
    );
    assert!(
        GameSelector {
            app_id: Some(0),
            name: None
        }
        .validate()
        .is_err()
    );
    assert!(
        GameSelector {
            app_id: None,
            name: Some("   ".to_string())
        }
        .validate()
        .is_err()
    );
}

#[test]
fn cada_intencion_valida_su_esquema() {
    let game = by_id(10);
    let collection = EntitySelector {
        id: Some("col".to_string()),
        name: None,
    };

    // Casos válidos, uno por intención del catálogo.
    let valid: Vec<AgentIntent> = vec![
        AgentIntent::RegisterSession {
            game: game.clone(),
            minutes: 120,
            started_at: Some("2026-08-18T19:00:00Z".to_string()),
            progress: Some(40),
            note: "Capítulo 3".to_string(),
        },
        AgentIntent::MarkFinished {
            game: game.clone(),
            completed_on: Some("2026-08-18".to_string()),
            keep_playable: true,
            priority: Some(1),
        },
        AgentIntent::ChangeStatus {
            game: game.clone(),
            status_id: "playing".to_string(),
        },
        AgentIntent::AdjustPriority {
            game: game.clone(),
            priority: None,
            delta: Some(-2),
        },
        AgentIntent::Pin {
            game: game.clone(),
            pinned: true,
        },
        AgentIntent::Track {
            game: game.clone(),
            tracking: false,
        },
        AgentIntent::Rate {
            game: game.clone(),
            rating: Some(9),
        },
        AgentIntent::Annotate {
            game: game.clone(),
            note: "Buenísimo".to_string(),
            append: true,
        },
        AgentIntent::SetNextAction {
            game: game.clone(),
            action: Some("Terminar el pantano".to_string()),
        },
        AgentIntent::SetCheckpoint {
            game: game.clone(),
            checkpoint: Some("Antes del jefe".to_string()),
        },
        AgentIntent::CreateCollection {
            name: "Pendientes de 2026".to_string(),
            description: String::new(),
            color: "#5CAAC1".to_string(),
            icon: "folder".to_string(),
            games: vec![game.clone()],
        },
        AgentIntent::AddToCollection {
            collection: collection.clone(),
            games: vec![game.clone()],
        },
        AgentIntent::RemoveFromCollection {
            collection: collection.clone(),
            games: vec![game.clone()],
        },
        AgentIntent::CreateCuratedList {
            name: "Joyas".to_string(),
            description: String::new(),
            kind: "showcase".to_string(),
            accent: "violet".to_string(),
            icon: "list".to_string(),
            pinned: true,
        },
        AgentIntent::AddToCuratedList {
            list: collection.clone(),
            games: vec![game.clone()],
            note: "Imprescindible".to_string(),
            highlight: true,
        },
        AgentIntent::Plan {
            game: game.clone(),
            column_id: "next".to_string(),
            target_date: Some("2026-09-01".to_string()),
            estimated_minutes: Some(600),
        },
        AgentIntent::ScheduleReminder {
            game: game.clone(),
            due_at: "2026-09-01T10:00:00Z".to_string(),
            note: "Retomarlo".to_string(),
        },
        AgentIntent::Query {
            query: AgentQuery::Library {
                text: "hollow".to_string(),
                status_id: None,
                limit: Some(5),
            },
        },
    ];
    assert_eq!(valid.len(), 18, "el catálogo tiene dieciocho intenciones");
    for intent in &valid {
        intent
            .validate()
            .unwrap_or_else(|error| panic!("«{}» debería valer: {error}", intent.name()));
    }
    // Todos los nombres públicos son distintos.
    let mut names = valid.iter().map(AgentIntent::name).collect::<Vec<_>>();
    names.sort_unstable();
    names.dedup();
    assert_eq!(names.len(), 18);

    // Casos inválidos: uno por regla que importa.
    let invalid: Vec<(&str, AgentIntent)> = vec![
        (
            "sesión de cero minutos",
            AgentIntent::RegisterSession {
                game: game.clone(),
                minutes: 0,
                started_at: None,
                progress: None,
                note: String::new(),
            },
        ),
        (
            "sesión de más de un día",
            AgentIntent::RegisterSession {
                game: game.clone(),
                minutes: 1_441,
                started_at: None,
                progress: None,
                note: String::new(),
            },
        ),
        (
            "progreso por encima de cien",
            AgentIntent::RegisterSession {
                game: game.clone(),
                minutes: 30,
                started_at: None,
                progress: Some(101),
                note: String::new(),
            },
        ),
        (
            "fecha de inicio sin formato ISO",
            AgentIntent::RegisterSession {
                game: game.clone(),
                minutes: 30,
                started_at: Some("ayer por la tarde".to_string()),
                progress: None,
                note: String::new(),
            },
        ),
        (
            "fecha de finalización con formato de hora",
            AgentIntent::MarkFinished {
                game: game.clone(),
                completed_on: Some("2026-08-18T10:00:00Z".to_string()),
                keep_playable: false,
                priority: None,
            },
        ),
        (
            "prioridad fuera de rango",
            AgentIntent::MarkFinished {
                game: game.clone(),
                completed_on: None,
                keep_playable: false,
                priority: Some(9),
            },
        ),
        (
            "prioridad absoluta y relativa a la vez",
            AgentIntent::AdjustPriority {
                game: game.clone(),
                priority: Some(3),
                delta: Some(-1),
            },
        ),
        (
            "prioridad sin valor ninguno",
            AgentIntent::AdjustPriority {
                game: game.clone(),
                priority: None,
                delta: None,
            },
        ),
        (
            "ajuste de prioridad nulo",
            AgentIntent::AdjustPriority {
                game: game.clone(),
                priority: None,
                delta: Some(0),
            },
        ),
        (
            "valoración fuera de rango",
            AgentIntent::Rate {
                game: game.clone(),
                rating: Some(0),
            },
        ),
        (
            "nota interminable",
            AgentIntent::Annotate {
                game: game.clone(),
                note: "a".repeat(4_001),
                append: false,
            },
        ),
        (
            "próxima acción demasiado larga",
            AgentIntent::SetNextAction {
                game: game.clone(),
                action: Some("a".repeat(281)),
            },
        ),
        (
            "color que no es hexadecimal",
            AgentIntent::CreateCollection {
                name: "Mala".to_string(),
                description: String::new(),
                color: "azul".to_string(),
                icon: "folder".to_string(),
                games: Vec::new(),
            },
        ),
        (
            "colección sin nombre",
            AgentIntent::CreateCollection {
                name: "   ".to_string(),
                description: String::new(),
                color: "#5CAAC1".to_string(),
                icon: "folder".to_string(),
                games: Vec::new(),
            },
        ),
        (
            "añadir sin juegos",
            AgentIntent::AddToCollection {
                collection: collection.clone(),
                games: Vec::new(),
            },
        ),
        (
            "tipo de lista inexistente",
            AgentIntent::CreateCuratedList {
                name: "Rara".to_string(),
                description: String::new(),
                kind: "inventado".to_string(),
                accent: "cyan".to_string(),
                icon: "list".to_string(),
                pinned: false,
            },
        ),
        (
            "acento fuera de la paleta",
            AgentIntent::CreateCuratedList {
                name: "Rara".to_string(),
                description: String::new(),
                kind: "manual".to_string(),
                accent: "fucsia".to_string(),
                icon: "list".to_string(),
                pinned: false,
            },
        ),
        (
            "fecha objetivo mal formada",
            AgentIntent::Plan {
                game: game.clone(),
                column_id: "next".to_string(),
                target_date: Some("01/09/2026".to_string()),
                estimated_minutes: None,
            },
        ),
        (
            "aviso sin fecha válida",
            AgentIntent::ScheduleReminder {
                game: game.clone(),
                due_at: "mañana".to_string(),
                note: String::new(),
            },
        ),
        (
            "límite de consulta fuera de rango",
            AgentIntent::Query {
                query: AgentQuery::Library {
                    text: String::new(),
                    status_id: None,
                    limit: Some(0),
                },
            },
        ),
    ];
    for (label, intent) in &invalid {
        assert!(
            intent.validate().is_err(),
            "«{label}» debería rechazarse en la validación de esquema"
        );
    }
}

#[test]
fn el_catalogo_viaja_por_json_con_los_nombres_acordados() {
    let raw = r#"{
        "intent": "registrar_sesion",
        "game": { "name": "DragonsWord Awakening" },
        "minutes": 120,
        "progress": 40
    }"#;
    let intent: AgentIntent = serde_json::from_str(raw).expect("deserializar la intención");
    assert_eq!(intent.name(), "registrar_sesion");
    intent.validate().expect("esquema válido");

    let raw = r#"{
        "intent": "añadir_a_coleccion",
        "collection": { "name": "Pendientes" },
        "games": [{ "appId": 10 }]
    }"#;
    let intent: AgentIntent = serde_json::from_str(raw).expect("deserializar con eñe");
    assert_eq!(intent.name(), "añadir_a_coleccion");

    // Una intención fuera del catálogo no se deserializa.
    let raw = r#"{ "intent": "borrar_biblioteca" }"#;
    assert!(serde_json::from_str::<AgentIntent>(raw).is_err());
}

// ---------------------------------------------------------------------------
// Autenticación, ámbitos y frecuencia
// ---------------------------------------------------------------------------

#[test]
fn un_token_desconocido_o_revocado_se_rechaza_igual() {
    let mut fixture = fixture();
    add_game(&fixture.connection, 10, "Hollow Knight");
    let token = issue_client(&mut fixture.connection, "Hermes", &["biblioteca:escribir"]);
    let intent = AgentIntent::Pin {
        game: by_id(10),
        pinned: true,
    };

    // Token con formato correcto pero cliente inexistente.
    let forged = format!(
        "vdx_{}_{}",
        uuid::Uuid::new_v4(),
        crypto::to_hex(&crypto::random_bytes(32))
    );
    let error = bridge::dispatch(
        &mut fixture.connection,
        &fixture.limiter,
        &request(&forged, "fija Hollow Knight", intent.clone()),
    )
    .expect_err("no debería autenticar");
    assert_eq!(error.code, "agent_token");

    // Secreto equivocado para un cliente que sí existe.
    let mut wrong = token.clone();
    wrong.truncate(wrong.len() - 2);
    wrong.push_str("00");
    let error = bridge::dispatch(
        &mut fixture.connection,
        &fixture.limiter,
        &request(&wrong, "fija Hollow Knight", intent.clone()),
    )
    .expect_err("no debería autenticar");
    assert_eq!(error.code, "agent_token");

    // Cliente desactivado.
    let client_id = token::parse(&token).expect("token válido").client_id;
    clients::set_enabled(&mut fixture.connection, &client_id, false).expect("desactivar");
    let error = bridge::dispatch(
        &mut fixture.connection,
        &fixture.limiter,
        &request(&token, "fija Hollow Knight", intent.clone()),
    )
    .expect_err("un cliente desactivado no actúa");
    assert_eq!(error.code, "agent_token");

    // Reactivado, funciona.
    clients::set_enabled(&mut fixture.connection, &client_id, true).expect("reactivar");
    bridge::dispatch(
        &mut fixture.connection,
        &fixture.limiter,
        &request(&token, "fija Hollow Knight", intent),
    )
    .expect("con el cliente activo sí");
    assert_eq!(personal_field::<i64>(&fixture.connection, 10, "pinned"), 1);

    // Revocado: la auditoría sobrevive, el token deja de valer.
    clients::revoke(&mut fixture.connection, &client_id).expect("revocar");
    assert!(!audit_rows(&fixture.connection).is_empty());
    assert!(clients::authenticate(&fixture.connection, &token).is_err());
}

#[test]
fn el_ambito_se_comprueba_antes_de_tocar_nada() {
    let mut fixture = fixture();
    add_game(&fixture.connection, 10, "Hollow Knight");
    // Solo lectura: no puede escribir.
    let token = issue_client(&mut fixture.connection, "Hermes", &["biblioteca:leer"]);

    let error = bridge::dispatch(
        &mut fixture.connection,
        &fixture.limiter,
        &request(
            &token,
            "sube la prioridad",
            AgentIntent::AdjustPriority {
                game: by_id(10),
                priority: Some(5),
                delta: None,
            },
        ),
    )
    .expect_err("sin ámbito no se escribe");
    assert_eq!(error.code, "agent_scope");
    assert_eq!(
        personal_field::<i64>(&fixture.connection, 10, "priority"),
        0
    );

    // Un nombre inexistente ni siquiera se resuelve: primero manda el ámbito.
    let error = bridge::dispatch(
        &mut fixture.connection,
        &fixture.limiter,
        &request(
            &token,
            "sube la prioridad de un juego que no tengo",
            AgentIntent::AdjustPriority {
                game: by_name("Juego Inexistente Que No Está"),
                priority: Some(5),
                delta: None,
            },
        ),
    )
    .expect_err("sin ámbito no se resuelve");
    assert_eq!(error.code, "agent_scope");

    // La consulta sí entra en su ámbito.
    let outcome = bridge::dispatch(
        &mut fixture.connection,
        &fixture.limiter,
        &request(
            &token,
            "qué tengo",
            AgentIntent::Query {
                query: AgentQuery::Statuses,
            },
        ),
    )
    .expect("leer sí puede");
    assert!(matches!(outcome, AgentOutcome::Answer { .. }));

    // Cada ámbito cubre exactamente su familia de intenciones.
    let scopes = ScopeSet::from_values(&["colecciones:escribir"]).expect("ámbitos");
    assert!(scopes.contains(AgentScope::CollectionsWrite));
    assert!(scopes.require(AgentScope::ListsWrite).is_err());
    assert!(ScopeSet::from_values(&["inventado:escribir"]).is_err());
    // Un JSON ilegible no concede nada.
    assert!(ScopeSet::from_json("no es json").is_empty());
    assert!(ScopeSet::from_json("[\"*\"]").is_empty());
}

#[test]
fn el_limite_de_frecuencia_es_por_cliente_y_se_recupera() {
    let limiter = RateLimiter::new(3, 60_000);
    for step in 0..3 {
        limiter
            .check("cliente-a", 1_000 + step)
            .unwrap_or_else(|error| panic!("la petición {step} debería entrar: {error}"));
    }
    let error = limiter
        .check("cliente-a", 1_100)
        .expect_err("la cuarta se corta");
    assert_eq!(error.code, "agent_rate_limit");
    // Otro cliente tiene su propio cupo.
    limiter
        .check("cliente-b", 1_100)
        .expect("cupo independiente");
    // Al salir la primera de la ventana vuelve a haber sitio.
    limiter
        .check("cliente-a", 61_001)
        .expect("la ventana avanzó");
    assert_eq!(limiter.remaining("cliente-a", 61_001), 1);
    limiter.forget("cliente-a");
    assert_eq!(limiter.remaining("cliente-a", 61_001), 3);
}

#[test]
fn el_puente_aplica_el_limite_y_lo_deja_en_la_auditoria() {
    let mut fixture = fixture();
    add_game(&fixture.connection, 10, "Hollow Knight");
    let token = issue_client(&mut fixture.connection, "Hermes", &["biblioteca:leer"]);
    let limiter = RateLimiter::new(2, 60_000);
    let build = || {
        request(
            &token,
            "qué estados hay",
            AgentIntent::Query {
                query: AgentQuery::Statuses,
            },
        )
    };

    bridge::dispatch(&mut fixture.connection, &limiter, &build()).expect("primera");
    bridge::dispatch(&mut fixture.connection, &limiter, &build()).expect("segunda");
    let error = bridge::dispatch(&mut fixture.connection, &limiter, &build())
        .expect_err("tercera fuera de cupo");
    assert_eq!(error.code, "agent_rate_limit");

    let rows = audit_rows(&fixture.connection);
    assert_eq!(rows.len(), 3, "también se audita el rechazo");
    assert_eq!(rows[2].1, "failed");
    assert!(
        rows[2]
            .2
            .as_deref()
            .unwrap_or_default()
            .contains("agent_rate_limit")
    );
}

// ---------------------------------------------------------------------------
// Flujo completo con las frases reales
// ---------------------------------------------------------------------------

#[test]
fn registra_dos_horas_y_el_cuarenta_por_ciento() {
    let mut fixture = fixture();
    add_game(&fixture.connection, 10, "Dragon's Word: Awakening");
    add_game(&fixture.connection, 20, "Hollow Knight");
    let token = issue_client(&mut fixture.connection, "Hermes", &["sesiones:escribir"]);

    let outcome = bridge::dispatch(
        &mut fixture.connection,
        &fixture.limiter,
        &request(
            &token,
            "Acabo de estar 2 horas jugando a DragonsWord Awakening y voy por el 40 % de la historia",
            AgentIntent::RegisterSession {
                game: by_name("DragonsWord Awakening"),
                minutes: 120,
                started_at: None,
                progress: Some(40),
                note: String::new(),
            },
        ),
    )
    .expect("la sesión debería registrarse");

    let AgentOutcome::Applied {
        affected,
        undo_token,
        summary,
        ..
    } = outcome
    else {
        panic!("debería aplicarse");
    };
    assert_eq!(affected.len(), 1);
    assert_eq!(affected[0].app_id, 10);
    assert_eq!(affected[0].title, "Dragon's Word: Awakening");
    assert!(undo_token.is_some(), "toda escritura es reversible");
    assert!(summary.contains("40"));

    assert_eq!(
        personal_field::<i64>(&fixture.connection, 10, "progress"),
        40
    );
    let (minutes, before, after): (i64, i64, i64) = fixture
        .connection
        .query_row(
            "SELECT CAST((julianday(ended_at) - julianday(started_at)) * 1440 + 0.5 AS INTEGER),
                    progress_before, progress_after
               FROM game_sessions WHERE app_id = 10",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .expect("leer la sesión");
    assert_eq!(minutes, 120);
    assert_eq!(before, 0);
    assert_eq!(after, 40);
    // El juego queda con fecha de inicio.
    assert!(personal_field::<Option<String>>(&fixture.connection, 10, "started_at").is_some());
}

#[test]
fn marca_terminado_manteniendolo_jugable_y_baja_la_prioridad() {
    let mut fixture = fixture();
    add_game(&fixture.connection, 10, "Stardew Valley");
    fixture
        .connection
        .execute(
            "UPDATE game_personal SET status_id = 'playing', priority = 4 WHERE app_id = 10",
            [],
        )
        .expect("preparar el estado inicial");
    let token = issue_client(&mut fixture.connection, "Hermes", &["biblioteca:escribir"]);

    bridge::dispatch(
        &mut fixture.connection,
        &fixture.limiter,
        &request(
            &token,
            "Stardew Valley ya me lo he pasado pero seguiré jugando: bájale la prioridad",
            AgentIntent::MarkFinished {
                game: by_name("stardew valley"),
                completed_on: Some("2026-08-18".to_string()),
                keep_playable: true,
                priority: Some(1),
            },
        ),
    )
    .expect("debería aplicarse");

    assert_eq!(
        personal_field::<String>(&fixture.connection, 10, "status_id"),
        "playing",
        "sigue siendo jugable"
    );
    assert_eq!(
        personal_field::<i64>(&fixture.connection, 10, "progress"),
        100
    );
    assert_eq!(
        personal_field::<i64>(&fixture.connection, 10, "priority"),
        1
    );
    assert_eq!(
        personal_field::<Option<String>>(&fixture.connection, 10, "completed_at").as_deref(),
        Some("2026-08-18")
    );

    // Sin «keepPlayable» sí cambia de estado.
    bridge::dispatch(
        &mut fixture.connection,
        &fixture.limiter,
        &request(
            &token,
            "márcalo como completado del todo",
            AgentIntent::MarkFinished {
                game: by_id(10),
                completed_on: None,
                keep_playable: false,
                priority: None,
            },
        ),
    )
    .expect("debería aplicarse");
    assert_eq!(
        personal_field::<String>(&fixture.connection, 10, "status_id"),
        "completed"
    );
}

#[test]
fn ante_un_nombre_ambiguo_pregunta_y_no_toca_nada() {
    let mut fixture = fixture();
    add_game(&fixture.connection, 10, "Dragon Age: Origins");
    add_game(&fixture.connection, 20, "Dragon Age: Inquisition");
    let token = issue_client(&mut fixture.connection, "Hermes", &["biblioteca:escribir"]);

    let outcome = bridge::dispatch(
        &mut fixture.connection,
        &fixture.limiter,
        &request(
            &token,
            "sube la prioridad de Dragon Age",
            AgentIntent::AdjustPriority {
                game: by_name("Dragon Age"),
                priority: Some(5),
                delta: None,
            },
        ),
    )
    .expect("preguntar no es un error");

    let AgentOutcome::NeedsGameChoice {
        candidates, query, ..
    } = outcome
    else {
        panic!("debería pedir que se elija");
    };
    assert_eq!(query, "Dragon Age");
    assert!(candidates.len() >= 2);
    assert_eq!(
        personal_field::<i64>(&fixture.connection, 10, "priority"),
        0
    );
    assert_eq!(
        personal_field::<i64>(&fixture.connection, 20, "priority"),
        0
    );

    // Con el AppID elegido sí se aplica.
    let chosen = candidates[0].app_id;
    bridge::dispatch(
        &mut fixture.connection,
        &fixture.limiter,
        &request(
            &token,
            "el de Origins",
            AgentIntent::AdjustPriority {
                game: by_id(chosen),
                priority: Some(5),
                delta: None,
            },
        ),
    )
    .expect("con el AppID no hay duda");
    assert_eq!(
        personal_field::<i64>(&fixture.connection, chosen, "priority"),
        5
    );
}

// ---------------------------------------------------------------------------
// Confirmación humana
// ---------------------------------------------------------------------------

#[test]
fn un_cambio_masivo_espera_a_una_persona() {
    let mut fixture = fixture();
    for app_id in 1..=7u32 {
        add_game(&fixture.connection, app_id, &format!("Juego {app_id}"));
    }
    fixture
        .connection
        .execute(
            "INSERT INTO collections(id, name, description, color, icon, kind, match_mode, position)
             VALUES ('col', 'Pendientes', '', '#5CAAC1', 'folder', 'manual', 'all', 0)",
            [],
        )
        .expect("crear colección");
    let token = issue_client(&mut fixture.connection, "Hermes", &["colecciones:escribir"]);

    let games = (1..=7u32).map(by_id).collect::<Vec<_>>();
    let outcome = bridge::dispatch(
        &mut fixture.connection,
        &fixture.limiter,
        &request(
            &token,
            "mete estos siete en Pendientes",
            AgentIntent::AddToCollection {
                collection: EntitySelector {
                    id: None,
                    name: Some("Pendientes".to_string()),
                },
                games,
            },
        ),
    )
    .expect("debería quedar pendiente");

    let AgentOutcome::PendingConfirmation {
        audit_id,
        reason,
        affected,
        ..
    } = outcome
    else {
        panic!("siete juegos superan el umbral y deben confirmarse");
    };
    assert!(reason.contains('7'));
    assert_eq!(affected.len(), 7);
    let members: i64 = fixture
        .connection
        .query_row("SELECT COUNT(*) FROM collection_games", [], |row| {
            row.get(0)
        })
        .expect("contar");
    assert_eq!(members, 0, "nada se aplica antes de confirmar");

    let confirmed = bridge::confirm(&mut fixture.connection, &audit_id, true).expect("confirmar");
    assert!(matches!(confirmed, AgentOutcome::Applied { .. }));
    let members: i64 = fixture
        .connection
        .query_row("SELECT COUNT(*) FROM collection_games", [], |row| {
            row.get(0)
        })
        .expect("contar");
    assert_eq!(members, 7);

    // La fila queda cerrada: no se puede confirmar dos veces.
    assert!(bridge::confirm(&mut fixture.connection, &audit_id, true).is_err());
    let rows = audit_rows(&fixture.connection);
    assert_eq!(rows.len(), 1, "una petición, una fila");
    assert_eq!(rows[0].1, "applied");
}

#[test]
fn quitar_de_una_coleccion_siempre_se_confirma_y_puede_rechazarse() {
    let mut fixture = fixture();
    add_game(&fixture.connection, 10, "Hollow Knight");
    fixture
        .connection
        .execute(
            "INSERT INTO collections(id, name, description, color, icon, kind, match_mode, position)
             VALUES ('col', 'Pendientes', '', '#5CAAC1', 'folder', 'manual', 'all', 0)",
            [],
        )
        .expect("crear colección");
    fixture
        .connection
        .execute(
            "INSERT INTO collection_games(collection_id, app_id, position) VALUES ('col', 10, 0)",
            [],
        )
        .expect("añadir juego");
    let token = issue_client(&mut fixture.connection, "Hermes", &["colecciones:escribir"]);

    let outcome = bridge::dispatch(
        &mut fixture.connection,
        &fixture.limiter,
        &request(
            &token,
            "saca Hollow Knight de Pendientes",
            AgentIntent::RemoveFromCollection {
                collection: EntitySelector {
                    id: Some("col".to_string()),
                    name: None,
                },
                games: vec![by_id(10)],
            },
        ),
    )
    .expect("debería quedar pendiente");
    let AgentOutcome::PendingConfirmation { audit_id, .. } = outcome else {
        panic!("quitar siempre se confirma, aunque sea un solo juego");
    };

    let rejected = bridge::confirm(&mut fixture.connection, &audit_id, false).expect("rechazar");
    assert!(matches!(rejected, AgentOutcome::Rejected { .. }));
    let members: i64 = fixture
        .connection
        .query_row("SELECT COUNT(*) FROM collection_games", [], |row| {
            row.get(0)
        })
        .expect("contar");
    assert_eq!(members, 1, "el rechazo deja la colección intacta");
    assert_eq!(audit_rows(&fixture.connection)[0].1, "rejected");
}

// ---------------------------------------------------------------------------
// Deshacer
// ---------------------------------------------------------------------------

fn undo_token_of(outcome: &AgentOutcome) -> String {
    match outcome {
        AgentOutcome::Applied { undo_token, .. } => undo_token
            .clone()
            .expect("toda escritura devuelve un token"),
        other => panic!("se esperaba una acción aplicada, no {other:?}"),
    }
}

#[test]
fn deshacer_restaura_la_ficha_personal() {
    let mut fixture = fixture();
    add_game(&fixture.connection, 10, "Hollow Knight");
    fixture
        .connection
        .execute(
            "UPDATE game_personal SET priority = 2, notes = 'nota previa' WHERE app_id = 10",
            [],
        )
        .expect("estado inicial");
    let token = issue_client(&mut fixture.connection, "Hermes", &["biblioteca:escribir"]);

    let outcome = bridge::dispatch(
        &mut fixture.connection,
        &fixture.limiter,
        &request(
            &token,
            "anota que va por el pantano",
            AgentIntent::Annotate {
                game: by_id(10),
                note: "Va por el pantano".to_string(),
                append: true,
            },
        ),
    )
    .expect("anotar");
    let undo = undo_token_of(&outcome);
    let notes = personal_field::<Option<String>>(&fixture.connection, 10, "notes");
    assert_eq!(notes.as_deref(), Some("nota previa\nVa por el pantano"));

    let undone = bridge::undo(&mut fixture.connection, &undo, &Requester::Human).expect("deshacer");
    assert!(matches!(undone, AgentOutcome::Undone { restored: 1, .. }));
    assert_eq!(
        personal_field::<Option<String>>(&fixture.connection, 10, "notes").as_deref(),
        Some("nota previa")
    );
    assert_eq!(
        personal_field::<i64>(&fixture.connection, 10, "priority"),
        2
    );
    assert_eq!(audit_rows(&fixture.connection)[0].1, "undone");

    // El token es de un solo uso.
    let error = bridge::undo(&mut fixture.connection, &undo, &Requester::Human)
        .expect_err("no se deshace dos veces");
    assert_eq!(error.code, "not_found");
}

#[test]
fn deshacer_borra_la_sesion_y_devuelve_el_progreso() {
    let mut fixture = fixture();
    add_game(&fixture.connection, 10, "Hollow Knight");
    let token = issue_client(&mut fixture.connection, "Hermes", &["sesiones:escribir"]);

    let outcome = bridge::dispatch(
        &mut fixture.connection,
        &fixture.limiter,
        &request(
            &token,
            "he jugado hora y media, voy por el 55 %",
            AgentIntent::RegisterSession {
                game: by_id(10),
                minutes: 90,
                started_at: None,
                progress: Some(55),
                note: "Ciudad de las lágrimas".to_string(),
            },
        ),
    )
    .expect("registrar");
    let undo = undo_token_of(&outcome);
    assert_eq!(
        personal_field::<i64>(&fixture.connection, 10, "progress"),
        55
    );

    bridge::undo(&mut fixture.connection, &undo, &Requester::Human).expect("deshacer");
    let sessions: i64 = fixture
        .connection
        .query_row("SELECT COUNT(*) FROM game_sessions", [], |row| row.get(0))
        .expect("contar sesiones");
    assert_eq!(sessions, 0);
    assert_eq!(
        personal_field::<i64>(&fixture.connection, 10, "progress"),
        0
    );
    assert!(personal_field::<Option<String>>(&fixture.connection, 10, "started_at").is_none());
    let activity: i64 = fixture
        .connection
        .query_row("SELECT COUNT(*) FROM activity", [], |row| row.get(0))
        .expect("contar actividad");
    assert_eq!(activity, 0, "la actividad del agente también se retira");
}

#[test]
fn deshacer_detecta_un_recibo_caducado() {
    let mut fixture = fixture();
    add_game(&fixture.connection, 10, "Hollow Knight");
    let token = issue_client(&mut fixture.connection, "Hermes", &["biblioteca:escribir"]);

    let outcome = bridge::dispatch(
        &mut fixture.connection,
        &fixture.limiter,
        &request(
            &token,
            "ponle un 9",
            AgentIntent::Rate {
                game: by_id(10),
                rating: Some(9),
            },
        ),
    )
    .expect("valorar");
    let undo = undo_token_of(&outcome);

    // La persona usuaria edita el juego después.
    fixture
        .connection
        .execute("UPDATE game_personal SET rating = 4 WHERE app_id = 10", [])
        .expect("edición posterior");

    let error = bridge::undo(&mut fixture.connection, &undo, &Requester::Human)
        .expect_err("el recibo ya no vale");
    assert_eq!(error.code, "agent_stale");
    assert_eq!(
        personal_field::<i64>(&fixture.connection, 10, "rating"),
        4,
        "la edición posterior no se pisa"
    );
}

#[test]
fn deshacer_retira_una_coleccion_creada_por_el_agente() {
    let mut fixture = fixture();
    add_game(&fixture.connection, 10, "Hollow Knight");
    let token = issue_client(&mut fixture.connection, "Hermes", &["colecciones:escribir"]);

    let outcome = bridge::dispatch(
        &mut fixture.connection,
        &fixture.limiter,
        &request(
            &token,
            "crea una colección de metroidvanias",
            AgentIntent::CreateCollection {
                name: "Metroidvanias".to_string(),
                description: "Los que más me gustan".to_string(),
                color: "#5CAAC1".to_string(),
                icon: "folder".to_string(),
                games: vec![by_id(10)],
            },
        ),
    )
    .expect("crear colección");
    let undo = undo_token_of(&outcome);
    let total: i64 = fixture
        .connection
        .query_row("SELECT COUNT(*) FROM collections", [], |row| row.get(0))
        .expect("contar");
    assert_eq!(total, 1);

    bridge::undo(&mut fixture.connection, &undo, &Requester::Human).expect("deshacer");
    let total: i64 = fixture
        .connection
        .query_row("SELECT COUNT(*) FROM collections", [], |row| row.get(0))
        .expect("contar");
    assert_eq!(total, 0);
}

#[test]
fn un_agente_no_puede_deshacer_lo_de_otro() {
    let mut fixture = fixture();
    add_game(&fixture.connection, 10, "Hollow Knight");
    let first = issue_client(&mut fixture.connection, "Hermes", &["biblioteca:escribir"]);
    let second = issue_client(&mut fixture.connection, "Otro", &["biblioteca:escribir"]);

    let outcome = bridge::dispatch(
        &mut fixture.connection,
        &fixture.limiter,
        &request(
            &first,
            "fíjalo",
            AgentIntent::Pin {
                game: by_id(10),
                pinned: true,
            },
        ),
    )
    .expect("fijar");
    let undo = undo_token_of(&outcome);

    let error = bridge::undo_as_client(&mut fixture.connection, &fixture.limiter, &second, &undo)
        .expect_err("no es suyo");
    assert_eq!(error.code, "agent_scope");
    assert_eq!(personal_field::<i64>(&fixture.connection, 10, "pinned"), 1);

    bridge::undo_as_client(&mut fixture.connection, &fixture.limiter, &first, &undo)
        .expect("el dueño sí");
    assert_eq!(personal_field::<i64>(&fixture.connection, 10, "pinned"), 0);
}

// ---------------------------------------------------------------------------
// Integridad de la auditoría
// ---------------------------------------------------------------------------

#[test]
fn cada_peticion_deja_exactamente_una_fila_con_todo_su_contexto() {
    let mut fixture = fixture();
    add_game(&fixture.connection, 10, "Hollow Knight");
    let token = issue_client(
        &mut fixture.connection,
        "Hermes",
        &["biblioteca:escribir", "biblioteca:leer"],
    );
    let frase = "ponle un 8 a Hollow Knight";

    bridge::dispatch(
        &mut fixture.connection,
        &fixture.limiter,
        &request(
            &token,
            frase,
            AgentIntent::Rate {
                game: by_name("Hollow Knight"),
                rating: Some(8),
            },
        ),
    )
    .expect("valorar");

    // Un fallo también deja su fila.
    bridge::dispatch(
        &mut fixture.connection,
        &fixture.limiter,
        &request(
            &token,
            "ponle un 40",
            AgentIntent::Rate {
                game: by_id(10),
                rating: Some(40),
            },
        ),
    )
    .expect_err("valoración fuera de rango");

    let entries = audit::list(&fixture.connection, 50).expect("listar auditoría");
    assert_eq!(entries.len(), 2);

    let applied = entries
        .iter()
        .find(|entry| entry.result == AuditResult::Applied)
        .expect("hay una aplicada");
    assert_eq!(applied.intent, "valorar");
    assert_eq!(applied.utterance, frase);
    assert_eq!(
        applied.affected,
        vec![AffectedGame {
            app_id: 10,
            title: "Hollow Knight".to_string()
        }]
    );
    assert!(applied.undoable);
    assert!(applied.client_name.as_deref() == Some("Hermes"));
    assert_eq!(
        applied
            .arguments
            .get("intent")
            .and_then(|value| value.as_str()),
        Some("valorar")
    );
    // Los argumentos guardados llevan el juego ya resuelto.
    assert_eq!(
        applied
            .arguments
            .get("game")
            .and_then(|game| game.get("appId"))
            .and_then(|value| value.as_u64()),
        Some(10)
    );

    let failed = entries
        .iter()
        .find(|entry| entry.result == AuditResult::Failed)
        .expect("hay una fallida");
    assert!(!failed.undoable);
    assert!(
        failed
            .error_message
            .as_deref()
            .unwrap_or_default()
            .contains("validation")
    );

    // El token nunca aparece en el registro.
    let raw: String = fixture
        .connection
        .query_row(
            "SELECT group_concat(arguments_json || utterance || COALESCE(error_message, ''))
               FROM agent_audit_log",
            [],
            |row| row.get(0),
        )
        .expect("volcado del registro");
    assert!(!raw.contains(&token));
    assert!(!raw.contains("vdx_"));
}

#[test]
fn la_consulta_de_biblioteca_responde_sin_escribir() {
    let mut fixture = fixture();
    add_game(&fixture.connection, 10, "Hollow Knight");
    add_game(&fixture.connection, 20, "Stardew Valley");
    let token = issue_client(&mut fixture.connection, "Hermes", &["biblioteca:leer"]);

    let outcome = bridge::dispatch(
        &mut fixture.connection,
        &fixture.limiter,
        &request(
            &token,
            "qué tengo de hollow",
            AgentIntent::Query {
                query: AgentQuery::Library {
                    text: "hollow".to_string(),
                    status_id: None,
                    limit: Some(5),
                },
            },
        ),
    )
    .expect("consultar");

    let AgentOutcome::Answer { data, .. } = outcome else {
        panic!("una consulta responde");
    };
    let games = data
        .get("games")
        .and_then(|value| value.as_array())
        .expect("lista");
    assert_eq!(games.len(), 1);
    assert_eq!(
        games[0].get("appId").and_then(|value| value.as_u64()),
        Some(10)
    );

    // Nada cambió en la biblioteca.
    assert_eq!(
        personal_field::<i64>(&fixture.connection, 10, "progress"),
        0
    );
    let entry = &audit::list(&fixture.connection, 5).expect("auditoría")[0];
    assert_eq!(entry.intent, "consultar");
    assert!(!entry.undoable, "una lectura no se deshace");
}

#[test]
fn la_resolucion_no_escribe_y_distingue_el_juego_que_no_existe() {
    let fixture = fixture();
    add_game(&fixture.connection, 10, "Hollow Knight");

    let resolved = resolve(
        &fixture.connection,
        &AgentIntent::Pin {
            game: by_name("hollow knight"),
            pinned: true,
        },
    )
    .expect("resolver");
    match resolved {
        Resolved::Ready(resolution) => {
            assert_eq!(resolution.affected[0].app_id, 10);
            assert_eq!(
                resolution.intent,
                AgentIntent::Pin {
                    game: by_id(10),
                    pinned: true
                }
            );
        }
        other => panic!("debería resolverse, no {other:?}"),
    }

    let error = resolve(
        &fixture.connection,
        &AgentIntent::Pin {
            game: by_id(999),
            pinned: true,
        },
    )
    .expect_err("ese AppID no está");
    assert_eq!(error.code, "not_found");
    assert_eq!(personal_field::<i64>(&fixture.connection, 10, "pinned"), 0);
}

#[test]
fn el_planificador_y_los_avisos_tambien_se_deshacen() {
    let mut fixture = fixture();
    add_game(&fixture.connection, 10, "Hollow Knight");
    let token = issue_client(
        &mut fixture.connection,
        "Hermes",
        &["planificador:escribir", "avisos:escribir"],
    );

    let planned = bridge::dispatch(
        &mut fixture.connection,
        &fixture.limiter,
        &request(
            &token,
            "ponlo en «A continuación»",
            AgentIntent::Plan {
                game: by_id(10),
                column_id: "next".to_string(),
                target_date: Some("2026-09-01".to_string()),
                estimated_minutes: Some(600),
            },
        ),
    )
    .expect("planificar");
    let column: String = fixture
        .connection
        .query_row(
            "SELECT column_id FROM planner_items WHERE app_id = 10",
            [],
            |row| row.get(0),
        )
        .expect("leer planificador");
    assert_eq!(column, "next");

    let reminded = bridge::dispatch(
        &mut fixture.connection,
        &fixture.limiter,
        &request(
            &token,
            "recuérdamelo el 1 de septiembre",
            AgentIntent::ScheduleReminder {
                game: by_id(10),
                due_at: "2026-09-01T10:00:00Z".to_string(),
                note: "Retomarlo".to_string(),
            },
        ),
    )
    .expect("programar aviso");

    bridge::undo(
        &mut fixture.connection,
        &undo_token_of(&reminded),
        &Requester::Human,
    )
    .expect("deshacer el aviso");
    let reminders: i64 = fixture
        .connection
        .query_row("SELECT COUNT(*) FROM game_reminders", [], |row| row.get(0))
        .expect("contar avisos");
    assert_eq!(reminders, 0);

    bridge::undo(
        &mut fixture.connection,
        &undo_token_of(&planned),
        &Requester::Human,
    )
    .expect("deshacer la planificación");
    let items: i64 = fixture
        .connection
        .query_row("SELECT COUNT(*) FROM planner_items", [], |row| row.get(0))
        .expect("contar elementos");
    assert_eq!(items, 0);
}

#[test]
fn las_listas_curadas_se_crean_se_llenan_y_se_deshacen() {
    let mut fixture = fixture();
    add_game(&fixture.connection, 10, "Hollow Knight");
    let token = issue_client(&mut fixture.connection, "Hermes", &["listas:escribir"]);

    bridge::dispatch(
        &mut fixture.connection,
        &fixture.limiter,
        &request(
            &token,
            "crea una lista de joyas",
            AgentIntent::CreateCuratedList {
                name: "Joyas".to_string(),
                description: String::new(),
                kind: "showcase".to_string(),
                accent: "violet".to_string(),
                icon: "list".to_string(),
                pinned: true,
            },
        ),
    )
    .expect("crear lista");

    let added = bridge::dispatch(
        &mut fixture.connection,
        &fixture.limiter,
        &request(
            &token,
            "mete Hollow Knight en Joyas",
            AgentIntent::AddToCuratedList {
                list: EntitySelector {
                    id: None,
                    name: Some("joyas".to_string()),
                },
                games: vec![by_name("Hollow Knight")],
                note: "Imprescindible".to_string(),
                highlight: true,
            },
        ),
    )
    .expect("añadir a la lista");
    let items: i64 = fixture
        .connection
        .query_row("SELECT COUNT(*) FROM curated_list_items", [], |row| {
            row.get(0)
        })
        .expect("contar");
    assert_eq!(items, 1);

    // Repetir la misma adición no duplica y avisa.
    let error = bridge::dispatch(
        &mut fixture.connection,
        &fixture.limiter,
        &request(
            &token,
            "mételo otra vez",
            AgentIntent::AddToCuratedList {
                list: EntitySelector {
                    id: None,
                    name: Some("Joyas".to_string()),
                },
                games: vec![by_id(10)],
                note: String::new(),
                highlight: false,
            },
        ),
    )
    .expect_err("ya estaba");
    assert_eq!(error.code, "validation");

    bridge::undo(
        &mut fixture.connection,
        &undo_token_of(&added),
        &Requester::Human,
    )
    .expect("deshacer la adición");
    let items: i64 = fixture
        .connection
        .query_row("SELECT COUNT(*) FROM curated_list_items", [], |row| {
            row.get(0)
        })
        .expect("contar");
    assert_eq!(items, 0);
}

#[test]
fn una_coleccion_inteligente_no_admite_cambios_del_agente() {
    let mut fixture = fixture();
    add_game(&fixture.connection, 10, "Hollow Knight");
    fixture
        .connection
        .execute(
            "INSERT INTO collections(id, name, description, color, icon, kind, match_mode, position)
             VALUES ('smart', 'Automática', '', '#5CAAC1', 'folder', 'smart', 'all', 0)",
            [],
        )
        .expect("crear colección inteligente");
    let token = issue_client(&mut fixture.connection, "Hermes", &["colecciones:escribir"]);

    let error = bridge::dispatch(
        &mut fixture.connection,
        &fixture.limiter,
        &request(
            &token,
            "mete Hollow Knight en Automática",
            AgentIntent::AddToCollection {
                collection: EntitySelector {
                    id: Some("smart".to_string()),
                    name: None,
                },
                games: vec![by_id(10)],
            },
        ),
    )
    .expect_err("una colección inteligente se calcula sola");
    assert_eq!(error.code, "validation");
}

// ---------------------------------------------------------------------------
// Cableado propuesto para commands.rs
// ---------------------------------------------------------------------------

/// Reproduce el cuerpo exacto de cada comando Tauri del informe de integración,
/// sin el atributo `#[tauri::command]`, para que ese diff no contenga código que
/// no compile contra la API real del puente.
mod wiring_check {
    use crate::agent::{
        self, AgentAuditEntry, AgentClientSummary, AgentOutcome, AgentRequest, IssuedAgentClient,
        NewAgentClient, Requester,
    };
    use crate::db::Database;
    use crate::error::AppResult;

    fn agent_dispatch(database: Database, request: AgentRequest) -> AppResult<AgentOutcome> {
        agent::bridge::dispatch(&mut database.open()?, agent::ratelimit::shared(), &request)
    }

    fn agent_confirm(
        database: Database,
        audit_id: String,
        approve: bool,
    ) -> AppResult<AgentOutcome> {
        agent::bridge::confirm(&mut database.open()?, &audit_id, approve)
    }

    fn agent_undo(database: Database, undo_token: String) -> AppResult<AgentOutcome> {
        agent::bridge::undo(&mut database.open()?, &undo_token, &Requester::Human)
    }

    fn agent_undo_as_client(
        database: Database,
        token: String,
        undo_token: String,
    ) -> AppResult<AgentOutcome> {
        agent::bridge::undo_as_client(
            &mut database.open()?,
            agent::ratelimit::shared(),
            &token,
            &undo_token,
        )
    }

    fn issue_agent_client(
        database: Database,
        input: NewAgentClient,
    ) -> AppResult<IssuedAgentClient> {
        agent::clients::issue(&mut database.open()?, &input, agent::TokenPolicy::default())
    }

    fn rotate_agent_token(database: Database, client_id: String) -> AppResult<IssuedAgentClient> {
        agent::clients::rotate(
            &mut database.open()?,
            &client_id,
            agent::TokenPolicy::default(),
        )
    }

    fn set_agent_client_scopes(
        database: Database,
        client_id: String,
        scopes: Vec<String>,
    ) -> AppResult<AgentClientSummary> {
        agent::clients::set_scopes(&mut database.open()?, &client_id, &scopes)
    }

    fn set_agent_client_enabled(
        database: Database,
        client_id: String,
        enabled: bool,
    ) -> AppResult<AgentClientSummary> {
        agent::clients::set_enabled(&mut database.open()?, &client_id, enabled)
    }

    fn revoke_agent_client(database: Database, client_id: String) -> AppResult<()> {
        agent::clients::revoke(&mut database.open()?, &client_id)?;
        agent::ratelimit::shared().forget(&client_id);
        Ok(())
    }

    fn list_agent_clients(database: Database) -> AppResult<Vec<AgentClientSummary>> {
        agent::clients::list(&database.open()?)
    }

    fn list_agent_audit(database: Database, limit: u32) -> AppResult<Vec<AgentAuditEntry>> {
        agent::audit::list(&database.open()?, limit)
    }

    #[test]
    fn el_cableado_propuesto_compila() {
        // Las funciones existen y tienen las firmas que exige `database_read`.
        fn assert_send_static<T: Send + 'static>() {}
        assert_send_static::<AgentRequest>();
        assert_send_static::<AgentOutcome>();
        assert_send_static::<NewAgentClient>();
        assert_send_static::<IssuedAgentClient>();
        assert_send_static::<AgentClientSummary>();
        assert_send_static::<AgentAuditEntry>();
        let _ = (
            agent_dispatch as fn(_, _) -> _,
            agent_confirm as fn(_, _, _) -> _,
            agent_undo as fn(_, _) -> _,
            agent_undo_as_client as fn(_, _, _) -> _,
            issue_agent_client as fn(_, _) -> _,
            rotate_agent_token as fn(_, _) -> _,
            set_agent_client_scopes as fn(_, _, _) -> _,
            set_agent_client_enabled as fn(_, _, _) -> _,
            revoke_agent_client as fn(_, _) -> _,
            list_agent_clients as fn(_) -> _,
            list_agent_audit as fn(_, _) -> _,
        );
    }
}

// ---------------------------------------------------------------------------
// Identificador y nombre a la vez
// ---------------------------------------------------------------------------

#[test]
fn el_identificador_manda_y_el_nombre_corrobora() {
    // Un modelo que tiene las dos cosas manda las dos. Antes eso era un error
    // de validación y la orden se perdía; ahora se aplica, porque el AppID es
    // inequívoco y el nombre sólo sirve para confirmar que hablan de lo mismo.
    let mut fixture = fixture();
    add_game(&fixture.connection, 367520, "Hollow Knight");
    let token = issue_client(
        &mut fixture.connection,
        "Agente",
        &["biblioteca:leer", "biblioteca:escribir"],
    );

    let outcome = bridge::dispatch(
        &mut fixture.connection,
        &fixture.limiter,
        &AgentRequest {
            token: token.clone(),
            utterance: String::new(),
            intent: AgentIntent::Pin {
                game: GameSelector {
                    app_id: Some(367520),
                    name: Some("Hollow Knight".to_string()),
                },
                pinned: true,
            },
        },
    )
    .expect("aplicar");
    assert!(matches!(outcome, AgentOutcome::Applied { .. }), "{outcome:?}");
}

#[test]
fn un_identificador_que_no_es_de_ese_juego_se_rechaza() {
    // Es el caso que justifica comprobar: si el modelo se equivoca de AppID,
    // aplicar el cambio al juego equivocado sería el peor final posible.
    let mut fixture = fixture();
    add_game(&fixture.connection, 367520, "Hollow Knight");
    add_game(&fixture.connection, 570, "Dota 2");
    let token = issue_client(
        &mut fixture.connection,
        "Agente",
        &["biblioteca:leer", "biblioteca:escribir"],
    );

    let error = bridge::dispatch(
        &mut fixture.connection,
        &fixture.limiter,
        &AgentRequest {
            token,
            utterance: String::new(),
            intent: AgentIntent::Pin {
                game: GameSelector {
                    app_id: Some(570),
                    name: Some("Hollow Knight".to_string()),
                },
                pinned: true,
            },
        },
    )
    .expect_err("rechazar");
    assert_eq!(error.code, "validation");
    assert!(error.message.contains("Dota 2"), "{}", error.message);

    let fijado: i64 = fixture
        .connection
        .query_row("SELECT pinned FROM game_personal WHERE app_id = 570", [], |row| row.get(0))
        .expect("leer");
    assert_eq!(fijado, 0, "no se toca el juego equivocado");
}

// ---------------------------------------------------------------------------
// Recuentos que coinciden con la biblioteca
// ---------------------------------------------------------------------------

#[test]
fn una_muestra_dice_cuantos_hay_de_verdad() {
    // Es el fallo que se vio en Telegram: el modelo pidió la biblioteca con un
    // límite, contó las filas que le llegaron y contestó «tienes 20 juegos en
    // Backlog» cuando había 215. La respuesta tiene que llevar el total, y
    // decir que lo que se enseña es una muestra.
    let mut fixture = fixture();
    for i in 0..30_u32 {
        add_game(&fixture.connection, 1000 + i, &format!("Juego {i}"));
    }
    let token = issue_client(&mut fixture.connection, "Agente", &["biblioteca:leer"]);

    let outcome = bridge::dispatch(
        &mut fixture.connection,
        &fixture.limiter,
        &AgentRequest {
            token,
            utterance: String::new(),
            intent: AgentIntent::Query {
                query: AgentQuery::Library {
                    text: String::new(),
                    status_id: None,
                    limit: Some(5),
                },
            },
        },
    )
    .expect("consultar");

    let AgentOutcome::Answer { data, .. } = outcome else {
        panic!("una consulta contesta con datos: {outcome:?}");
    };
    assert_eq!(data["shown"], serde_json::json!(5), "{data}");
    assert_eq!(data["matched"], serde_json::json!(30), "{data}");
    assert_eq!(data["truncated"], serde_json::json!(true), "{data}");
    assert_eq!(data["games"].as_array().map(Vec::len), Some(5));
}

#[test]
fn cuando_cabe_entero_no_se_dice_que_es_una_muestra() {
    let mut fixture = fixture();
    add_game(&fixture.connection, 2000, "Único");
    let token = issue_client(&mut fixture.connection, "Agente", &["biblioteca:leer"]);

    let outcome = bridge::dispatch(
        &mut fixture.connection,
        &fixture.limiter,
        &AgentRequest {
            token,
            utterance: String::new(),
            intent: AgentIntent::Query {
                query: AgentQuery::Library {
                    text: String::new(),
                    status_id: None,
                    limit: Some(50),
                },
            },
        },
    )
    .expect("consultar");

    let AgentOutcome::Answer { data, .. } = outcome else {
        panic!("una consulta contesta con datos: {outcome:?}");
    };
    assert_eq!(data["matched"], serde_json::json!(1));
    assert_eq!(data["truncated"], serde_json::json!(false));
}
