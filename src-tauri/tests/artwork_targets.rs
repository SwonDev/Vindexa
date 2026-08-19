//! Qué carátulas se ofrecen para completar la caché de arte.
//!
//! Esta prueba existe por un fallo concreto: la consulta se escribió mirando la
//! columna equivocada —`last_played_at` vive en `games`, no en la ficha
//! personal— y fallaba entera contra una base real. Como quien la consume se
//! traga el error a propósito —precargar es una mejora de tiempos, no una
//! función—, la aplicación no daba ninguna señal: simplemente no precargaba
//! nada y no había forma de enterarse mirando la pantalla.

#[allow(dead_code)]
#[path = "../src/error.rs"]
mod error;
#[allow(dead_code)]
#[path = "../src/db/library.rs"]
mod library;
#[allow(dead_code)]
#[path = "../src/models.rs"]
mod models;
#[path = "support/schema.rs"]
mod support;

use rusqlite::{Connection, params};

/// El esquema trae las tablas, no los valores iniciales de la aplicación, así
/// que el estado al que apunta cada ficha personal se crea aquí.
fn estado_base(connection: &Connection) {
    connection
        .execute(
            "INSERT OR IGNORE INTO statuses(id, name, color, position, built_in)
             VALUES ('unclassified', 'Sin clasificar', '#8A939E', 0, 1)",
            [],
        )
        .expect("insertar estado base");
}

fn juego(connection: &Connection, app_id: u32, cover: Option<&str>) {
    connection
        .execute(
            "INSERT INTO games(app_id, title, cover_url) VALUES (?1, 'Un juego', ?2)",
            params![app_id, cover],
        )
        .expect("insertar juego");
    connection
        .execute(
            "INSERT INTO game_personal(app_id, status_id) VALUES (?1, 'unclassified')",
            [app_id],
        )
        .expect("insertar ficha personal");
}

#[test]
fn solo_se_ofrece_el_arte_de_lo_que_se_va_a_enseñar() {
    let connection = support::base_en_memoria();
    estado_base(&connection);
    juego(&connection, 10, Some("https://ejemplo/10.jpg"));
    juego(&connection, 20, None);
    juego(&connection, 30, Some(""));
    juego(&connection, 40, Some("https://ejemplo/40.jpg"));
    connection
        .execute("INSERT INTO game_archive(app_id) VALUES (40)", [])
        .expect("archivar");

    let objetivos = library::artwork_targets(&connection).expect("listar carátulas");

    let ids: Vec<u32> = objetivos.iter().map(|target| target.app_id).collect();
    assert_eq!(ids, vec![10], "sin carátula, vacía o archivado no entran");
    assert_eq!(objetivos[0].cover_url, "https://ejemplo/10.jpg");
}

#[test]
fn un_prestamo_sin_confirmar_no_gasta_disco() {
    // No se enseña en la biblioteca, así que descargar su arte sería gastar red
    // y disco en algo que nadie va a mirar.
    let connection = support::base_en_memoria();
    estado_base(&connection);
    connection
        .execute(
            "INSERT INTO games(app_id, title, cover_url, ownership_source, family_availability)
             VALUES (50, 'Prestado', 'https://ejemplo/50.jpg', 'family_shared', 'unknown')",
            [],
        )
        .expect("insertar prestado");
    connection
        .execute(
            "INSERT INTO game_personal(app_id, status_id) VALUES (50, 'unclassified')",
            [],
        )
        .expect("insertar ficha personal");

    assert!(
        library::artwork_targets(&connection)
            .expect("listar")
            .is_empty()
    );
}

#[test]
fn lo_jugado_hace_poco_va_primero() {
    // Se precarga por orden de interés: lo que se ha tocado hace poco es lo que
    // más probablemente se vuelva a mirar.
    let connection = support::base_en_memoria();
    estado_base(&connection);
    juego(&connection, 60, Some("https://ejemplo/60.jpg"));
    juego(&connection, 70, Some("https://ejemplo/70.jpg"));
    connection
        .execute(
            "UPDATE games SET last_played_at = '2026-08-19T10:00:00.000Z' WHERE app_id = 70",
            [],
        )
        .expect("marcar jugado");

    let objetivos = library::artwork_targets(&connection).expect("listar");
    assert_eq!(objetivos.first().map(|target| target.app_id), Some(70));
}
