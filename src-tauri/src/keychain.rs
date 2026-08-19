//! El único sitio por el que se toca el llavero del sistema.
//!
//! # Por qué existe
//!
//! Una prueba que abre el llavero de verdad hace dos cosas malas a la vez.
//!
//! La primera se ve: macOS le pide la contraseña a quien esté delante, y se la
//! vuelve a pedir **en cada compilación**, porque el binario de pruebas cambia
//! de firma cada vez y el llavero concede permisos por binario. «Permitir
//! siempre» no ayuda: el siguiente ya es otro programa. Pasó de verdad, y lo
//! sufrió quien estaba trabajando en su ordenador mientras la batería corría.
//!
//! La segunda no se ve, y es peor: el resultado de la prueba pasa a depender de
//! qué secretos tenga guardados esa máquina. Deja de ser una prueba.
//!
//! Por eso el acceso pasa por aquí, y al compilar para pruebas se guarda en
//! memoria. El código de producción no cambia de forma: mismas funciones,
//! mismos errores.

/// Lee un secreto del llavero.
#[cfg(not(test))]
pub fn get(service: &str, account: &str) -> keyring::Result<String> {
    keyring::Entry::new(service, account)?.get_password()
}

/// Guarda un secreto en el llavero, sustituyendo el anterior si lo había.
#[cfg(not(test))]
pub fn set(service: &str, account: &str, value: &str) -> keyring::Result<()> {
    keyring::Entry::new(service, account)?.set_password(value)
}

/// Borra un secreto del llavero.
#[cfg(not(test))]
pub fn delete(service: &str, account: &str) -> keyring::Result<()> {
    keyring::Entry::new(service, account)?.delete_credential()
}

/// Llavero en memoria de las pruebas. Vive lo que dure el proceso y no toca el
/// del sistema.
#[cfg(test)]
mod prueba {
    use std::collections::HashMap;
    use std::sync::{Mutex, OnceLock};

    pub(super) fn almacen() -> &'static Mutex<HashMap<(String, String), String>> {
        static ALMACEN: OnceLock<Mutex<HashMap<(String, String), String>>> = OnceLock::new();
        ALMACEN.get_or_init(|| Mutex::new(HashMap::new()))
    }

    pub(super) fn clave(service: &str, account: &str) -> (String, String) {
        (service.to_owned(), account.to_owned())
    }
}

#[cfg(test)]
pub fn get(service: &str, account: &str) -> keyring::Result<String> {
    prueba::almacen()
        .lock()
        .expect("llavero de pruebas")
        .get(&prueba::clave(service, account))
        .cloned()
        .ok_or(keyring::Error::NoEntry)
}

#[cfg(test)]
pub fn set(service: &str, account: &str, value: &str) -> keyring::Result<()> {
    prueba::almacen()
        .lock()
        .expect("llavero de pruebas")
        .insert(prueba::clave(service, account), value.to_owned());
    Ok(())
}

#[cfg(test)]
pub fn delete(service: &str, account: &str) -> keyring::Result<()> {
    prueba::almacen()
        .lock()
        .expect("llavero de pruebas")
        .remove(&prueba::clave(service, account))
        .map(|_| ())
        .ok_or(keyring::Error::NoEntry)
}

#[cfg(test)]
mod tests {
    use super::{delete, get, set};

    #[test]
    fn el_llavero_de_las_pruebas_guarda_lee_y_olvida_sin_tocar_el_del_sistema() {
        let servicio = "io.vindexa.pruebas";
        assert!(matches!(
            get(servicio, "no-existe"),
            Err(keyring::Error::NoEntry)
        ));

        set(servicio, "cuenta", "valor").expect("guardar");
        assert_eq!(get(servicio, "cuenta").expect("leer"), "valor");

        set(servicio, "cuenta", "otro").expect("sustituir");
        assert_eq!(get(servicio, "cuenta").expect("leer"), "otro");

        delete(servicio, "cuenta").expect("borrar");
        assert!(matches!(
            get(servicio, "cuenta"),
            Err(keyring::Error::NoEntry)
        ));
        // Borrar lo que ya no está se distingue de borrar algo: quien llama
        // decide si eso es un error, igual que con el llavero real.
        assert!(matches!(
            delete(servicio, "cuenta"),
            Err(keyring::Error::NoEntry)
        ));
    }
}
